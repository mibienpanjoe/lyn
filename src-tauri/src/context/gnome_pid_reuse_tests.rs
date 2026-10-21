//! Opt-in kernel proof, never a production authentication bypass.
use super::super::*;

fn private_mapping(mapping: &str, outside: u32) -> bool {
    outside > 0
        && mapping.split_ascii_whitespace().collect::<Vec<_>>() == ["0", &outside.to_string(), "1"]
}

fn private_proc_view(status: &str) -> bool {
    let values: Vec<_> = status
        .lines()
        .filter_map(|line| line.strip_prefix("NSpid:"))
        .collect();
    values.len() == 1 && values[0].split_ascii_whitespace().collect::<Vec<_>>() == ["1"]
}

fn require_private_namespace() {
    assert_eq!(std::process::id(), 1, "never change a host PID counter");
    assert_eq!(unsafe { libc::getppid() }, 0);
    assert_eq!(unsafe { libc::geteuid() }, 0);
    assert_eq!(fs::read_link("/proc/self").unwrap(), Path::new("1"));
    let mut status = String::new();
    File::open("/proc/self/status")
        .unwrap()
        .take(65537)
        .read_to_string(&mut status)
        .unwrap();
    assert!(status.len() <= 65536 && private_proc_view(&status));
    for kind in ["pid", "mnt", "user"] {
        let expected =
            std::env::var_os(format!("LYN_PID_REUSE_PARENT_{}", kind.to_uppercase())).unwrap();
        assert_ne!(
            fs::read_link(format!("/proc/self/ns/{kind}")).unwrap(),
            Path::new(&expected)
        );
    }
    for kind in ["uid", "gid"] {
        let outside = std::env::var(format!("LYN_PID_REUSE_OUTSIDE_{}", kind.to_uppercase()))
            .unwrap()
            .parse()
            .unwrap();
        assert!(private_mapping(
            &fs::read_to_string(format!("/proc/self/{kind}_map")).unwrap(),
            outside
        ));
    }
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
    assert_eq!(
        unsafe { libc::statfs(c"/proc".as_ptr(), stat.as_mut_ptr()) },
        0
    );
    assert_eq!(unsafe { stat.assume_init() }.f_type, libc::PROC_SUPER_MAGIC);
}

extern "C" fn deadline(_: libc::c_int) {
    // Namespace PID1 exiting also kills its owned descendants.
    unsafe { libc::_exit(124) }
}

fn arm_deadline() {
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = deadline as *const () as usize;
        assert_eq!(libc::sigemptyset(&mut action.sa_mask), 0);
        assert_eq!(
            libc::sigaction(libc::SIGALRM, &action, std::ptr::null_mut()),
            0
        );
        let mut signals: libc::sigset_t = std::mem::zeroed();
        assert_eq!(libc::sigemptyset(&mut signals), 0);
        assert_eq!(libc::sigaddset(&mut signals, libc::SIGALRM), 0);
        assert_eq!(
            libc::pthread_sigmask(libc::SIG_UNBLOCK, &signals, std::ptr::null_mut()),
            0
        );
        libc::alarm(5);
    }
}

#[test]
fn namespace_write_guard_requires_private_identity_mapping() {
    assert!(private_mapping("         0       1000          1\n", 1000));
    for (mapping, uid) in [
        ("0 0 1", 0),
        ("0 1000 4294967295", 1000),
        ("0 1001 1", 1000),
        ("0 1000 1\n1 1001 1", 1000),
        ("", 1000),
    ] {
        assert!(!private_mapping(mapping, uid));
    }
}

#[test]
fn namespace_write_guard_rejects_ancestor_procfs_even_with_equal_numbers() {
    assert!(private_proc_view("Name:\tfixture\nNSpid:\t1\n"));
    for status in [
        "NSpid:\t1 1\n",
        "NSpid:\t42 1\n",
        "NSpid:\t2\n",
        "NSpid:\t1\nNSpid:\t1\n",
        "NStgid:\t1\n",
        "",
    ] {
        assert!(!private_proc_view(status));
    }
}

#[test]
fn namespace_write_guard_refuses_the_normal_test_process() {
    assert!(std::panic::catch_unwind(require_private_namespace).is_err());
}

#[test]
fn isolated_pid_reuse_entry() {
    use std::{
        io::Write,
        os::unix::net::UnixListener,
        process::{Command, Stdio},
    };
    let Ok(nonce) = std::env::var("LYN_PID_REUSE_NONCE") else {
        return;
    };
    let nonce = uuid::Uuid::parse_str(&nonce).unwrap();
    assert!(!nonce.is_nil());
    arm_deadline();
    require_private_namespace();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("peer.sock");
    let listener = UnixListener::bind(&path).unwrap();
    // All libtest/listener setup precedes these two fixed counter writes.
    // Only this namespace's two intended Python spawns may allocate here.
    let connect = || {
        require_private_namespace();
        fs::write("/proc/sys/kernel/ns_last_pid", b"63").unwrap();
        let child = Command::new("/usr/bin/python3").args(["-I", "-c",
            "import socket,sys; s=socket.socket(socket.AF_UNIX); s.connect(sys.argv[1]); sys.stdin.buffer.read(1)"])
            .arg(&path).stdin(Stdio::piped()).stdout(Stdio::null()).spawn().unwrap();
        assert_eq!(
            child.id(),
            64,
            "actual PID reuse required; never retry allocation"
        );
        let (stream, _) = listener.accept().unwrap();
        let peer =
            NativePeer::authenticate_executable(&stream, 0, Path::new("/usr/bin/python3")).unwrap();
        assert_eq!(peer.pid(), child.id());
        (child, stream, peer)
    };
    let (mut old_child, _old_stream, mut old_peer) = connect();
    old_peer.revalidate().unwrap();
    old_child.stdin.take().unwrap().write_all(b"x").unwrap();
    assert!(old_child.wait().unwrap().success());
    let mut event = libc::pollfd {
        fd: old_peer.lifetime.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    assert_eq!(unsafe { libc::poll(&mut event, 1, 0) }, 1);
    assert_ne!(
        event.revents & libc::POLLIN,
        0,
        "process exit requires POLLIN"
    );
    assert_eq!(event.revents & (libc::POLLERR | libc::POLLNVAL), 0);
    let (mut replacement, _new_stream, current) = connect();
    assert_eq!(
        current.pid(),
        old_peer.pid(),
        "numeric PID must really match"
    );
    current.revalidate().unwrap();
    assert!(old_peer.revalidate().is_err());
    // Even identical refreshed numeric metadata cannot revive the dead pin.
    // This mutation is private test state, never an authentication entrypoint.
    old_peer.identity = current.identity;
    assert!(old_peer.revalidate().is_err());
    current.revalidate().unwrap();
    replacement.stdin.take().unwrap().write_all(b"x").unwrap();
    assert!(replacement.wait().unwrap().success());
    println!("\nLYN_PID_REUSE_PROVED:{nonce}");
}
