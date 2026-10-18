//! Kernel PTY metadata only. Never open, duplicate or read a terminal stream.

use std::{
    fs::{self, File},
    io::{self, Read},
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::Path,
};

use super::gnome_channel::monotonic_us;

const MAX_FDS: usize = 4096;
const MAX_FDINFO: usize = 16 * 1024;

/// A live observation, not a permanent terminal identity or capture authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PtyWitness {
    master_fd: u32,
    mount_id: u64,
    filesystem: u64,
    master_filesystem: u64,
    master_inode: u64,
    slave_inode: u64,
    pub(super) slave_device: u64,
}

pub(super) fn inspect(pid: u32, number: u32, deadline: u64) -> io::Result<PtyWitness> {
    check_deadline(deadline)?;
    let root = std::path::PathBuf::from(format!("/proc/{pid}"));
    let uid = unsafe { libc::geteuid() };
    if fs::metadata(&root)?.uid() != uid {
        return Err(unavailable());
    }
    // Only the broker's devpts instance is supported. Reject namespace
    // indirection rather than equating identical numbers across containers.
    for namespace in ["mnt", "user", "pid"] {
        let ours = fs::metadata(format!("/proc/self/ns/{namespace}"))?;
        let theirs = fs::metadata(root.join(format!("ns/{namespace}")))?;
        if (ours.dev(), ours.ino()) != (theirs.dev(), theirs.ino()) {
            return Err(unavailable());
        }
    }
    let slave_path = format!("/dev/pts/{number}");
    let slave = fs::metadata(&slave_path)?;
    if !slave.file_type().is_char_device() || slave.uid() != uid {
        return Err(unavailable());
    }
    let mut found = None;
    for (count, entry) in fs::read_dir(root.join("fdinfo"))?.enumerate() {
        check_deadline(deadline)?;
        if count >= MAX_FDS {
            return Err(unavailable());
        }
        let entry = entry?;
        let Some(fd) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            return Err(unavailable());
        };
        // Opening fdinfo reads kernel metadata, NOT /proc/PID/fd/FD.
        let text = match read_fdinfo(&entry.path()) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let Some((mount_id, inode, index)) = parse_fdinfo(&text) else {
            continue;
        };
        if index != number {
            continue;
        }
        let master = fs::metadata(root.join(format!("fd/{fd}")))?;
        // PTMX is 5:2. A direct devpts master shares the slave's superblock.
        // Legacy /dev/ptmx instead lives on devtmpfs: check that exact trusted
        // node/root and the kernel's sibling /dev/pts route, not rdev alone.
        if !master.file_type().is_char_device()
            || master.rdev() != libc::makedev(5, 2)
            || !master_routes_to_slave(&root, fd, &master, &slave)?
            || master.ino() != inode
            || read_fdinfo(&entry.path())? != text
        {
            return Err(unavailable());
        }
        let witness = PtyWitness {
            master_fd: fd,
            mount_id,
            filesystem: slave.dev(),
            master_filesystem: master.dev(),
            master_inode: inode,
            slave_inode: slave.ino(),
            slave_device: slave.rdev(),
        };
        // Duplicated masters are conservatively unresolved, never first-match.
        if found.replace(witness).is_some() {
            return Err(unavailable());
        }
    }
    let after = fs::metadata(slave_path)?;
    if (slave.dev(), slave.ino(), slave.rdev(), slave.uid())
        != (after.dev(), after.ino(), after.rdev(), after.uid())
    {
        return Err(unavailable());
    }
    check_deadline(deadline)?;
    found.ok_or_else(unavailable)
}

fn master_routes_to_slave(
    root: &Path,
    fd: u32,
    master: &fs::Metadata,
    slave: &fs::Metadata,
) -> io::Result<bool> {
    let our_root = fs::metadata("/")?;
    let peer_root = fs::metadata(root.join("root"))?;
    if (our_root.dev(), our_root.ino()) != (peer_root.dev(), peer_root.ino()) {
        return Ok(false);
    }
    let target = fs::read_link(root.join(format!("fd/{fd}")))?;
    if target == Path::new("/dev/pts/ptmx") {
        return Ok(master.dev() == slave.dev());
    }
    if target != Path::new("/dev/ptmx") {
        return Ok(false);
    }
    let node = fs::symlink_metadata("/dev/ptmx")?;
    let dev = fs::metadata("/dev")?;
    let pts = fs::metadata("/dev/pts")?;
    // This legacy route assumes root-managed mount topology. User/mount/PID
    // namespaces were checked above; reject chroots and relocated device nodes.
    // https://docs.kernel.org/filesystems/devpts.html
    Ok(node.file_type().is_char_device()
        && node.uid() == 0
        && (master.dev(), master.ino()) == (node.dev(), node.ino())
        && pts.is_dir()
        && pts.uid() == 0
        && pts.mode() & 0o022 == 0
        && dev.is_dir()
        && dev.uid() == 0
        && dev.mode() & 0o022 == 0
        && pts.dev() == slave.dev()
        && (our_root.dev(), our_root.ino()) == (peer_root.dev(), peer_root.ino()))
}

fn read_fdinfo(path: &Path) -> io::Result<String> {
    let mut text = String::new();
    File::open(path)?
        .take(MAX_FDINFO as u64 + 1)
        .read_to_string(&mut text)?;
    if text.len() > MAX_FDINFO {
        return Err(unavailable());
    }
    Ok(text)
}

fn parse_fdinfo(text: &str) -> Option<(u64, u64, u32)> {
    let (mut mount, mut inode, mut index) = (None, None, None);
    for line in text.lines() {
        let (key, value) = line.split_once(':')?;
        match key {
            "mnt_id" if mount.is_none() => {
                mount = Some(value.trim().parse::<u64>().ok().filter(|n| *n > 0)?)
            }
            "ino" if inode.is_none() => {
                inode = Some(value.trim().parse::<u64>().ok().filter(|n| *n > 0)?)
            }
            "tty-index" if index.is_none() => index = Some(value.trim().parse::<u32>().ok()?),
            "mnt_id" | "ino" | "tty-index" => return None,
            _ => {}
        }
    }
    Some((mount?, inode?, index?))
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
#[path = "gnome_pty_tests.rs"]
mod tests;
