use super::*;
use x11rb::protocol::res::{ClientIdMask, ClientIdSpec, ClientIdValue};

#[test]
fn negotiated_version_must_match_the_requested_protocol() {
    assert!(version_supported(1, 2));
    for (major, minor) in [(0, 9), (1, 0), (1, 1), (1, 3), (2, 0), (2, 2)] {
        assert!(!version_supported(major, minor));
    }
}

fn id(window: u32, pid: u32) -> ClientIdValue {
    ClientIdValue {
        spec: ClientIdSpec {
            client: window,
            mask: ClientIdMask::LOCAL_CLIENT_PID,
        },
        value: vec![pid],
    }
}

#[test]
fn only_one_exact_positive_local_owner_is_accepted() {
    assert_eq!(parse_owner(77, &[id(77, 123)]), Some(123));
    for ids in [
        vec![],
        vec![id(88, 123)],
        vec![id(77, 0)],
        vec![id(77, u32::MAX)],
        vec![id(77, 123), id(77, 123)],
    ] {
        assert_eq!(parse_owner(77, &ids), None);
    }
    assert_eq!(parse_owner(0, &[id(0, 123)]), None);
}

#[test]
fn maps_window_to_exactly_one_server_advertised_client_range() {
    use x11rb::protocol::res::Client;
    let client = Client {
        resource_base: 0x200000,
        resource_mask: 0x1fffff,
    };
    assert_eq!(client_base(0x20000a, &[client]), Some(0x200000));
    assert_eq!(client_base(0x40000a, &[client]), None);
    assert_eq!(client_base(0x20000a, &[client, client]), None);
    assert_eq!(
        client_base(
            0x20000a,
            &[Client {
                resource_base: 0,
                resource_mask: u32::MAX
            }]
        ),
        None
    );
    assert_eq!(
        client_base(
            0x20000a,
            &[Client {
                resource_base: 0x20000a,
                resource_mask: 0x1fffff
            }]
        ),
        None
    );
}

#[test]
fn client_xid_unknown_mask_and_noncanonical_pid_lengths_are_not_owners() {
    for mask in [
        ClientIdMask::CLIENT_XID,
        ClientIdMask::from(3u32),
        ClientIdMask::from(4u32),
    ] {
        let mut value = id(77, 123);
        value.spec.mask = mask;
        assert_eq!(parse_owner(77, &[value]), None);
    }
    for bytes in [vec![], vec![123, 456]] {
        let mut value = id(77, 123);
        value.value = bytes;
        assert_eq!(parse_owner(77, &[value]), None);
    }
}

#[test]
fn a_silent_x_server_cannot_stall_the_setup_handshake() {
    use std::{
        os::unix::net::UnixStream,
        time::{Duration, Instant},
    };
    use x11rb::rust_connection::{DefaultStream, RustConnection};
    let (client, _silent_server) = UnixStream::pair().unwrap();
    let (stream, _) = DefaultStream::from_unix_stream(client).unwrap();
    let started = Instant::now();
    let stream = DeadlineStream {
        inner: stream,
        deadline: started + Duration::from_millis(30),
    };
    assert!(RustConnection::connect_to_stream(stream, 0).is_err());
    assert!(started.elapsed() < Duration::from_millis(200));
}

#[test]
fn xserver_socket_must_belong_to_root_or_the_current_user_before_auth() {
    let credentials = |uid, pid| libc::ucred { uid, pid, gid: uid };
    assert!(accepts_server(credentials(0, 123), 1000));
    assert!(accepts_server(credentials(1000, 123), 1000));
    assert!(!accepts_server(credentials(1001, 123), 1000));
    assert!(!accepts_server(credentials(1000, 0), 1000));
    let (client, _server) = std::os::unix::net::UnixStream::pair().unwrap();
    assert!(trusted_server(&client));
}
