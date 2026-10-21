//! Foreground group provenance from kernel metadata; never terminal content.

use super::gnome_channel::monotonic_us;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::MetadataExt,
    },
    path::{Path, PathBuf},
};

const MAX_PROCESSES: usize = 8192;
const MAX_MEMBERS: usize = 64;
const MAX_ANCESTRY: usize = 64;
const MAX_PINS: usize = 256;
const MAX_SELF_STATUS: usize = 65536;

/// Mint on Rust's invocation boundary, before popup/owner/socket work. Never
/// reconstruct BOOTTIME later from a historical MONOTONIC timestamp.
#[derive(Clone, Copy)]
pub(crate) struct ProcessInvocation {
    monotonic: u64,
    boottime_lower: u64,
}

impl ProcessInvocation {
    pub(crate) fn now() -> io::Result<Self> {
        let mut time = std::mem::MaybeUninit::<libc::timespec>::uninit();
        if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, time.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let time = unsafe { time.assume_init() };
        let boottime_lower = u64::try_from(time.tv_sec)
            .ok()
            .and_then(|s| s.checked_mul(1_000_000))
            .and_then(|s| s.checked_add(u64::try_from(time.tv_nsec).ok()? / 1000))
            .ok_or_else(unavailable)?;
        Ok(Self {
            monotonic: monotonic_us().map_err(|_| unavailable())?,
            boottime_lower,
        })
    }
    pub(crate) fn monotonic_us(self) -> u64 {
        self.monotonic
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stat {
    parent: u32,
    group: u32,
    session: u32,
    tty: u32,
    foreground: i32,
    started: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct Process {
    stat: Stat,
    proc_dev: u64,
    proc_inode: u64,
    exe_dev: u64,
    exe_inode: u64,
    cwd: PathBuf,
    cwd_dev: u64,
    cwd_inode: u64,
}

/// Retains kernel lifetime pins. Paths/identities stay Rust-owned and ephemeral.
#[derive(Debug)]
pub(super) struct ForegroundGroup {
    pub(super) directory: PathBuf,
    processes: BTreeMap<u32, Process>,
    ancestors: BTreeMap<u32, (Stat, u64, u64)>,
    pins: Vec<OwnedFd>,
}

impl PartialEq for ForegroundGroup {
    fn eq(&self, other: &Self) -> bool {
        self.directory == other.directory
            && self.processes == other.processes
            && self.ancestors == other.ancestors
    }
}

impl ForegroundGroup {
    pub(super) fn revalidate_lifetimes(&self) -> io::Result<()> {
        for pin in &self.pins {
            let mut event = libc::pollfd {
                fd: pin.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            if unsafe { libc::poll(&mut event, 1, 0) } != 0 {
                return Err(unavailable());
            }
        }
        Ok(())
    }
}

pub(super) fn inspect(
    server: u32,
    group: u32,
    tty: u64,
    invocation: ProcessInvocation,
    deadline: u64,
) -> io::Result<ForegroundGroup> {
    #[cfg(test)]
    let inspection_started = monotonic_us().map_err(|_| unavailable())?;
    check_deadline(deadline)?;
    if group == 0 || group > i32::MAX as u32 || tty == 0 {
        return Err(unavailable());
    }
    require_same_proc_pid_view()?;
    let uid = unsafe { libc::geteuid() };
    let invocation_boot = invocation.boottime_lower;
    if monotonic_us().map_err(|_| unavailable())? < invocation.monotonic {
        return Err(unavailable());
    }
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let ticks = u64::try_from(ticks).map_err(|_| unavailable())?;
    let mut result = ForegroundGroup {
        directory: PathBuf::new(),
        processes: BTreeMap::new(),
        ancestors: BTreeMap::new(),
        pins: Vec::new(),
    };
    for (count, entry) in fs::read_dir("/proc")?.enumerate() {
        check_deadline(deadline).inspect_err(|_| {
            #[cfg(test)]
            eprintln!("native process scan timed out: entries={count}, initialRemainingUs={}, elapsedUs={}",
                deadline.saturating_sub(inspection_started),
                monotonic_us().unwrap_or(inspection_started).saturating_sub(inspection_started));
        })?;
        if count >= MAX_PROCESSES {
            return Err(unavailable());
        }
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|text| text.parse::<u32>().ok())
        else {
            continue;
        };
        // Exclusion only, never admission. The namespace guard above makes
        // syscall and procfs numbers comparable; all matching members still
        // pass the complete UID, lifetime, TTY, ancestry and cwd checks below.
        if !kernel_group_matches(pid, group)? {
            continue;
        }
        let path = entry.path();
        let before = match fs::metadata(&path) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let stat = match read_stat(&path) {
            Ok(Some(value)) => value,
            Ok(None) => continue,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if !is_group_member(stat, before.uid(), uid, group)? {
            continue;
        }
        if result.processes.len() >= MAX_MEMBERS
            || stat.foreground != group as i32
            || u64::from(stat.tty) != tty
            || !started_before(stat.started, invocation_boot, ticks)
        {
            return Err(unavailable());
        }
        let pin = pin(pid)?;
        same_namespace_and_root(&path)?;
        let exe = fs::metadata(path.join("exe"))?;
        // Known local proxies do not establish the remote/multiplexer pane cwd.
        for binary in [
            "ssh",
            "mosh-client",
            "tmux",
            "screen",
            "docker",
            "podman",
            "nsenter",
            "flatpak",
        ] {
            if fs::metadata(format!("/usr/bin/{binary}"))
                .is_ok_and(|known| (known.dev(), known.ino()) == (exe.dev(), exe.ino()))
            {
                return Err(unavailable());
            }
        }
        let cwd = fs::canonicalize(path.join("cwd"))?;
        let directory = fs::metadata(&cwd)?;
        let after = fs::metadata(&path)?;
        if !directory.is_dir()
            || before.uid() != after.uid()
            || (before.dev(), before.ino()) != (after.dev(), after.ino())
            || read_stat(&path)? != Some(stat)
        {
            return Err(unavailable());
        }
        if !result.directory.as_os_str().is_empty() && result.directory != cwd {
            return Err(unavailable());
        }
        result.directory = cwd.clone();
        result.processes.insert(
            pid,
            Process {
                stat,
                proc_dev: before.dev(),
                proc_inode: before.ino(),
                exe_dev: exe.dev(),
                exe_inode: exe.ino(),
                cwd,
                cwd_dev: directory.dev(),
                cwd_inode: directory.ino(),
            },
        );
        result.pins.push(pin);
    }
    if result.processes.is_empty() {
        return Err(unavailable());
    }
    let session = result
        .processes
        .values()
        .next()
        .ok_or_else(unavailable)?
        .stat
        .session;
    for process in result.processes.values() {
        if process.stat.session != session {
            return Err(unavailable());
        }
        let mut parent = process.stat.parent;
        let mut reached = false;
        for _ in 0..MAX_ANCESTRY {
            check_deadline(deadline)?;
            if parent == server {
                reached = true;
                break;
            }
            if parent <= 1 {
                return Err(unavailable());
            }
            let root = PathBuf::from(format!("/proc/{parent}"));
            if result.pins.len() >= MAX_PINS {
                return Err(unavailable());
            }
            let pin = pin(parent)?;
            let metadata = fs::metadata(&root)?;
            let stat = read_stat(&root)?.ok_or_else(unavailable)?;
            if metadata.uid() != uid
                || stat.session != session
                || u64::from(stat.tty) != tty
                || stat.foreground != group as i32
                || !started_before(stat.started, invocation_boot, ticks)
            {
                return Err(unavailable());
            }
            same_namespace_and_root(&root)?;
            let identity = (stat, metadata.dev(), metadata.ino());
            if result
                .ancestors
                .insert(parent, identity)
                .is_some_and(|old| old != identity)
            {
                return Err(unavailable());
            }
            result.pins.push(pin);
            parent = stat.parent;
        }
        if !reached {
            return Err(unavailable());
        }
    }
    result.revalidate_lifetimes()?;
    check_deadline(deadline)?;
    Ok(result)
}

fn same_namespace_and_root(process: &Path) -> io::Result<()> {
    for relative in ["ns/mnt", "ns/user", "ns/pid", "root"] {
        let ours = fs::metadata(Path::new("/proc/self").join(relative))?;
        let theirs = fs::metadata(process.join(relative))?;
        if (ours.dev(), ours.ino()) != (theirs.dev(), theirs.ino()) {
            return Err(unavailable());
        }
    }
    Ok(())
}

fn require_same_proc_pid_view() -> io::Result<()> {
    let mut status = String::new();
    File::open("/proc/self/status")?
        .take((MAX_SELF_STATUS + 1) as u64)
        .read_to_string(&mut status)?;
    same_proc_pid_view(&status, std::process::id())
}

fn same_proc_pid_view(status: &str, pid: u32) -> io::Result<()> {
    if status.len() > MAX_SELF_STATUS || pid == 0 || pid > i32::MAX as u32 {
        return Err(unavailable());
    }
    // NSpid is ordered from the namespace mounting procfs to the innermost
    // one. Equal numeric PIDs in two namespaces still mean different views.
    let mut fields = status
        .lines()
        .filter_map(|line| line.strip_prefix("NSpid:"));
    let mut ids = fields
        .next()
        .ok_or_else(unavailable)?
        .split_ascii_whitespace();
    if ids.next() != Some(pid.to_string().as_str())
        || ids.next().is_some()
        || fields.next().is_some()
    {
        return Err(unavailable());
    }
    Ok(())
}

fn probe_group_with(
    pid: u32,
    group: u32,
    observe: impl FnOnce(libc::pid_t) -> io::Result<u32>,
) -> io::Result<bool> {
    if pid == 0 || pid > i32::MAX as u32 || group == 0 || group > i32::MAX as u32 {
        return Err(unavailable());
    }
    match observe(pid as libc::pid_t) {
        // Kernel threads may have PGID zero; the requested terminal group
        // cannot. Zero is an observed nonmember, not a syscall failure.
        Ok(actual) if actual <= i32::MAX as u32 => Ok(actual == group),
        Ok(_) => Err(unavailable()),
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(false),
        Err(error) => Err(error),
    }
}

fn kernel_group_matches(pid: u32, group: u32) -> io::Result<bool> {
    probe_group_with(pid, group, |pid| {
        let actual = unsafe { libc::getpgid(pid) };
        if actual < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(actual as u32)
        }
    })
}

fn pin(pid: u32) -> io::Result<OwnedFd> {
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0_u32) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd as i32) })
}

fn read_stat(process: &Path) -> io::Result<Option<Stat>> {
    let mut text = String::new();
    File::open(process.join("stat"))?
        .take(4097)
        .read_to_string(&mut text)?;
    if text.len() > 4096 {
        return Err(unavailable());
    }
    Ok(parse_stat(&text))
}

fn is_group_member(stat: Stat, actual_uid: u32, expected_uid: u32, group: u32) -> io::Result<bool> {
    if stat.group != group {
        return Ok(false);
    }
    if actual_uid != expected_uid {
        return Err(unavailable());
    }
    Ok(true)
}

fn parse_stat(text: &str) -> Option<Stat> {
    let fields: Vec<_> = text
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .take(20)
        .collect();
    if !matches!(*fields.first()?, "R" | "S" | "D" | "T" | "t" | "I") {
        return None;
    }
    Some(Stat {
        parent: fields.get(1)?.parse().ok()?,
        group: fields.get(2)?.parse().ok()?,
        session: fields.get(3)?.parse().ok()?,
        tty: fields.get(4)?.parse::<i32>().ok()? as u32,
        foreground: fields.get(5)?.parse().ok()?,
        started: fields.get(19)?.parse::<u64>().ok().filter(|n| *n > 0)?,
    })
}

fn started_before(started: u64, invocation_boot_us: u64, ticks: u64) -> bool {
    // starttime has only tick precision. Its entire tick must precede invoke.
    ticks > 0
        && started
            .checked_add(1)
            .and_then(|n| n.checked_mul(1_000_000))
            .zip(invocation_boot_us.checked_mul(ticks))
            .is_some_and(|(end, invoke)| end < invoke)
}

fn check_deadline(deadline: u64) -> io::Result<()> {
    if monotonic_us().map_err(|_| unavailable())? >= deadline {
        return Err(io::ErrorKind::TimedOut.into());
    }
    Ok(())
}
fn unavailable() -> io::Error {
    io::ErrorKind::NotConnected.into()
}

#[cfg(test)]
#[path = "gnome_process_tests.rs"]
mod tests;
