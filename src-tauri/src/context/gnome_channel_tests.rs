use super::super::gnome_protocol::Outcome;
use super::*;
use std::{
    io::{Read, Write},
    thread,
    time::{Duration, Instant},
};

fn deadline() -> u64 {
    monotonic_us().unwrap() + 500_000
}

fn assert_disconnected(client: &mut UnixStream) {
    match client.read(&mut [0; 1]) {
        Ok(0) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::NotConnected
            ) => {}
        other => panic!("expected closed socket, got {other:?}"),
    }
}

fn handshake(client: &mut UnixStream) -> Uuid {
    client
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    client.set_nonblocking(true).unwrap();
    write_frame(client, Frame::Hello, deadline()).unwrap();
    let Frame::Bind { generation } = read_frame(client, deadline()).unwrap() else {
        panic!("expected bind")
    };
    write_frame(client, Frame::Ready { generation }, deadline()).unwrap();
    generation
}

fn reply(query: Frame, outcome: Option<Outcome>) -> Frame {
    let Frame::Query {
        correlation,
        invoked_us,
    } = query
    else {
        panic!("expected query")
    };
    Frame::Reply {
        correlation,
        observed_us: monotonic_us().unwrap(),
        outcome: outcome.unwrap_or(Outcome::Candidate(Candidate {
            source_id: 17,
            pty_number: 0,
            foreground_pgid: std::process::id(),
            registered_us: invoked_us - 2,
            changed_us: invoked_us - 1,
            revision: 1,
        })),
    }
}

fn connect(server: UnixStream) -> NativeChannel {
    let peer = NativePeer::fixture(&server).unwrap();
    NativeChannel::handshake(server, peer, deadline()).unwrap()
}

#[test]
fn same_user_non_native_client_cannot_open_production_channel() {
    let (server, _client) = UnixStream::pair().unwrap();
    assert!(NativeChannel::connect(server).is_err());
}

#[test]
fn fragmented_frames_complete_and_correlate_one_native_query() {
    let (server, mut client) = UnixStream::pair().unwrap();
    let worker = thread::spawn(move || {
        client
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        for fragment in Frame::Hello.encode().chunks(7) {
            client.write_all(fragment).unwrap();
        }
        client.set_nonblocking(true).unwrap();
        let Frame::Bind { generation } = read_frame(&mut client, deadline()).unwrap() else {
            panic!()
        };
        write_frame(&mut client, Frame::Ready { generation }, deadline()).unwrap();
        let query = read_frame(&mut client, deadline()).unwrap();
        write_frame(&mut client, reply(query, None), deadline()).unwrap();
        // Keep the socket alive until the caller has validated its reply.
        client.set_nonblocking(false).unwrap();
        assert_eq!(client.read(&mut [0; 1]).unwrap(), 0);
    });
    let mut channel = connect(server);
    assert_eq!(channel.peer_pid(), std::process::id());
    let result = channel.query(77, monotonic_us().unwrap()).unwrap();
    assert_eq!(result.source_id, 17);
    assert_eq!(result.foreground_pgid, std::process::id());
    drop(channel);
    worker.join().unwrap();
}

#[test]
fn incorrect_ready_generation_closes_admission() {
    let (server, mut client) = UnixStream::pair().unwrap();
    let worker = thread::spawn(move || {
        client.set_nonblocking(true).unwrap();
        write_frame(&mut client, Frame::Hello, deadline()).unwrap();
        assert!(matches!(
            read_frame(&mut client, deadline()),
            Ok(Frame::Bind { .. })
        ));
        write_frame(
            &mut client,
            Frame::Ready {
                generation: Uuid::new_v4(),
            },
            deadline(),
        )
        .unwrap();
    });
    let peer = NativePeer::fixture(&server).unwrap();
    assert!(NativeChannel::handshake(server, peer, deadline()).is_err());
    worker.join().unwrap();
}

#[test]
fn wrong_reply_request_disconnects_instead_of_completing_another_capture() {
    let (server, mut client) = UnixStream::pair().unwrap();
    let worker = thread::spawn(move || {
        handshake(&mut client);
        let mut response = reply(read_frame(&mut client, deadline()).unwrap(), None);
        if let Frame::Reply { correlation, .. } = &mut response {
            correlation.request_id = Uuid::new_v4();
        }
        write_frame(&mut client, response, deadline()).unwrap();
        client.set_nonblocking(false).unwrap();
        assert_eq!(client.read(&mut [0; 1]).unwrap(), 0);
    });
    let mut channel = connect(server);
    assert_eq!(
        channel.query(77, monotonic_us().unwrap()),
        Err(ChannelError::Reply(ReplyReject::Correlation))
    );
    assert_eq!(
        channel.query(77, monotonic_us().unwrap()),
        Err(ChannelError::Disconnected)
    );
    worker.join().unwrap();
}

#[test]
fn explicit_refusal_does_not_poison_the_next_distinct_request() {
    let (server, mut client) = UnixStream::pair().unwrap();
    let worker = thread::spawn(move || {
        handshake(&mut client);
        let first = read_frame(&mut client, deadline()).unwrap();
        write_frame(
            &mut client,
            reply(first, Some(Outcome::Missing)),
            deadline(),
        )
        .unwrap();
        let second = read_frame(&mut client, deadline()).unwrap();
        assert_ne!(first, second);
        write_frame(&mut client, reply(second, None), deadline()).unwrap();
        client.set_nonblocking(false).unwrap();
        assert_eq!(client.read(&mut [0; 1]).unwrap(), 0);
    });
    let mut channel = connect(server);
    assert_eq!(
        channel.query(77, monotonic_us().unwrap()),
        Err(ChannelError::Reply(ReplyReject::Unresolved(
            Outcome::Missing
        )))
    );
    assert!(channel.query(77, monotonic_us().unwrap()).is_ok());
    drop(channel);
    worker.join().unwrap();
}

#[test]
fn slow_drip_cannot_extend_the_absolute_frame_deadline() {
    let (mut server, mut client) = UnixStream::pair().unwrap();
    server.set_nonblocking(true).unwrap();
    let worker = thread::spawn(move || {
        for byte in Frame::Hello.encode() {
            if client.write_all(&[byte]).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
    });
    let started = Instant::now();
    assert_eq!(
        read_frame(&mut server, monotonic_us().unwrap() + 35_000),
        Err(ChannelError::Timeout)
    );
    assert!(started.elapsed() < Duration::from_millis(250));
    drop(server);
    worker.join().unwrap();
}

#[test]
fn truncated_and_noncanonical_frames_are_refused() {
    for truncated in [true, false] {
        let (mut server, mut client) = UnixStream::pair().unwrap();
        server.set_nonblocking(true).unwrap();
        let mut bytes = Frame::Hello.encode();
        bytes[111] = 1;
        client
            .write_all(if truncated { &bytes[..61] } else { &bytes })
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        assert_eq!(
            read_frame(&mut server, deadline()),
            Err(if truncated {
                ChannelError::Disconnected
            } else {
                ChannelError::Protocol
            })
        );
    }
}

#[test]
fn extra_byte_after_hello_is_not_silently_accepted() {
    let (server, mut client) = UnixStream::pair().unwrap();
    let mut oversized = Frame::Hello.encode().to_vec();
    oversized.push(1);
    client.write_all(&oversized).unwrap();
    let peer = NativePeer::fixture(&server).unwrap();
    assert!(matches!(
        NativeChannel::handshake(server, peer, deadline()),
        Err(ChannelError::Protocol)
    ));
}

#[test]
fn ancillary_file_descriptors_are_rejected_without_becoming_native_metadata() {
    let (mut server, client) = UnixStream::pair().unwrap();
    server.set_nonblocking(true).unwrap();
    let file = std::fs::File::open("/dev/null").unwrap();
    let bytes = Frame::Hello.encode();
    let mut control = [0_usize; 4];
    let mut payload = libc::iovec {
        iov_base: bytes.as_ptr().cast_mut().cast(),
        iov_len: bytes.len(),
    };
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut payload;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = unsafe { libc::CMSG_SPACE(4) } as usize;
    unsafe {
        let header = libc::CMSG_FIRSTHDR(&message);
        (*header).cmsg_len = libc::CMSG_LEN(4) as usize;
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        std::ptr::write(libc::CMSG_DATA(header).cast::<i32>(), file.as_raw_fd());
        assert_eq!(
            libc::sendmsg(client.as_raw_fd(), &message, libc::MSG_NOSIGNAL),
            FRAME_BYTES as isize
        );
    }
    assert_eq!(
        read_frame(&mut server, deadline()),
        Err(ChannelError::Protocol)
    );
}

#[test]
fn duplicate_queued_reply_is_rejected_not_reused_as_another_context() {
    let (server, mut client) = UnixStream::pair().unwrap();
    let worker = thread::spawn(move || {
        handshake(&mut client);
        let frame = reply(read_frame(&mut client, deadline()).unwrap(), None);
        let mut bytes = frame.encode().to_vec();
        bytes.extend_from_slice(&frame.encode());
        client.write_all(&bytes).unwrap();
        client.set_nonblocking(false).unwrap();
        // Closing with a duplicate still unread may produce ECONNRESET, not EOF.
        assert_disconnected(&mut client);
    });
    let mut channel = connect(server);
    assert_eq!(
        channel.query(77, monotonic_us().unwrap()),
        Err(ChannelError::Protocol)
    );
    assert_eq!(
        channel.query(77, monotonic_us().unwrap()),
        Err(ChannelError::Disconnected)
    );
    worker.join().unwrap();
}

#[test]
fn a_silent_module_times_out_and_its_generation_cannot_be_reused() {
    let (server, mut client) = UnixStream::pair().unwrap();
    let worker = thread::spawn(move || {
        handshake(&mut client);
        assert!(matches!(
            read_frame(&mut client, deadline()),
            Ok(Frame::Query { .. })
        ));
        // Silence until the server's absolute deadline closes the connection.
        client.set_nonblocking(false).unwrap();
        assert_eq!(client.read(&mut [0; 1]).unwrap(), 0);
    });
    let mut channel = connect(server);
    let started = Instant::now();
    assert_eq!(
        channel.query(77, monotonic_us().unwrap()),
        Err(ChannelError::Timeout)
    );
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(
        channel.query(77, monotonic_us().unwrap()),
        Err(ChannelError::Disconnected)
    );
    worker.join().unwrap();
}

#[test]
fn a_future_invocation_cannot_create_an_unbounded_socket_deadline() {
    let (server, mut client) = UnixStream::pair().unwrap();
    let worker = thread::spawn(move || {
        handshake(&mut client);
        client.set_nonblocking(false).unwrap();
        assert_eq!(
            client.read(&mut [0; 1]).unwrap(),
            0,
            "no query may be sent for a future timestamp"
        );
    });
    let mut channel = connect(server);
    assert_eq!(
        channel.query(77, monotonic_us().unwrap() + 1_000_000_000),
        Err(ChannelError::Protocol)
    );
    worker.join().unwrap();
}
