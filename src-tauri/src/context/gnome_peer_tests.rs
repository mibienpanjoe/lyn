use super::*;
use std::{os::unix::net::UnixStream, path::Path};

#[path = "gnome_pid_reuse_tests.rs"]
mod pid_reuse;

#[test]
fn same_uid_is_not_sufficient_for_native_admission() {
    let (server, _client) = UnixStream::pair().unwrap();
    assert!(NativePeer::authenticate(&server).is_err());
}

#[test]
fn pins_kernel_pid_uid_executable_and_process_lifetime() {
    let (server, _client) = UnixStream::pair().unwrap();
    let executable = std::env::current_exe().unwrap();
    let peer =
        NativePeer::authenticate_executable(&server, unsafe { libc::geteuid() }, &executable)
            .unwrap();
    assert_eq!(peer.pid(), std::process::id());
    assert!(peer.revalidate().is_ok());
}

#[test]
fn refuses_wrong_uid_and_wrong_executable_inode() {
    let (server, _client) = UnixStream::pair().unwrap();
    let uid = unsafe { libc::geteuid() };
    let executable = std::env::current_exe().unwrap();
    assert!(
        NativePeer::authenticate_executable(&server, uid.wrapping_add(1), &executable).is_err()
    );
    assert!(NativePeer::authenticate_executable(&server, uid, Path::new("/usr/bin/true")).is_err());
}

#[test]
fn stat_parser_ignores_comm_even_with_spaces_parentheses_and_newlines() {
    let mut fields = vec!["0"; 20];
    fields[0] = "S";
    fields[19] = "12345";
    let text = format!("42 (untrusted ) name\nwith (parens)) {}", fields.join(" "));
    assert_eq!(start_time(&text), Some(12345));
    assert_eq!(start_time("42 (truncated) S"), None);
    fields[0] = "Z";
    assert_eq!(start_time(&format!("42 (x) {}", fields.join(" "))), None);
}

#[test]
fn a_real_connecting_process_is_pinned_and_its_exit_revokes_admission() {
    use std::{
        io::Write,
        os::fd::AsRawFd,
        os::unix::net::UnixListener,
        process::{Command, Stdio},
    };
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("native-peer.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let mut child = Child(Command::new("/usr/bin/python3").args(["-I", "-c",
        "import socket,sys; s=socket.socket(socket.AF_UNIX); s.connect(sys.argv[1]); sys.stdin.buffer.read(1)"])
        .arg(&path).stdin(Stdio::piped()).stdout(Stdio::null()).spawn().unwrap());
    let mut incoming = libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    assert_eq!(
        unsafe { libc::poll(&mut incoming, 1, 2000) },
        1,
        "fixture did not connect"
    );
    let (socket, _) = listener.accept().unwrap();
    let peer = NativePeer::authenticate_executable(
        &socket,
        unsafe { libc::geteuid() },
        Path::new("/usr/bin/python3"),
    )
    .unwrap();
    assert_eq!(peer.pid(), child.0.id());
    assert!(peer.revalidate().is_ok());
    child.0.stdin.take().unwrap().write_all(b"x").unwrap();
    assert!(child.0.wait().unwrap().success());
    assert!(peer.revalidate().is_err());
}
