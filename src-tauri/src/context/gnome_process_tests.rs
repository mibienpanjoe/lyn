use super::*;

fn stat(
    state: &str,
    ppid: u32,
    group: u32,
    session: u32,
    tty: i32,
    foreground: i32,
    started: u64,
) -> String {
    let mut fields = vec!["0".to_owned(); 20];
    for (index, value) in [
        (0, state.to_owned()),
        (1, ppid.to_string()),
        (2, group.to_string()),
        (3, session.to_string()),
        (4, tty.to_string()),
        (5, foreground.to_string()),
        (19, started.to_string()),
    ] {
        fields[index] = value;
    }
    format!("42 (discard ) name\n(parens)) {}", fields.join(" "))
}

#[test]
fn stat_fields_do_not_confuse_group_session_or_foreground() {
    let record = parse_stat(&stat("S", 8, 40, 30, 34816, 40, 123)).unwrap();
    assert_eq!(
        (
            record.parent,
            record.group,
            record.session,
            record.tty,
            record.foreground,
            record.started
        ),
        (8, 40, 30, 34816, 40, 123)
    );
    assert!(parse_stat(&stat("Z", 8, 40, 30, 34816, 40, 123)).is_none());
    assert!(parse_stat(&stat("S", 8, 40, 30, 0, -1, 123)).is_some());
    assert!(parse_stat("42 (broken) S").is_none());
}

#[test]
fn process_start_fence_reserves_a_full_kernel_tick() {
    assert!(started_before(100, 1_020_000, 100));
    assert!(!started_before(100, 1_010_000, 100));
    assert!(!started_before(101, 1_010_000, 100));
    assert!(!started_before(u64::MAX, u64::MAX, 100));
    assert!(!started_before(1, 1_000_000, 0));
}

#[test]
fn foreign_uid_in_the_target_group_is_not_silently_skipped() {
    let row = parse_stat(&stat("S", 8, 40, 30, 34816, 40, 123)).unwrap();
    assert_eq!(is_group_member(row, 1000, 1000, 40).unwrap(), true);
    assert!(is_group_member(row, 0, 1000, 40).is_err());
    assert_eq!(is_group_member(row, 0, 1000, 99).unwrap(), false);
}

#[test]
fn missing_group_and_expired_deadline_are_unresolved() {
    let invocation = ProcessInvocation::now().unwrap();
    assert!(inspect(std::process::id(), u32::MAX, 34816, invocation, u64::MAX).is_err());
    assert!(inspect(std::process::id(), std::process::id(), 34816, invocation, 0).is_err());
}

struct Fixture {
    child: std::process::Child,
    group: u32,
    tty: u64,
    directory: tempfile::TempDir,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.child.stdin.take();
        let _ = self.child.wait();
    }
}
impl Fixture {
    fn new(mode: u8) -> Self {
        use std::{
            io::{BufRead, BufReader},
            process::{Command, Stdio},
        };
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("other")).unwrap();
        let mut child = Command::new("/usr/bin/python3")
            .args([
                "-I",
                "-c",
                r#"
import fcntl,os,sys,termios
m,s=os.openpty()
tty=os.fstat(s).st_rdev
ready_r,ready_w=os.pipe()
stop_r,stop_w=os.pipe()
pid=os.fork()
if pid==0:
    os.close(m); os.close(ready_r); os.close(stop_w)
    os.setsid(); fcntl.ioctl(s,termios.TIOCSCTTY,0)
    os.chdir(sys.argv[1])
    second=0
    if sys.argv[2]!='0':
        second=os.fork()
        if second==0:
            if sys.argv[2]=='1': os.chdir('other')
            os.close(ready_w)
            os.read(stop_r,1); os._exit(0)
    os.write(ready_w,b'1'); os.close(ready_w)
    os.read(stop_r,1)
    if second: os.waitpid(second,0)
    os._exit(0)
os.close(s); os.close(ready_w); os.close(stop_r)
os.read(ready_r,1)
print(pid,tty,flush=True)
sys.stdin.buffer.read(1)
os.close(stop_w); os.waitpid(pid,0)
"#,
            ])
            .arg(directory.path())
            .arg(mode.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let values: Vec<_> = line.split_whitespace().collect();
        let fixture = Self {
            group: values[0].parse().unwrap(),
            tty: values[1].parse().unwrap(),
            child,
            directory,
        };
        // Wait on the actual kernel tick fence, not an arbitrary warmup sleep.
        let start = std::time::Instant::now();
        loop {
            let row = read_stat(&PathBuf::from(format!("/proc/{}", fixture.group)))
                .unwrap()
                .unwrap();
            if started_before(
                row.started,
                ProcessInvocation::now().unwrap().boottime_lower,
                unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as u64,
            ) {
                break;
            }
            assert!(start.elapsed().as_secs() < 1);
            std::thread::yield_now();
        }
        fixture
    }
    fn inspect(&self, server: u32, tty: u64) -> io::Result<ForegroundGroup> {
        let invocation = ProcessInvocation::now().unwrap();
        inspect(
            server,
            self.group,
            tty,
            invocation,
            invocation.monotonic + 500_000,
        )
    }
}

#[test]
fn real_foreground_group_proves_cwd_but_not_a_wrong_parent_or_tty() {
    let fixture = Fixture::new(0);
    let proof = fixture.inspect(fixture.child.id(), fixture.tty).unwrap();
    assert_eq!(
        proof.directory,
        fs::canonicalize(fixture.directory.path()).unwrap()
    );
    assert_eq!(proof.processes.len(), 1);
    assert!(fixture.inspect(1, fixture.tty).is_err());
    assert!(
        fixture
            .inspect(fixture.child.id(), fixture.tty + 1)
            .is_err()
    );
    let mut foreground = libc::pollfd {
        fd: proof.pins[0].as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    drop(fixture);
    assert_eq!(unsafe { libc::poll(&mut foreground, 1, 1000) }, 1);
    assert!(proof.revalidate_lifetimes().is_err());
}

#[test]
fn frozen_boot_fence_does_not_move_with_a_later_suspend_offset() {
    let invocation = ProcessInvocation {
        monotonic: 5_000_000,
        boottime_lower: 5_000_000,
    };
    assert!(!started_before(700, invocation.boottime_lower, 100));
    // Reconstructing with a later +100s suspend offset would wrongly accept it.
    assert!(started_before(
        700,
        invocation.boottime_lower + 100_000_000,
        100
    ));
    assert_eq!(invocation.monotonic_us(), 5_000_000);
}

#[test]
fn real_pipeline_members_with_distinct_directories_are_unresolved() {
    let fixture = Fixture::new(1);
    // The child announces readiness before the grandchild can be guaranteed to
    // have changed cwd. Wait for its authoritative cwd, bounded to this fixture.
    let start = std::time::Instant::now();
    loop {
        let children =
            fs::read_to_string(format!("/proc/{0}/task/{0}/children", fixture.group)).unwrap();
        if children.split_whitespace().any(|pid| {
            fs::canonicalize(format!("/proc/{pid}/cwd")).is_ok_and(|cwd| cwd.ends_with("other"))
                && read_stat(&PathBuf::from(format!("/proc/{pid}")))
                    .ok()
                    .flatten()
                    .is_some_and(|row| {
                        started_before(
                            row.started,
                            ProcessInvocation::now().unwrap().boottime_lower,
                            unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as u64,
                        )
                    })
        }) {
            break;
        }
        assert!(start.elapsed().as_secs() < 1);
        std::thread::yield_now();
    }
    assert!(fixture.inspect(fixture.child.id(), fixture.tty).is_err());
}

#[test]
fn real_pipeline_in_one_directory_is_a_single_coherent_group() {
    let fixture = Fixture::new(2);
    let start = std::time::Instant::now();
    let proof = loop {
        if let Ok(proof) = fixture.inspect(fixture.child.id(), fixture.tty)
            && proof.processes.len() == 2
        {
            break proof;
        }
        assert!(start.elapsed().as_secs() < 2);
        std::thread::yield_now();
    };
    assert_eq!(
        proof.directory,
        fs::canonicalize(fixture.directory.path()).unwrap()
    );
}
