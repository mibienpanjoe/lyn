//! Bounded native admission worker. No source registration or capture authority.

use super::{
    gnome_channel::{ChannelError, NativeChannel},
    gnome_endpoint::NativePublication,
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

/// Observation of at least one admitted transport, not foreground/source or
/// capture proof. Busy means observation would queue behind a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeConnectionStatus {
    Connected,
    Disconnected,
    Busy,
    Unavailable,
}

/// Correlated but still untrusted: window/PTY/process attestation is mandatory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeEvidence {
    pub(crate) server_pid: u32,
    pub(crate) generation: Uuid,
    pub(crate) candidate: Candidate,
}

pub(crate) struct NativeProcessEvidence {
    pub(crate) native: NativeEvidence,
    window_id: u32,
    group: super::gnome_process::ForegroundGroup,
}

impl NativeProcessEvidence {
    pub(crate) fn directory(&self) -> &Path {
        &self.group.directory
    }

    pub(crate) fn revalidate(&self, broker: &NativeBroker) -> Result<(), BrokerError> {
        self.group
            .revalidate_lifetimes()
            .map_err(|_| BrokerError::Unavailable)?;
        let now =
            super::gnome_process::ProcessInvocation::now().map_err(|_| BrokerError::Unavailable)?;
        let current = broker
            .query_window_process(self.window_id, now)
            .inspect_err(|error| {
                #[cfg(test)]
                eprintln!("native continuity query refused: {error:?}");
                #[cfg(not(test))]
                let _ = error;
            })?;
        if current.native != self.native || current.group != self.group {
            #[cfg(test)]
            eprintln!(
                "native continuity mismatch: topology={}, group={}",
                current.native != self.native,
                current.group != self.group
            );
            return Err(BrokerError::Unavailable);
        }
        self.group
            .revalidate_lifetimes()
            .map_err(|_| BrokerError::Unavailable)
    }
}

/// A source cannot be assembled from an ordinary provider payload. It retains
/// the original kernel pins and the broker generation through selection/save.
pub(crate) struct NativeSourceLease {
    broker: Arc<NativeBroker>,
    evidence: NativeProcessEvidence,
}

impl NativeSourceLease {
    pub(crate) fn query(
        broker: Arc<NativeBroker>,
        window: u32,
        invocation: super::gnome_process::ProcessInvocation,
    ) -> Result<Self, BrokerError> {
        let evidence = broker.query_window_process(window, invocation)?;
        Ok(Self { broker, evidence })
    }
    pub(crate) fn directory(&self) -> &Path {
        self.evidence.directory()
    }
    pub(crate) fn window(&self) -> u32 {
        self.evidence.window_id
    }
    pub(crate) fn revalidate(&self) -> bool {
        self.evidence.revalidate(&self.broker).is_ok()
    }
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
    worker: Mutex<Option<JoinHandle<()>>>,
}

struct BrokerEndpoint {
    // Declaration order also protects thread-spawn failure/panic cleanup:
    // remove the owned socket before removing the empty instance directory.
    listener: NativeListener,
    publication: Option<NativePublication>,
}

impl NativeBroker {
    pub(crate) fn connection_status(&self) -> NativeConnectionStatus {
        use std::sync::TryLockError;
        if self.shared.stopping.load(Ordering::Acquire) {
            return NativeConnectionStatus::Unavailable;
        }
        let snapshot: Vec<_> = match self.shared.channels.try_lock() {
            Ok(channels) => channels
                .iter()
                .map(|(&pid, slot)| (pid, slot.clone()))
                .collect(),
            Err(TryLockError::WouldBlock) => return NativeConnectionStatus::Busy,
            Err(TryLockError::Poisoned(_)) => return NativeConnectionStatus::Unavailable,
        };
        let mut status = NativeConnectionStatus::Disconnected;
        let mut connected = false;
        for (pid, slot) in snapshot {
            let mut channel = match slot.try_lock() {
                Ok(channel) => channel,
                Err(TryLockError::WouldBlock) => {
                    status = NativeConnectionStatus::Busy;
                    continue;
                }
                Err(TryLockError::Poisoned(_)) => {
                    return NativeConnectionStatus::Unavailable;
                }
            };
            // Kernel/idle checks only: no provider request, reconnect, source
            // selection or session mutation. No table mutex spans this I/O.
            if !channel.revalidate_idle() {
                continue;
            }
            let current = match self.shared.channels.try_lock() {
                Ok(channels) => channels
                    .get(&pid)
                    .is_some_and(|current| Arc::ptr_eq(current, &slot)),
                Err(TryLockError::WouldBlock) => {
                    status = NativeConnectionStatus::Busy;
                    continue;
                }
                Err(TryLockError::Poisoned(_)) => {
                    return NativeConnectionStatus::Unavailable;
                }
            };
            if self.shared.stopping.load(Ordering::Acquire) {
                return NativeConnectionStatus::Unavailable;
            }
            if current {
                connected = true;
            } else {
                status = NativeConnectionStatus::Busy;
            }
        }
        if self.shared.stopping.load(Ordering::Acquire) {
            NativeConnectionStatus::Unavailable
        } else if connected {
            NativeConnectionStatus::Connected
        } else {
            status
        }
    }

    #[cfg(test)]
    pub(crate) fn start(runtime: &Path) -> io::Result<Self> {
        Self::spawn(runtime, NativeChannel::connect)
    }

    pub(crate) fn start_published(runtime: &Path) -> io::Result<Self> {
        let publication = NativePublication::prepare(runtime)?;
        let (listener, stop) = NativeListener::bind(publication.instance_path())?;
        Self::spawn_endpoint(
            BrokerEndpoint {
                listener,
                publication: Some(publication),
            },
            stop,
            NativeChannel::connect,
        )
    }

    #[cfg(test)]
    fn spawn(
        runtime: &Path,
        connect: fn(UnixStream) -> Result<NativeChannel, ChannelError>,
    ) -> io::Result<Self> {
        let (listener, stop) = NativeListener::bind(runtime)?;
        Self::spawn_endpoint(
            BrokerEndpoint {
                listener,
                publication: None,
            },
            stop,
            connect,
        )
    }

    fn spawn_endpoint(
        mut endpoint: BrokerEndpoint,
        stop: NativeListenerStop,
        connect: fn(UnixStream) -> Result<NativeChannel, ChannelError>,
    ) -> io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let state = shared.clone();
        let (ready, started) = std::sync::mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("lyn-gnome-admission".into())
            .spawn(move || {
                // Publish only from the live worker after bind/listen. The
                // caller receives either readiness or the publication error.
                let result = match endpoint.publication.as_mut() {
                    Some(publication) => publication.publish(&endpoint.listener),
                    None => Ok(()),
                };
                let running = result.is_ok();
                let acknowledged = ready.send(result).is_ok();
                while running && acknowledged && !state.stopping.load(Ordering::Acquire) {
                    let Ok(Some(stream)) = endpoint.listener.accept() else {
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
                drop(endpoint);
            })?;
        let startup = started
            .recv()
            .unwrap_or_else(|_| Err(io::Error::other("native worker exited before readiness")));
        if let Err(error) = startup {
            let _ = stop.stop();
            let _ = worker.join();
            return Err(error);
        }
        Ok(Self {
            shared,
            stop,
            worker: Mutex::new(Some(worker)),
        })
    }

    /// Tauri exits the process without dropping all managed Arc owners. Retire
    /// this lifetime explicitly before exit, even while leases retain an Arc.
    /// Concurrent/repeated callers wait for the same worker to finish; this
    /// lifecycle mutex is never used by admission or capture queries.
    pub(crate) fn shutdown(&self) {
        self.shared.stopping.store(true, Ordering::Release);
        let _ = self.stop.stop();
        let mut worker = self
            .worker
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(worker) = worker.take() {
            let _ = worker.join();
        }
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
        let pid = owner(window_id).ok_or_else(|| {
            #[cfg(test)]
            eprintln!(r#"{{"check":"native_window_query_refused","stage":"initial_owner"}}"#);
            BrokerError::Unavailable
        })?;
        let evidence = self.query(pid, window_id, invoked_us).inspect_err(|_| {
            #[cfg(test)]
            eprintln!(r#"{{"check":"native_window_query_refused","stage":"native_exchange"}}"#);
        })?;
        if owner(window_id) != Some(pid) {
            #[cfg(test)]
            eprintln!(r#"{{"check":"native_window_query_refused","stage":"final_owner"}}"#);
            return Err(BrokerError::Unavailable);
        }
        if !self.connection_is_current(&evidence) {
            #[cfg(test)]
            eprintln!(r#"{{"check":"native_window_query_refused","stage":"final_channel"}}"#);
            return Err(BrokerError::Unavailable);
        }
        Ok(evidence)
    }

    /// Additional live kernel master-PTY proof. This is not process/cwd proof
    /// and must not yet be promoted to a registered context source.
    pub(crate) fn query_window_pty(
        &self,
        window_id: u32,
        invoked_us: u64,
    ) -> Result<NativeEvidence, BrokerError> {
        self.query_window_pty_with(
            window_id,
            invoked_us,
            crate::platform::x11_owner::local_owner,
            |pid, candidate, deadline| {
                super::gnome_pty::inspect(pid, candidate.pty_number, deadline)
            },
        )
        .map(|(evidence, _)| evidence)
    }

    pub(crate) fn query_window_process(
        &self,
        window_id: u32,
        invocation: super::gnome_process::ProcessInvocation,
    ) -> Result<NativeProcessEvidence, BrokerError> {
        let invoked_us = invocation.monotonic_us();
        let (native, (_, group)) = self.query_window_pty_with(
            window_id,
            invoked_us,
            crate::platform::x11_owner::local_owner,
            |pid, candidate, deadline| {
                let pty = super::gnome_pty::inspect(pid, candidate.pty_number, deadline)
                    .inspect_err(|error| {
                        #[cfg(test)]
                        eprintln!("native PTY inspection refused: {:?}", error.kind());
                        #[cfg(not(test))]
                        let _ = error;
                    })?;
                let group = super::gnome_process::inspect(
                    pid,
                    candidate.foreground_pgid,
                    pty.slave_device,
                    invocation,
                    deadline,
                )
                .inspect_err(|error| {
                    #[cfg(test)]
                    eprintln!("native process inspection refused: {:?}", error.kind());
                    #[cfg(not(test))]
                    let _ = error;
                })?;
                Ok((pty, group))
            },
        )?;
        group
            .revalidate_lifetimes()
            .map_err(|_| BrokerError::Unavailable)?;
        let deadline = invoked_us
            .checked_add(super::gnome_protocol::REPLY_BUDGET_US)
            .ok_or(BrokerError::Unavailable)?;
        if super::gnome_channel::monotonic_us().map_err(BrokerError::Channel)? >= deadline {
            #[cfg(test)]
            eprintln!("native process lifetime fence exceeded deadline");
            return Err(BrokerError::Unavailable);
        }
        Ok(NativeProcessEvidence {
            native,
            window_id,
            group,
        })
    }

    fn query_window_pty_with<W: PartialEq>(
        &self,
        window_id: u32,
        invoked_us: u64,
        mut owner: impl FnMut(u32) -> Option<u32>,
        mut inspect: impl FnMut(u32, Candidate, u64) -> io::Result<W>,
    ) -> Result<(NativeEvidence, W), BrokerError> {
        let evidence = self.query_window_with(window_id, invoked_us, &mut owner)?;
        let deadline = invoked_us
            .checked_add(super::gnome_protocol::REPLY_BUDGET_US)
            .ok_or(BrokerError::Unavailable)?;
        let slot = self
            .shared
            .channels
            .lock()
            .map_err(|_| BrokerError::Unavailable)?
            .get(&evidence.server_pid)
            .cloned()
            .ok_or(BrokerError::Unavailable)?;
        let mut channel = slot.try_lock().map_err(|_| BrokerError::Busy)?;
        if channel.generation() != evidence.generation || !channel.revalidate_idle() {
            #[cfg(test)]
            eprintln!("native pre-inspection channel fence refused");
            return Err(BrokerError::Unavailable);
        }
        let before = inspect(evidence.server_pid, evidence.candidate, deadline)
            .map_err(|_| BrokerError::Unavailable)?;
        // Inodes and indices can recycle. Finish kernel observations BEFORE
        // confirming GTK source lifetime/revision against the frozen invocation.
        if inspect(evidence.server_pid, evidence.candidate, deadline)
            .map_err(|_| BrokerError::Unavailable)?
            != before
        {
            #[cfg(test)]
            eprintln!("native kernel snapshots differ");
            return Err(BrokerError::Unavailable);
        }
        // Re-query the frozen window under the ORIGINAL invocation fence.
        let confirmed = channel
            .query(window_id, invoked_us)
            .map_err(BrokerError::Channel)?;
        if confirmed != evidence.candidate || !channel.revalidate_idle() {
            #[cfg(test)]
            eprintln!("native post-inspection channel/candidate fence refused");
            return Err(BrokerError::Unavailable);
        }
        drop(channel);
        if owner(window_id) != Some(evidence.server_pid)
            || !self.connection_is_current(&evidence)
            || super::gnome_channel::monotonic_us().map_err(BrokerError::Channel)? >= deadline
        {
            #[cfg(test)]
            eprintln!("native final owner/generation/deadline fence refused");
            return Err(BrokerError::Unavailable);
        }
        Ok((evidence, before))
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
    pub(crate) fn fixture_generation(&self) -> Option<Uuid> {
        let channels = self.shared.channels.lock().ok()?;
        if channels.len() != 1 || self.shared.stopping.load(Ordering::Acquire) {
            return None;
        }
        // Read only already-admitted local state, without a provider exchange,
        // reconnection, source selection or blocking on a busy channel.
        let channel = channels.values().next()?.try_lock().ok()?;
        channel.is_open().then(|| channel.generation())
    }

    #[cfg(test)]
    pub(crate) fn reconnect_fixture_peer(&self) -> Option<(Uuid, Uuid)> {
        // Fault injection only: retire exactly one real admitted channel. Its
        // replacement must still pass the production peer/handshake boundary.
        let (pid, slot) = {
            let channels = self.shared.channels.lock().ok()?;
            if channels.len() != 1 {
                return None;
            }
            let (&pid, slot) = channels.iter().next()?;
            (pid, slot.clone())
        };
        let original = slot.try_lock().ok()?.generation();
        self.shared.remove(pid, &slot);
        drop(slot);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            let current = self.shared.channels.lock().ok()?.get(&pid).cloned();
            if let Some(current) = current
                && let Ok(channel) = current.try_lock()
                && channel.generation() != original
            {
                return Some((original, channel.generation()));
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        None
    }

    #[cfg(test)]
    fn start_fixture(runtime: &Path) -> io::Result<Self> {
        Self::spawn(runtime, NativeChannel::connect_fixture)
    }
}

impl Drop for NativeBroker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
#[path = "gnome_broker_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "gnome_probe_tests.rs"]
mod probe_tests;
