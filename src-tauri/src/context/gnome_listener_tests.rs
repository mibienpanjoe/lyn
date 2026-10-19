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
fn abrupt_listener_exit_fixture() {
    let Some(runtime) = std::env::var_os("LYN_GNOME_LISTENER_CRASH_RUNTIME") else {
        return;
    };
    let runtime = PathBuf::from(runtime);
    assert!(
        runtime
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("lyn-native-")
    );
    // A broken parent must not leave this private test process behind.
    unsafe { libc::alarm(5) };
    let (_listener, _stop) = NativeListener::bind(&runtime).unwrap();
    use std::os::unix::fs::OpenOptionsExt;
    let _ready = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(runtime.join("ready"))
        .unwrap();
    loop {
        thread::park();
    }
}

#[test]
fn killed_owner_leaves_a_stale_socket_that_cannot_be_unlinked_on_bind() {
    use std::{
        os::unix::process::ExitStatusExt,
        process::{Command, Stdio},
    };
    let directory = private_runtime();
    let path = directory.path().join(SOCKET_NAME);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "context::gnome_listener::tests::abrupt_listener_exit_fixture",
            "--test-threads=1",
        ])
        .env("LYN_GNOME_LISTENER_CRASH_RUNTIME", directory.path())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, child.id(), 0) } as i32;
    assert!(
        raw >= 0,
        "pidfd support is required, never use a numeric-PID fallback"
    );
    let pin = unsafe { OwnedFd::from_raw_fd(raw) };
    let deadline = Instant::now() + Duration::from_secs(2);
    while !directory.path().join("ready").exists() {
        assert!(
            child.try_wait().unwrap().is_none(),
            "private listener fixture exited before readiness"
        );
        if Instant::now() >= deadline {
            unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    pin.as_raw_fd(),
                    libc::SIGKILL,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                )
            };
            let _ = child.wait();
            panic!("private listener fixture did not become ready");
        }
        thread::sleep(Duration::from_millis(5));
    }
    let before = fs::symlink_metadata(&path).unwrap();
    assert_eq!(
        unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                pin.as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        },
        0
    );
    let status = child.wait().unwrap();
    assert_eq!(status.signal(), Some(libc::SIGKILL));
    assert_eq!(
        UnixStream::connect(&path).unwrap_err().kind(),
        io::ErrorKind::ConnectionRefused
    );
    assert!(
        matches!(NativeListener::bind(directory.path()), Err(error) if error.kind() == io::ErrorKind::AddrInUse)
    );
    let after = fs::symlink_metadata(&path).unwrap();
    assert!(after.file_type().is_socket());
    assert_eq!(
        (after.dev(), after.ino(), after.mode(), after.uid()),
        (before.dev(), before.ino(), before.mode(), before.uid())
    );
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
