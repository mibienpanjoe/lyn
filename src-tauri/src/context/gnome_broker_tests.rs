use super::super::gnome_channel::monotonic_us;
use super::super::gnome_protocol::{FRAME_BYTES, Frame, Outcome, ReplyReject};
use super::*;
use std::{
    fs,
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    thread,
    time::{Duration, Instant},
};

fn answer(stream: &mut UnixStream) {
    let Frame::Query {
        correlation,
        invoked_us,
    } = receive(stream)
    else {
        panic!("expected Query")
    };
    stream
        .write_all(
            &Frame::Reply {
                correlation,
                observed_us: monotonic_us().unwrap(),
                outcome: Outcome::Candidate(super::super::gnome_protocol::Candidate {
                    source_id: u64::from(std::process::id()),
                    pty_number: 0,
                    foreground_pgid: std::process::id(),
                    registered_us: invoked_us - 2,
                    changed_us: invoked_us - 1,
                    revision: 1,
                }),
            }
            .encode(),
        )
        .unwrap();
}

fn assert_closed(stream: &mut UnixStream) {
    match stream.read(&mut [0; 1]) {
        Ok(0) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
            ) => {}
        other => panic!("expected closed socket, got {other:?}"),
    }
}

fn runtime() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

fn receive(stream: &mut UnixStream) -> Frame {
    let mut bytes = [0; FRAME_BYTES];
    stream.read_exact(&mut bytes).unwrap();
    Frame::decode(&bytes).unwrap()
}

fn client_handshake(stream: &mut UnixStream) -> uuid::Uuid {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream.write_all(&Frame::Hello.encode()).unwrap();
    let Frame::Bind { generation } = receive(stream) else {
        panic!("expected Bind")
    };
    stream
        .write_all(&Frame::Ready { generation }.encode())
        .unwrap();
    generation
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !condition() {
        assert!(Instant::now() < deadline, "broker state did not settle");
        thread::yield_now();
    }
}

#[test]
fn idle_broker_drop_joins_and_releases_the_endpoint() {
    let runtime = runtime();
    let broker = NativeBroker::start(runtime.path()).unwrap();
    let path = runtime.path().join("lyn-gnome-v1.sock");
    assert!(path.exists());
    let started = Instant::now();
    drop(broker);
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(!path.exists());
}

#[test]
fn ordinary_same_user_client_is_not_admitted_by_production_broker() {
    let runtime = runtime();
    let broker = NativeBroker::start(runtime.path()).unwrap();
    let mut client = UnixStream::connect(runtime.path().join("lyn-gnome-v1.sock")).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    assert_eq!(client.read(&mut [0; 1]).unwrap(), 0);
    assert!(broker.shared.channels.lock().unwrap().is_empty());
    assert_eq!(
        broker.query(std::process::id(), 77, monotonic_us().unwrap()),
        Err(BrokerError::Unavailable)
    );
}

#[test]
fn fixture_server_receives_only_exact_pid_requests_and_unresolved_stays_reusable() {
    let runtime = runtime();
    let broker = NativeBroker::start_fixture(runtime.path()).unwrap();
    let path = runtime.path().join("lyn-gnome-v1.sock");
    let worker = thread::spawn(move || {
        let mut client = UnixStream::connect(path).unwrap();
        let generation = client_handshake(&mut client);
        for expected_window in [77, 88] {
            let Frame::Query { correlation, .. } = receive(&mut client) else {
                panic!("expected Query")
            };
            assert_eq!(correlation.generation, generation);
            assert_eq!(correlation.window_id, expected_window);
            client
                .write_all(
                    &Frame::Reply {
                        correlation,
                        observed_us: monotonic_us().unwrap(),
                        outcome: Outcome::Missing,
                    }
                    .encode(),
                )
                .unwrap();
        }
        assert_eq!(client.read(&mut [0; 1]).unwrap(), 0);
    });
    wait_until(|| broker.shared.channels.lock().unwrap().len() == 1);
    assert_eq!(
        broker.query(u32::MAX, 12, monotonic_us().unwrap()),
        Err(BrokerError::Unavailable)
    );
    for window in [77, 88] {
        assert_eq!(
            broker.query(std::process::id(), window, monotonic_us().unwrap()),
            Err(BrokerError::Channel(ChannelError::Reply(
                ReplyReject::Unresolved(Outcome::Missing)
            )))
        );
    }
    drop(broker);
    worker.join().unwrap();
}

#[test]
fn idle_reconnect_replaces_generation_and_closes_the_old_socket() {
    let runtime = runtime();
    let broker = NativeBroker::start_fixture(runtime.path()).unwrap();
    let path = runtime.path().join("lyn-gnome-v1.sock");
    let mut old = UnixStream::connect(&path).unwrap();
    let old_generation = client_handshake(&mut old);
    wait_until(|| broker.shared.channels.lock().unwrap().len() == 1);
    let mut new = UnixStream::connect(&path).unwrap();
    let generation = client_handshake(&mut new);
    assert_ne!(generation, old_generation);
    wait_until(|| {
        broker
            .shared
            .channels
            .lock()
            .unwrap()
            .get(&std::process::id())
            .is_some_and(|slot| {
                slot.try_lock()
                    .is_ok_and(|channel| channel.generation() == generation)
            })
    });
    assert_closed(&mut old);
    let client = thread::spawn(move || {
        answer(&mut new);
        assert_closed(&mut new);
    });
    let evidence = broker
        .query(std::process::id(), 77, monotonic_us().unwrap())
        .unwrap();
    assert_eq!(evidence.server_pid, std::process::id());
    assert_eq!(evidence.generation, generation);
    assert_eq!(evidence.candidate.source_id, u64::from(std::process::id()));
    drop(broker);
    client.join().unwrap();
}

#[test]
fn busy_channel_refuses_a_second_query_without_holding_registry_lock() {
    let runtime = runtime();
    let broker = Arc::new(NativeBroker::start_fixture(runtime.path()).unwrap());
    let path = runtime.path().join("lyn-gnome-v1.sock");
    let (sent, received) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let client = thread::spawn(move || {
        let mut client = UnixStream::connect(path).unwrap();
        client_handshake(&mut client);
        let Frame::Query { correlation, .. } = receive(&mut client) else {
            panic!("expected Query")
        };
        sent.send(()).unwrap();
        released.recv_timeout(Duration::from_secs(1)).unwrap();
        client
            .write_all(
                &Frame::Reply {
                    correlation,
                    observed_us: monotonic_us().unwrap(),
                    outcome: Outcome::Missing,
                }
                .encode(),
            )
            .unwrap();
        assert_closed(&mut client);
    });
    wait_until(|| broker.shared.channels.lock().unwrap().len() == 1);
    let query_broker = broker.clone();
    let query =
        thread::spawn(move || query_broker.query(std::process::id(), 77, monotonic_us().unwrap()));
    received.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(broker.shared.channels.try_lock().is_ok());
    assert_eq!(
        broker.query(std::process::id(), 88, monotonic_us().unwrap()),
        Err(BrokerError::Busy)
    );
    let mut reconnect = UnixStream::connect(runtime.path().join("lyn-gnome-v1.sock")).unwrap();
    client_handshake(&mut reconnect);
    assert_closed(&mut reconnect);
    release.send(()).unwrap();
    assert_eq!(
        query.join().unwrap(),
        Err(BrokerError::Channel(ChannelError::Reply(
            ReplyReject::Unresolved(Outcome::Missing)
        )))
    );
    drop(broker);
    client.join().unwrap();
}

#[test]
fn protocol_failure_retires_channel_instead_of_routing_another_request() {
    let runtime = runtime();
    let broker = NativeBroker::start_fixture(runtime.path()).unwrap();
    let path = runtime.path().join("lyn-gnome-v1.sock");
    let client = thread::spawn(move || {
        let mut client = UnixStream::connect(path).unwrap();
        client_handshake(&mut client);
        assert!(matches!(receive(&mut client), Frame::Query { .. }));
        client.write_all(&Frame::Hello.encode()).unwrap();
        assert_closed(&mut client);
    });
    wait_until(|| broker.shared.channels.lock().unwrap().len() == 1);
    assert!(matches!(
        broker.query(std::process::id(), 77, monotonic_us().unwrap()),
        Err(BrokerError::Channel(_))
    ));
    assert!(broker.shared.channels.lock().unwrap().is_empty());
    assert_eq!(
        broker.query(std::process::id(), 77, monotonic_us().unwrap()),
        Err(BrokerError::Unavailable)
    );
    client.join().unwrap();
}

#[test]
fn shutdown_during_partial_handshake_waits_only_for_absolute_handshake_budget() {
    let runtime = runtime();
    let broker = NativeBroker::start_fixture(runtime.path()).unwrap();
    let mut client = UnixStream::connect(runtime.path().join("lyn-gnome-v1.sock")).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client.write_all(&Frame::Hello.encode()).unwrap();
    assert!(matches!(receive(&mut client), Frame::Bind { .. }));
    // Consume most of the original budget. Stop must not grant a fresh 500ms
    // handshake timeout; allow 150ms scheduling slack beyond what remains.
    thread::sleep(Duration::from_millis(350));
    let started = Instant::now();
    drop(broker);
    assert!(started.elapsed() < Duration::from_millis(300));
    assert_closed(&mut client);
    assert!(!runtime.path().join("lyn-gnome-v1.sock").exists());
}

// Dedicated subprocess entrypoint: the parent launches this exact test so each
// connection has a distinct real kernel PID, not a test-supplied peer identity.
#[test]
fn subprocess_fixture_peer_entry() {
    let Some(path) = std::env::var_os("LYN_NATIVE_TEST_SOCKET") else {
        return;
    };
    let mut stream = UnixStream::connect(path).unwrap();
    client_handshake(&mut stream);
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    loop {
        let mut bytes = [0; FRAME_BYTES];
        match stream.read_exact(&mut bytes) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return,
            Err(error) => panic!("fixture read failed: {error}"),
        }
        let Frame::Query {
            correlation,
            invoked_us,
        } = Frame::decode(&bytes).unwrap()
        else {
            panic!("expected Query")
        };
        stream
            .write_all(
                &Frame::Reply {
                    correlation,
                    observed_us: monotonic_us().unwrap(),
                    outcome: Outcome::Candidate(super::super::gnome_protocol::Candidate {
                        source_id: u64::from(std::process::id()),
                        pty_number: 0,
                        foreground_pgid: std::process::id(),
                        registered_us: invoked_us - 2,
                        changed_us: invoked_us - 1,
                        revision: 1,
                    }),
                }
                .encode(),
            )
            .unwrap();
    }
}

struct FixtureChildren(Vec<std::process::Child>);

#[test]
fn reconnect_during_post_query_owner_check_cannot_return_retired_evidence() {
    let runtime = runtime();
    let broker = NativeBroker::start_fixture(runtime.path()).unwrap();
    let path = runtime.path().join("lyn-gnome-v1.sock");
    let first_path = path.clone();
    let first = thread::spawn(move || {
        let mut client = UnixStream::connect(first_path).unwrap();
        client_handshake(&mut client);
        answer(&mut client);
        assert_closed(&mut client);
    });
    wait_until(|| broker.shared.channels.lock().unwrap().len() == 1);
    let mut checks = 0;
    let mut replacement = None;
    let result = broker.query_window_with(77, monotonic_us().unwrap(), |_| {
        checks += 1;
        if checks == 2 {
            let mut client = UnixStream::connect(&path).unwrap();
            let generation = client_handshake(&mut client);
            wait_until(|| {
                broker
                    .shared
                    .channels
                    .lock()
                    .unwrap()
                    .get(&std::process::id())
                    .is_some_and(|slot| {
                        slot.try_lock()
                            .is_ok_and(|channel| channel.generation() == generation)
                    })
            });
            replacement = Some(client);
        }
        Some(std::process::id())
    });
    assert_eq!(result, Err(BrokerError::Unavailable));
    drop(replacement);
    drop(broker);
    first.join().unwrap();
}

impl FixtureChildren {
    fn spawn(&mut self, path: &std::path::Path) -> u32 {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "context::gnome_broker::tests::subprocess_fixture_peer_entry",
                "--test-threads=1",
            ])
            .env("LYN_NATIVE_TEST_SOCKET", path)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        self.0.push(child);
        pid
    }
}

impl Drop for FixtureChildren {
    fn drop(&mut self) {
        for child in &mut self.0 {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}

#[test]
fn real_distinct_peers_hit_capacity_and_a_dead_peer_releases_a_slot() {
    let runtime = runtime();
    let broker = NativeBroker::start_fixture(runtime.path()).unwrap();
    let path = runtime.path().join("lyn-gnome-v1.sock");
    let mut children = FixtureChildren(Vec::new());
    for _ in 0..MAX_SERVERS {
        let pid = children.spawn(&path);
        wait_until(|| broker.shared.channels.lock().unwrap().contains_key(&pid));
    }
    let rejected = children.spawn(&path);
    wait_until(|| children.0[MAX_SERVERS].try_wait().unwrap().is_some());
    assert!(children.0[MAX_SERVERS].wait().unwrap().success());
    assert_eq!(broker.shared.channels.lock().unwrap().len(), MAX_SERVERS);
    assert_eq!(
        broker.query(rejected, 77, monotonic_us().unwrap()),
        Err(BrokerError::Unavailable)
    );
    let first = children.0[0].id();
    let result = broker.query(first, 77, monotonic_us().unwrap()).unwrap();
    assert_eq!(result.candidate.source_id, u64::from(first));
    children.0[0].kill().unwrap();
    children.0[0].wait().unwrap();
    let replacement = children.spawn(&path);
    wait_until(|| {
        broker
            .shared
            .channels
            .lock()
            .unwrap()
            .contains_key(&replacement)
    });
    let channels = broker.shared.channels.lock().unwrap();
    assert_eq!(channels.len(), MAX_SERVERS);
    assert!(!channels.contains_key(&first));
    drop(channels);
    assert_eq!(
        broker
            .query(replacement, 88, monotonic_us().unwrap())
            .unwrap()
            .candidate
            .source_id,
        u64::from(replacement)
    );
    drop(broker);
    for child in &mut children.0[1..] {
        assert!(child.wait().unwrap().success());
    }
}
