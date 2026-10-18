//! Bounded native admission worker. No source registration or capture authority.

use super::{
    gnome_channel::{ChannelError, NativeChannel},
    gnome_listener::{NativeListener, NativeListenerStop},
    gnome_protocol::Candidate,
};
use std::{
    collections::HashMap,
    io,
    os::unix::net::UnixStream,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};
use uuid::Uuid;

const MAX_SERVERS: usize = 8;
type ChannelSlot = Arc<Mutex<NativeChannel>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrokerError {
    Unavailable,
    Busy,
    Channel(ChannelError),
}

/// Correlated but still untrusted: window/PTY/process attestation is mandatory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeEvidence {
    pub(crate) server_pid: u32,
    pub(crate) generation: Uuid,
    pub(crate) candidate: Candidate,
}

#[derive(Default)]
struct Shared {
    channels: Mutex<HashMap<u32, ChannelSlot>>,
    stopping: AtomicBool,
}

impl Shared {
    fn remove(&self, pid: u32, slot: &ChannelSlot) {
        if let Ok(mut channels) = self.channels.lock()
            && channels
                .get(&pid)
                .is_some_and(|current| Arc::ptr_eq(current, slot))
        {
            channels.remove(&pid);
        }
    }

    fn admit(&self, channel: NativeChannel) {
        // Reap idle dead channels before capacity checks. No table lock during
        // kernel/socket checks, and never wait for a channel handling a query.
        let snapshot: Vec<_> = match self.channels.lock() {
            Ok(channels) => channels
                .iter()
                .map(|(&pid, slot)| (pid, slot.clone()))
                .collect(),
            Err(_) => return,
        };
        for (pid, slot) in snapshot {
            if let Ok(mut channel) = slot.try_lock()
                && !channel.revalidate_idle()
            {
                self.remove(pid, &slot);
            }
        }
        if self.stopping.load(Ordering::Acquire) {
            return;
        }
        let Ok(mut channels) = self.channels.lock() else {
            return;
        };
        let pid = channel.peer_pid();
        let previous = channels.get(&pid).cloned();
        let mut previous_guard = if let Some(previous) = previous.as_ref() {
            // Reconnection must not replace an in-flight query or accumulate
            // live retired sockets through cloned registry entries.
            let Ok(previous) = previous.try_lock() else {
                return;
            };
            Some(previous)
        } else if channels.len() >= MAX_SERVERS {
            return;
        } else {
            None
        };
        channels.insert(pid, Arc::new(Mutex::new(channel)));
        drop(channels);
        // Keep the old slot exclusively owned until closed, but do not hold
        // the registry lock across shutdown or any other channel operation.
        if let Some(previous) = previous_guard.as_mut() {
            previous.close();
        }
    }
}

pub(crate) struct NativeBroker {
    shared: Arc<Shared>,
    stop: NativeListenerStop,
    worker: Option<JoinHandle<()>>,
}

impl NativeBroker {
    pub(crate) fn start(runtime: &Path) -> io::Result<Self> {
        Self::spawn(runtime, NativeChannel::connect)
    }

    fn spawn(
        runtime: &Path,
        connect: fn(UnixStream) -> Result<NativeChannel, ChannelError>,
    ) -> io::Result<Self> {
        let (mut listener, stop) = NativeListener::bind(runtime)?;
        let shared = Arc::new(Shared::default());
        let state = shared.clone();
        let worker = thread::Builder::new()
            .name("lyn-gnome-admission".into())
            .spawn(move || {
                while !state.stopping.load(Ordering::Acquire) {
                    let Ok(Some(stream)) = listener.accept() else {
                        break;
                    };
                    // One handshake at a time, bounded by NativeChannel's 500ms
                    // deadline. No thread-per-client or capture/session lock.
                    if let Ok(channel) = connect(stream) {
                        state.admit(channel);
                    }
                }
                state.stopping.store(true, Ordering::Release);
                let retired = match state.channels.lock() {
                    Ok(mut channels) => std::mem::take(&mut *channels),
                    Err(_) => HashMap::new(),
                };
                // Destruction closes sockets and pidfds: drop them only after
                // the registry guard has been released, including accept errors.
                drop(retired);
            })?;
        Ok(Self {
            shared,
            stop,
            worker: Some(worker),
        })
    }

    /// Resolve the owner from the local X server, then revalidate it after the
    /// native exchange. Evidence still needs PTY/process attestation.
    pub(crate) fn query_window(
        &self,
        window_id: u32,
        invoked_us: u64,
    ) -> Result<NativeEvidence, BrokerError> {
        self.query_window_with(
            window_id,
            invoked_us,
            crate::platform::x11_owner::local_owner,
        )
    }

    fn query_window_with(
        &self,
        window_id: u32,
        invoked_us: u64,
        mut owner: impl FnMut(u32) -> Option<u32>,
    ) -> Result<NativeEvidence, BrokerError> {
        let pid = owner(window_id).ok_or(BrokerError::Unavailable)?;
        let evidence = self.query(pid, window_id, invoked_us)?;
        if owner(window_id) != Some(pid) || !self.connection_is_current(&evidence) {
            return Err(BrokerError::Unavailable);
        }
        Ok(evidence)
    }

    fn connection_is_current(&self, evidence: &NativeEvidence) -> bool {
        let slot = match self.shared.channels.lock() {
            Ok(channels) => channels.get(&evidence.server_pid).cloned(),
            Err(_) => return false,
        };
        let Some(slot) = slot else {
            return false;
        };
        let Ok(mut channel) = slot.try_lock() else {
            return false;
        };
        if channel.generation() != evidence.generation || !channel.revalidate_idle() {
            return false;
        }
        // Revalidation did kernel/socket checks without the registry mutex.
        // Hold the slot guard through this final fence so admission cannot
        // replace the connection between validation and the observation point.
        !self.shared.stopping.load(Ordering::Acquire)
            && self.shared.channels.lock().is_ok_and(|channels| {
                channels
                    .get(&evidence.server_pid)
                    .is_some_and(|current| Arc::ptr_eq(current, &slot))
            })
    }

    /// `server_pid` must come from verified frozen-window ownership, not a
    /// provider payload or latest-report heuristic. No fallback to other peers.
    /// Call on a context worker, never while holding the capture-session mutex.
    fn query(
        &self,
        server_pid: u32,
        window_id: u32,
        invoked_us: u64,
    ) -> Result<NativeEvidence, BrokerError> {
        if self.shared.stopping.load(Ordering::Acquire) {
            return Err(BrokerError::Unavailable);
        }
        let slot = self
            .shared
            .channels
            .lock()
            .map_err(|_| BrokerError::Unavailable)?
            .get(&server_pid)
            .cloned()
            .ok_or(BrokerError::Unavailable)?;
        let mut channel = slot.try_lock().map_err(|_| BrokerError::Busy)?;
        // A reconnect may have retired this clone before try_lock succeeded.
        let current = self
            .shared
            .channels
            .lock()
            .map_err(|_| BrokerError::Unavailable)?
            .get(&server_pid)
            .is_some_and(|current| Arc::ptr_eq(current, &slot));
        if !current {
            return Err(BrokerError::Unavailable);
        }
        let generation = channel.generation();
        let result = channel.query(window_id, invoked_us);
        if !channel.is_open() {
            self.shared.remove(server_pid, &slot);
        }
        if self.shared.stopping.load(Ordering::Acquire) {
            return Err(BrokerError::Unavailable);
        }
        result
            .map(|candidate| NativeEvidence {
                server_pid,
                generation,
                candidate,
            })
            .map_err(BrokerError::Channel)
    }

    #[cfg(test)]
    fn start_fixture(runtime: &Path) -> io::Result<Self> {
        Self::spawn(runtime, NativeChannel::connect_fixture)
    }
}

impl Drop for NativeBroker {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::Release);
        let _ = self.stop.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
#[path = "gnome_broker_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "gnome_probe_tests.rs"]
mod probe_tests;
