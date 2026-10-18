use super::*;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

fn fixture() -> (OwnedFd, OwnedFd, u32) {
    let (mut master, mut slave) = (-1, -1);
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        },
        0
    );
    let master = unsafe { OwnedFd::from_raw_fd(master) };
    let slave = unsafe { OwnedFd::from_raw_fd(slave) };
    let mut number = 0_u32;
    assert_eq!(
        unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCGPTN, &mut number) },
        0
    );
    (master, slave, number)
}

#[test]
fn a_real_master_is_required_and_its_close_revokes_the_witness() {
    let (master, _slave, number) = fixture();
    let witness = inspect(std::process::id(), number, u64::MAX).unwrap();
    assert_eq!(witness.master_fd, master.as_raw_fd() as u32);
    assert_eq!(
        inspect(std::process::id(), number, u64::MAX).unwrap(),
        witness
    );
    drop(master);
    assert!(inspect(std::process::id(), number, u64::MAX).is_err());
}

#[test]
fn a_slave_number_does_not_prove_master_ownership() {
    let (_master, _slave, number) = fixture();
    assert!(inspect(1, number, u64::MAX).is_err());
    assert!(inspect(std::process::id(), u32::MAX, u64::MAX).is_err());
    assert!(inspect(std::process::id(), number, 0).is_err());
}

#[test]
fn duplicate_master_is_unresolved_until_the_duplicate_closes() {
    let (master, _slave, number) = fixture();
    let fd = master.as_raw_fd();
    assert!(inspect(std::process::id(), number, u64::MAX).is_ok());
    let duplicated = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    assert!(duplicated >= 0);
    let duplicate = unsafe { OwnedFd::from_raw_fd(duplicated) };
    assert!(inspect(std::process::id(), number, u64::MAX).is_err());
    drop(duplicate);
    assert!(inspect(std::process::id(), number, u64::MAX).is_ok());
    drop(master);
}

#[test]
fn a_direct_devpts_route_must_not_bypass_a_changed_process_root() {
    use std::os::unix::fs::symlink;
    let (_master, _slave, number) = fixture();
    let proc = tempfile::tempdir().unwrap();
    fs::create_dir(proc.path().join("fd")).unwrap();
    symlink(proc.path(), proc.path().join("root")).unwrap();
    symlink("/dev/pts/ptmx", proc.path().join("fd/7")).unwrap();
    let master = fs::metadata("/dev/pts/ptmx").unwrap();
    let slave = fs::metadata(format!("/dev/pts/{number}")).unwrap();
    assert!(!master_routes_to_slave(proc.path(), 7, &master, &slave).unwrap());
}

#[test]
fn fdinfo_read_refuses_oversize_metadata() {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(&vec![b'x'; MAX_FDINFO + 1]).unwrap();
    assert!(read_fdinfo(file.path()).is_err());
}

#[test]
fn another_same_user_process_holding_the_master_does_not_attest_our_process() {
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
    };
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Child(Command::new("/usr/bin/python3").args(["-I", "-c",
        "import os,sys; m,s=os.openpty(); print(os.ttyname(s).rsplit('/',1)[1],flush=True); sys.stdin.buffer.read(1)"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap());
    let mut number = String::new();
    BufReader::new(child.0.stdout.take().unwrap())
        .read_line(&mut number)
        .unwrap();
    let number = number.trim().parse().unwrap();
    assert!(inspect(child.0.id(), number, u64::MAX).is_ok());
    assert!(inspect(std::process::id(), number, u64::MAX).is_err());
    drop(child.0.stdin.take());
    assert!(child.0.wait().unwrap().success());
}

#[test]
fn fdinfo_is_bounded_canonical_and_duplicate_keys_are_refused() {
    assert_eq!(
        parse_fdinfo("pos:\t0\nflags:\t02\nmnt_id:\t42\nino:\t2\ntty-index:\t0\n"),
        Some((42, 2, 0))
    );
    for text in [
        "tty-index: 1\n",
        "mnt_id: 0\nino: 2\ntty-index: 1\n",
        "mnt_id: 2\nino: 2\ntty-index: -1\n",
        "mnt_id: 2\nino: 2\ntty-index: 1\ntty-index: 1\n",
        "mnt_id: 2 3\nino: 2\ntty-index: 1\n",
    ] {
        assert_eq!(parse_fdinfo(text), None, "{text:?}");
    }
}
