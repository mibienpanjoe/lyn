use super::*;
use crate::context::gnome_listener::NativeListener;
use std::os::unix::{
    fs::{PermissionsExt, symlink},
    net::UnixListener,
};

fn runtime() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("lyn-pub-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

#[test]
fn publishes_only_after_listen_and_cleans_owned_metadata_but_not_lock() {
    let root = runtime();
    let mut publication = NativePublication::prepare(root.path()).unwrap();
    let instance = publication.instance_path().to_owned();
    assert!(!root.path().join(CURRENT).exists());
    assert_eq!(fs::metadata(&instance).unwrap().mode() & 0o777, 0o700);
    let (listener, _stop) = NativeListener::bind(&instance).unwrap();
    publication.publish(&listener).unwrap();
    let record = fs::read(root.path().join(CURRENT)).unwrap();
    assert_eq!(record.len(), 36);
    assert_eq!(
        record,
        format!("{}\n", instance.file_name().unwrap().to_str().unwrap()).as_bytes()
    );
    assert_eq!(
        fs::metadata(root.path().join(CURRENT)).unwrap().mode() & 0o777,
        0o600
    );
    drop(listener);
    drop(publication);
    assert!(!root.path().join(CURRENT).exists());
    assert!(!instance.exists());
    assert!(root.path().join(LOCK).exists());
    assert!(NativePublication::prepare(root.path()).is_ok());
}

#[test]
fn concurrent_publisher_refuses_without_mutating_the_live_publication() {
    let root = runtime();
    let mut publication = NativePublication::prepare(root.path()).unwrap();
    let (listener, _stop) = NativeListener::bind(publication.instance_path()).unwrap();
    publication.publish(&listener).unwrap();
    let before = fs::read(root.path().join(CURRENT)).unwrap();
    assert!(NativePublication::prepare(root.path()).is_err());
    assert_eq!(fs::read(root.path().join(CURRENT)).unwrap(), before);
    drop(listener);
}

#[test]
fn clean_release_unlocks_even_with_an_inherited_open_file_description() {
    let root = runtime();
    let publication = NativePublication::prepare(root.path()).unwrap();
    // dup models the shared open file description briefly inherited between
    // fork and exec; FD_CLOEXEC does not close it before exec has happened.
    let inherited = publication.lock.0.try_clone().unwrap();
    drop(publication);
    let next = NativePublication::prepare(root.path()).unwrap();
    assert!(inherited.metadata().unwrap().is_file());
    drop(next);
}

#[test]
fn replaces_valid_stale_publication_without_touching_its_socket() {
    let root = runtime();
    let old = root.path().join(format!("gn-{}", "a".repeat(32)));
    fs::create_dir(&old).unwrap();
    fs::set_permissions(&old, fs::Permissions::from_mode(0o700)).unwrap();
    let socket = old.join("lyn-gnome-v1.sock");
    drop(UnixListener::bind(&socket).unwrap());
    let before = fs::symlink_metadata(&socket).unwrap();
    let record = root.path().join(CURRENT);
    fs::write(
        &record,
        format!("{}\n", old.file_name().unwrap().to_str().unwrap()),
    )
    .unwrap();
    fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).unwrap();
    let mut publication = NativePublication::prepare(root.path()).unwrap();
    assert_ne!(publication.instance_path(), old);
    let (listener, _stop) = NativeListener::bind(publication.instance_path()).unwrap();
    publication.publish(&listener).unwrap();
    let after = fs::symlink_metadata(&socket).unwrap();
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
    drop(listener);
}

#[test]
fn refuses_unsafe_lock_and_publication_nodes_without_modifying_them() {
    for name in [LOCK, CURRENT] {
        for kind in ["symlink", "public", "hardlink", "directory", "fifo"] {
            let root = runtime();
            let target = root.path().join(name);
            let other = root.path().join("other");
            fs::write(&other, b"untouched").unwrap();
            match kind {
                "symlink" => symlink(&other, &target).unwrap(),
                "public" => {
                    fs::write(&target, b"untouched").unwrap();
                    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
                }
                "hardlink" => fs::hard_link(&other, &target).unwrap(),
                "directory" => fs::create_dir(&target).unwrap(),
                "fifo" => {
                    let target = std::ffi::CString::new(target.as_os_str().as_bytes()).unwrap();
                    assert_eq!(unsafe { libc::mkfifo(target.as_ptr(), 0o600) }, 0);
                }
                _ => unreachable!(),
            }
            assert!(
                NativePublication::prepare(root.path()).is_err(),
                "{name}/{kind}"
            );
            assert!(fs::symlink_metadata(&target).is_ok());
            assert_eq!(fs::read(&other).unwrap(), b"untouched");
        }
    }
}

#[test]
fn refuses_malformed_records_public_runtime_and_overlong_addresses() {
    for body in [
        "../escape\n".to_owned(),
        "gn-ABC\n".to_owned(),
        format!("gn-{}\nextra", "a".repeat(32)),
    ] {
        let root = runtime();
        let record = root.path().join(CURRENT);
        fs::write(&record, &body).unwrap();
        fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(NativePublication::prepare(root.path()).is_err());
        assert_eq!(fs::read_to_string(record).unwrap(), body);
    }
    let root = runtime();
    let link = root.path().join("alias");
    symlink(root.path(), &link).unwrap();
    for path in [link.clone(), link.join(".")] {
        assert!(NativePublication::prepare(&path).is_err());
    }
    let long = root.path().join("x".repeat(65));
    fs::create_dir(&long).unwrap();
    fs::set_permissions(&long, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(NativePublication::prepare(&long).is_err());
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(NativePublication::prepare(root.path()).is_err());
}

#[test]
fn refuses_residue_and_scan_limits_without_deleting_anything() {
    for (count, prefix) in [(64, "gn-"), (4096, "other-")] {
        let root = runtime();
        for index in 0..count {
            fs::create_dir(root.path().join(format!("{prefix}{index:032x}"))).unwrap();
        }
        assert!(NativePublication::prepare(root.path()).is_err());
        assert_eq!(
            fs::read_dir(root.path())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name() != LOCK)
                .count(),
            count
        );
    }
}

#[test]
fn publication_and_cleanup_preserve_replaced_lock_record_and_directory() {
    let root = runtime();
    let mut publication = NativePublication::prepare(root.path()).unwrap();
    let instance = publication.instance_path().to_owned();
    let (listener, _stop) = NativeListener::bind(&instance).unwrap();
    fs::rename(root.path().join(LOCK), root.path().join("retired-lock")).unwrap();
    fs::write(root.path().join(LOCK), b"replacement").unwrap();
    fs::set_permissions(root.path().join(LOCK), fs::Permissions::from_mode(0o600)).unwrap();
    assert!(publication.publish(&listener).is_err());
    drop(listener);
    drop(publication);
    assert_eq!(fs::read(root.path().join(LOCK)).unwrap(), b"replacement");

    let mut publication = NativePublication::prepare(root.path()).unwrap();
    let instance = publication.instance_path().to_owned();
    let (listener, _stop) = NativeListener::bind(&instance).unwrap();
    publication.publish(&listener).unwrap();
    fs::rename(
        root.path().join(CURRENT),
        root.path().join("retired-record"),
    )
    .unwrap();
    fs::write(root.path().join(CURRENT), b"replacement").unwrap();
    drop(listener);
    fs::rename(&instance, root.path().join("retired-instance")).unwrap();
    fs::create_dir(&instance).unwrap();
    drop(publication);
    assert_eq!(fs::read(root.path().join(CURRENT)).unwrap(), b"replacement");
    assert!(instance.is_dir());
}

#[test]
fn refuses_an_unrelated_listener_a_replaced_socket_and_double_publication() {
    let root = runtime();
    let mut publication = NativePublication::prepare(root.path()).unwrap();
    let other = runtime();
    let (unrelated, _stop) = NativeListener::bind(other.path()).unwrap();
    assert!(publication.publish(&unrelated).is_err());
    let (listener, _stop) = NativeListener::bind(publication.instance_path()).unwrap();
    let path = listener.socket_path();
    fs::rename(path, path.with_extension("retired")).unwrap();
    let _replacement = UnixListener::bind(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(publication.publish(&listener).is_err());
    assert!(!root.path().join(CURRENT).exists());

    let root = runtime();
    let mut publication = NativePublication::prepare(root.path()).unwrap();
    let (listener, _stop) = NativeListener::bind(publication.instance_path()).unwrap();
    publication.publish(&listener).unwrap();
    let before = fs::symlink_metadata(root.path().join(CURRENT)).unwrap();
    assert!(publication.publish(&listener).is_err());
    let after = fs::symlink_metadata(root.path().join(CURRENT)).unwrap();
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
    drop(listener);
}

#[test]
fn accepts_the_last_instance_slot_and_unpublished_drop_cleans_its_empty_directory() {
    let root = runtime();
    for index in 0..63 {
        fs::create_dir(root.path().join(format!("gn-{index:032x}"))).unwrap();
    }
    let publication = NativePublication::prepare(root.path()).unwrap();
    let path = publication.instance_path().to_owned();
    assert!(path.is_dir());
    drop(publication);
    assert!(!path.exists());
    assert!(!root.path().join(CURRENT).exists());
}

#[test]
fn retains_the_unlinked_publication_inode_and_releases_all_owned_fds() {
    let root = runtime();
    for _ in 0..100 {
        let mut publication = NativePublication::prepare(root.path()).unwrap();
        let (listener, _stop) = NativeListener::bind(publication.instance_path()).unwrap();
        publication.publish(&listener).unwrap();
        let record = root.path().join(CURRENT);
        let before = fs::symlink_metadata(&record).unwrap();
        fs::remove_file(&record).unwrap();
        let pinned = publication.published.as_ref().unwrap().metadata().unwrap();
        assert_eq!(
            (pinned.dev(), pinned.ino(), pinned.nlink()),
            (before.dev(), before.ino(), 0)
        );
        drop(listener);
        drop(publication);
        // Count only descriptors targeting this unique fixture, not a global
        // FD total that concurrent Rust tests could legitimately change.
        let retained = fs::read_dir("/proc/self/fd")
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| fs::read_link(entry.path()).ok())
            .filter(|path| path.starts_with(root.path()))
            .count();
        assert_eq!(retained, 0);
    }
}

#[test]
fn publisher_crash_fixture() {
    let Some(path) = std::env::var_os("LYN_GNOME_PUBLICATION_CRASH_RUNTIME") else {
        return;
    };
    let path = PathBuf::from(path);
    assert!(
        path.file_name()
            .unwrap()
            .as_bytes()
            .starts_with(b"lyn-pub-")
    );
    // NativePublication validates the private directory; this bound also stops
    // the child if its controller panics before acquiring its pidfd.
    unsafe {
        libc::alarm(5);
    }
    let mut publication = NativePublication::prepare(&path).unwrap();
    let (listener, _stop) = NativeListener::bind(publication.instance_path()).unwrap();
    publication.publish(&listener).unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
fn killed_publisher_releases_lock_and_the_next_instance_preserves_its_stale_socket() {
    use std::{
        os::{fd::OwnedFd, unix::process::ExitStatusExt},
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let root = runtime();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "context::gnome_endpoint::tests::publisher_crash_fixture",
            "--test-threads=1",
        ])
        .env("LYN_GNOME_PUBLICATION_CRASH_RUNTIME", root.path())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, child.id(), 0) };
    assert!(fd >= 0, "pidfd required; no numeric-PID fallback");
    let pin = unsafe { OwnedFd::from_raw_fd(fd as _) };
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut ready = false;
    while Instant::now() < deadline {
        if record(
            &OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY)
                .open(root.path())
                .unwrap(),
        )
        .is_ok_and(|record| record.is_some())
        {
            ready = true;
            break;
        }
        if child.try_wait().unwrap().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if !ready {
        unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                pin.as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            );
        }
        let _ = child.wait();
        panic!("private publication did not become ready");
    }
    let old_record = fs::read_to_string(root.path().join(CURRENT)).unwrap();
    let old_instance = root.path().join(old_record.trim_end_matches('\n'));
    let old_socket = old_instance.join(SOCKET);
    let before = fs::symlink_metadata(&old_socket).unwrap();
    assert!(NativePublication::prepare(root.path()).is_err());
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
    assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGKILL));
    let mut publication = NativePublication::prepare(root.path()).unwrap();
    assert_ne!(publication.instance_path(), old_instance);
    let (listener, _stop) = NativeListener::bind(publication.instance_path()).unwrap();
    publication.publish(&listener).unwrap();
    assert_ne!(
        fs::read_to_string(root.path().join(CURRENT)).unwrap(),
        old_record
    );
    let after = fs::symlink_metadata(&old_socket).unwrap();
    assert_eq!(
        (before.dev(), before.ino(), before.mode()),
        (after.dev(), after.ino(), after.mode())
    );
    drop(listener);
    drop(publication);
    assert!(old_socket.exists());
}
