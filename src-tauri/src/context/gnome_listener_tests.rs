use super::*;
use std::{
    os::unix::fs::symlink,
    thread,
    time::{Duration, Instant},
};

fn private_runtime() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("lyn-native-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

#[test]
fn binds_a_private_socket_and_removes_only_its_owned_endpoint() {
    let directory = private_runtime();
    let fixture = fs::symlink_metadata(directory.path()).unwrap();
    assert_eq!(
        (fixture.uid(), fixture.mode() & 0o777),
        (unsafe { libc::geteuid() }, 0o700),
        "fixture must model a private XDG runtime"
    );
    let path = directory.path().join(SOCKET_NAME);
    let (listener, _stop) = NativeListener::bind(directory.path()).unwrap();
    assert_eq!(listener.socket_path(), path);
    let metadata = fs::symlink_metadata(&path).unwrap();
    assert!(metadata.file_type().is_socket());
    assert_eq!(metadata.mode() & 0o777, 0o600);
    assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
    drop(listener);
    assert!(!path.exists());
}

#[test]
fn refuses_public_or_symlinked_runtime_directories() {
    let directory = private_runtime();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(NativeListener::bind(directory.path()).is_err());
    assert!(!directory.path().join(SOCKET_NAME).exists());
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let parent = tempfile::tempdir().unwrap();
    let link = parent.path().join("runtime-link");
    symlink(directory.path(), &link).unwrap();
    for spelling in [link.clone(), link.join(""), link.join(".")] {
        assert!(
            NativeListener::bind(&spelling).is_err(),
            "symlink runtime accepted: {spelling:?}"
        );
    }
}

#[test]
fn never_unlinks_an_occupied_file_or_existing_listener() {
    let directory = private_runtime();
    let path = directory.path().join(SOCKET_NAME);
    fs::write(&path, b"fixture owned by another component").unwrap();
    assert!(NativeListener::bind(directory.path()).is_err());
    assert_eq!(
        fs::read(&path).unwrap(),
        b"fixture owned by another component"
    );
    fs::remove_file(&path).unwrap();
    let original = UnixListener::bind(&path).unwrap();
    assert!(NativeListener::bind(directory.path()).is_err());
    let client = UnixStream::connect(&path).unwrap();
    let (accepted, _) = original.accept().unwrap();
    drop((client, accepted, original));
}

#[test]
fn preserves_a_replacement_path_during_shutdown() {
    let directory = private_runtime();
    let path = directory.path().join(SOCKET_NAME);
    let (listener, _stop) = NativeListener::bind(directory.path()).unwrap();
    fs::remove_file(&path).unwrap();
    fs::write(&path, b"replacement fixture").unwrap();
    drop(listener);
    assert_eq!(fs::read(&path).unwrap(), b"replacement fixture");
}

#[test]
fn stop_wakes_an_idle_listener_without_connecting_a_fake_client() {
    let directory = private_runtime();
    let (mut listener, stop) = NativeListener::bind(directory.path()).unwrap();
    let worker = thread::spawn(move || listener.accept().unwrap().is_none());
    let started = Instant::now();
    stop.stop().unwrap();
    assert!(worker.join().unwrap());
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(!directory.path().join(SOCKET_NAME).exists());
}

#[test]
fn a_real_connection_is_accepted_and_stop_wins_over_ready_clients() {
    let directory = private_runtime();
    let path = directory.path().join(SOCKET_NAME);
    let (mut listener, stop) = NativeListener::bind(directory.path()).unwrap();
    let client = UnixStream::connect(&path).unwrap();
    let accepted = listener.accept().unwrap().unwrap();
    assert_ne!(accepted.as_raw_fd(), client.as_raw_fd());
    assert_eq!(
        unsafe { libc::fcntl(accepted.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
        libc::FD_CLOEXEC
    );
    let _queued = UnixStream::connect(&path).unwrap();
    stop.stop().unwrap();
    assert!(listener.accept().unwrap().is_none());
}

#[test]
fn dropping_the_stop_handle_also_revokes_future_accepts() {
    let directory = private_runtime();
    let (mut listener, stop) = NativeListener::bind(directory.path()).unwrap();
    drop(stop);
    assert!(listener.accept().unwrap().is_none());
}

#[test]
fn binding_does_not_admit_clients_before_explicit_listen() {
    let directory = private_runtime();
    let path = directory.path().join(SOCKET_NAME);
    let listener = bind_unlistening(&path).unwrap();
    assert_eq!(
        UnixStream::connect(&path).unwrap_err().kind(),
        io::ErrorKind::ConnectionRefused
    );
    assert_eq!(
        unsafe { libc::fcntl(listener.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
        libc::FD_CLOEXEC
    );
}

#[test]
fn a_saturated_listener_has_a_bounded_pending_queue() {
    use std::os::{
        fd::{FromRawFd, OwnedFd},
        unix::ffi::OsStrExt,
    };
    let directory = private_runtime();
    let (listener, _stop) = NativeListener::bind(directory.path()).unwrap();
    let path = listener.socket_path().as_os_str().as_bytes();
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as _;
    assert!(path.len() < address.sun_path.len());
    for (destination, byte) in address.sun_path.iter_mut().zip(path) {
        *destination = *byte as _;
    }
    let mut connections = Vec::new();
    let mut admitted = 0;
    for _ in 0..20 {
        let fd = unsafe {
            libc::socket(
                libc::AF_UNIX,
                libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                0,
            )
        };
        assert!(fd >= 0);
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        let result = unsafe {
            libc::connect(
                fd,
                (&address as *const libc::sockaddr_un).cast(),
                std::mem::size_of_val(&address) as _,
            )
        };
        if result == 0 {
            admitted += 1;
        } else {
            assert_eq!(io::Error::last_os_error().kind(), io::ErrorKind::WouldBlock);
        }
        connections.push(owned);
    }
    // Linux permits backlog+1 completed connections; no accept has occurred.
    assert!(admitted <= 17, "unexpected pending queue size {admitted}");
}
