//! Bounded local provider channel for Linux.
//!
//! The transport is a user-only Unix socket with peer-uid checks, a read
//! deadline, and a max message size. Windows named pipes and macOS adapters
//! are intentionally not implemented. Native window or process identity is
//! still assigned outside this module.

use std::{
    collections::HashMap,
    io::{self, Read},
    os::{fd::AsRawFd, unix::net::UnixStream},
    time::Duration,
};

use uuid::Uuid;

use crate::context::invocation_protocol::{INVOKE_REPLY_BUDGET, ObserveState, ProviderFrame};

pub(crate) const UNIX_READ_TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChannelReject {
    TooLarge,
    Timeout,
    Incomplete,
    PeerMismatch,
}

/// Byte channel for one provider connection. Not tied to X11.
pub(crate) trait ProviderByteChannel {
    fn recv(&mut self, max_bytes: u64) -> Result<Vec<u8>, ChannelReject>;
}

pub(crate) struct UnixSocketChannel {
    stream: UnixStream,
    expected_uid: u32,
    timeout: Duration,
}

impl UnixSocketChannel {
    pub(crate) fn new(stream: UnixStream, expected_uid: u32) -> Self {
        Self {
            stream,
            expected_uid,
            timeout: UNIX_READ_TIMEOUT,
        }
    }
}

impl ProviderByteChannel for UnixSocketChannel {
    fn recv(&mut self, max_bytes: u64) -> Result<Vec<u8>, ChannelReject> {
        recv_unix_message(&mut self.stream, self.expected_uid, max_bytes, self.timeout)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrokerReject {
    Replay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Admission {
    pub generation: u64,
}

#[derive(Default)]
pub(crate) struct InvocationBroker {
    live_generation: HashMap<Uuid, u64>,
    last_generation: HashMap<Uuid, u64>,
    seen_requests: HashMap<Uuid, u64>,
}

impl InvocationBroker {
    pub(crate) fn admit(&mut self, frame: &ProviderFrame) -> Result<Admission, BrokerReject> {
        match frame {
            ProviderFrame::Observe {
                instance_id,
                state: ObserveState::Ended,
                ..
            } => {
                self.live_generation.remove(instance_id);
                Ok(Admission {
                    generation: self.last_generation.get(instance_id).copied().unwrap_or(0),
                })
            }
            ProviderFrame::Observe { instance_id, .. }
            | ProviderFrame::Invoke {
                instance_id,
                request_id: None,
                ..
            } => Ok(Admission {
                generation: self.touch_generation(*instance_id),
            }),
            ProviderFrame::Invoke {
                instance_id,
                request_id: Some(request_id),
                ..
            } => {
                let generation = self.touch_generation(*instance_id);
                if self.seen_requests.contains_key(request_id) {
                    return Err(BrokerReject::Replay);
                }
                self.seen_requests.insert(*request_id, generation);
                Ok(Admission { generation })
            }
        }
    }

    pub(crate) fn live_generation(&self, instance_id: Uuid) -> Option<u64> {
        self.live_generation.get(&instance_id).copied()
    }

    fn touch_generation(&mut self, instance_id: Uuid) -> u64 {
        if let Some(generation) = self.live_generation.get(&instance_id) {
            return *generation;
        }
        let generation = self.last_generation.get(&instance_id).copied().unwrap_or(0) + 1;
        self.last_generation.insert(instance_id, generation);
        self.live_generation.insert(instance_id, generation);
        generation
    }
}

pub(crate) fn recv_unix_message(
    stream: &mut UnixStream,
    expected_uid: u32,
    max_bytes: u64,
    timeout: Duration,
) -> Result<Vec<u8>, ChannelReject> {
    debug_assert!(timeout <= INVOKE_REPLY_BUDGET);
    match peer_uid(stream) {
        Ok(uid) if uid == expected_uid => {}
        _ => return Err(ChannelReject::PeerMismatch),
    }
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(timeout));
    let mut bytes = Vec::new();
    match Read::by_ref(stream)
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
    {
        Ok(_) if bytes.len() as u64 > max_bytes => Err(ChannelReject::TooLarge),
        Ok(_) if bytes.is_empty() => Err(ChannelReject::Incomplete),
        Ok(_) => Ok(bytes),
        Err(error)
            if error.kind() == io::ErrorKind::TimedOut
                || error.kind() == io::ErrorKind::WouldBlock =>
        {
            Err(ChannelReject::Timeout)
        }
        Err(_) => Err(ChannelReject::Incomplete),
    }
}

fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    unsafe {
        let mut cred = std::mem::MaybeUninit::<libc::ucred>::uninit();
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let result = libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            cred.as_mut_ptr().cast(),
            &mut len,
        );
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(cred.assume_init().uid)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        os::unix::net::{UnixListener, UnixStream},
        path::Path,
        thread,
        time::Duration,
    };

    use super::*;
    use crate::context::invocation_protocol::{
        FrameReject, MAX_PROVIDER_MESSAGE_BYTES, parse_provider_frame,
    };

    const INSTANCE: &str = "7af0a690-8948-4f0a-b9f0-51e43c583efa";
    const REQUEST: &str = "c3b1a2d0-1111-4aaa-8bbb-0123456789ab";

    fn uid() -> u32 {
        unsafe { libc::getuid() }
    }

    fn with_socket(test: impl FnOnce(&Path, UnixListener)) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("lyn-broker-test.sock");
        let listener = UnixListener::bind(&path).unwrap();
        test(&path, listener);
    }

    fn recv_from_client(
        listener: UnixListener,
        expected_uid: u32,
        max_bytes: u64,
        timeout: Duration,
        client: impl FnOnce(&Path) + Send + 'static,
        path: &Path,
    ) -> Result<Vec<u8>, ChannelReject> {
        let owned_path = path.to_owned();
        let handle = thread::spawn(move || client(&owned_path));
        listener.set_nonblocking(false).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        let result = recv_unix_message(&mut stream, expected_uid, max_bytes, timeout);
        handle.join().unwrap();
        result
    }

    #[test]
    fn same_user_complete_message_is_accepted() {
        with_socket(|path, listener| {
            let payload = b"{\"version\":1}";
            let received = recv_from_client(
                listener,
                uid(),
                1024,
                Duration::from_millis(200),
                {
                    let payload = payload.to_vec();
                    move |path| {
                        let mut client = UnixStream::connect(path).unwrap();
                        client.write_all(&payload).unwrap();
                    }
                },
                path,
            )
            .unwrap();
            assert_eq!(received, payload);
        });
    }

    #[test]
    fn oversized_messages_are_rejected() {
        with_socket(|path, listener| {
            let result = recv_from_client(
                listener,
                uid(),
                8,
                Duration::from_millis(200),
                |path| {
                    let mut client = UnixStream::connect(path).unwrap();
                    client.write_all(&[b'x'; 16]).unwrap();
                },
                path,
            );
            assert_eq!(result, Err(ChannelReject::TooLarge));
        });
    }

    #[test]
    fn empty_close_is_incomplete() {
        with_socket(|path, listener| {
            let result = recv_from_client(
                listener,
                uid(),
                1024,
                Duration::from_millis(200),
                |path| {
                    let _ = UnixStream::connect(path).unwrap();
                },
                path,
            );
            assert_eq!(result, Err(ChannelReject::Incomplete));
        });
    }

    #[test]
    fn truncated_json_is_read_then_rejected_by_the_parser() {
        with_socket(|path, listener| {
            let result = recv_from_client(
                listener,
                uid(),
                1024,
                Duration::from_millis(200),
                |path| {
                    let mut client = UnixStream::connect(path).unwrap();
                    client.write_all(b"{").unwrap();
                },
                path,
            );
            assert_eq!(result, Ok(b"{".to_vec()));
            assert_eq!(parse_provider_frame(b"{"), Err(FrameReject::InvalidJson));
        });
    }

    #[test]
    fn silent_client_times_out() {
        with_socket(|path, listener| {
            let result = recv_from_client(
                listener,
                uid(),
                1024,
                Duration::from_millis(80),
                |path| {
                    let _hold = UnixStream::connect(path).unwrap();
                    thread::sleep(Duration::from_millis(200));
                },
                path,
            );
            assert_eq!(result, Err(ChannelReject::Timeout));
        });
    }

    #[test]
    fn mismatched_peer_uid_is_rejected_without_reading() {
        with_socket(|path, listener| {
            let result = recv_from_client(
                listener,
                uid().wrapping_add(1),
                1024,
                Duration::from_millis(200),
                |path| {
                    let mut client = UnixStream::connect(path).unwrap();
                    let _ = client.write_all(b"secret");
                },
                path,
            );
            assert_eq!(result, Err(ChannelReject::PeerMismatch));
        });
    }

    #[test]
    fn incompatible_version_is_not_admitted() {
        assert_eq!(
            parse_provider_frame(
                format!(
                    r#"{{"version":9,"instanceId":"{INSTANCE}","state":"focused","workspaceFolders":[]}}"#
                )
                .as_bytes()
            ),
            Err(FrameReject::UnknownVersion)
        );
        assert!(MAX_PROVIDER_MESSAGE_BYTES > 0);
    }

    #[test]
    fn replayed_v2_request_id_is_rejected() {
        let frame = parse_provider_frame(
            format!(
                r#"{{"version":2,"kind":"invoke","instanceId":"{INSTANCE}","requestId":"{REQUEST}","workspaceFolders":["/tmp/a"]}}"#
            )
            .as_bytes(),
        )
        .unwrap();
        let mut broker = InvocationBroker::default();
        assert!(broker.admit(&frame).is_ok());
        assert_eq!(broker.admit(&frame), Err(BrokerReject::Replay));
    }

    #[test]
    fn reconnect_after_ended_assigns_a_new_generation() {
        let instance = Uuid::parse_str(INSTANCE).unwrap();
        let live = parse_provider_frame(
            format!(
                r#"{{"version":1,"instanceId":"{INSTANCE}","state":"focused","workspaceFolders":["/tmp/a"]}}"#
            )
            .as_bytes(),
        )
        .unwrap();
        let ended = parse_provider_frame(
            format!(
                r#"{{"version":1,"instanceId":"{INSTANCE}","state":"ended","workspaceFolders":[]}}"#
            )
            .as_bytes(),
        )
        .unwrap();
        let mut broker = InvocationBroker::default();
        let first = broker.admit(&live).unwrap();
        assert_eq!(broker.live_generation(instance), Some(first.generation));
        broker.admit(&ended).unwrap();
        assert_eq!(broker.live_generation(instance), None);
        let second = broker.admit(&live).unwrap();
        assert_ne!(second.generation, first.generation);
        assert_eq!(broker.live_generation(instance), Some(second.generation));
    }

    #[test]
    fn v1_observe_does_not_consume_a_v2_request_id() {
        let observe = parse_provider_frame(
            format!(
                r#"{{"version":1,"instanceId":"{INSTANCE}","state":"focused","workspaceFolders":["/tmp/a"]}}"#
            )
            .as_bytes(),
        )
        .unwrap();
        let invoke = parse_provider_frame(
            format!(
                r#"{{"version":2,"kind":"invoke","instanceId":"{INSTANCE}","requestId":"{REQUEST}","workspaceFolders":["/tmp/a"]}}"#
            )
            .as_bytes(),
        )
        .unwrap();
        let mut broker = InvocationBroker::default();
        broker.admit(&observe).unwrap();
        assert!(broker.admit(&invoke).is_ok());
    }
}
