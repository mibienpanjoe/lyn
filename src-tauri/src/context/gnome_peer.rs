//! Kernel-authenticated native server identity, never client-supplied identity.

use std::{
    fs::{self, File},
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{fs::MetadataExt, net::UnixStream},
    },
    path::{Path, PathBuf},
};

use super::unix_broker::peer_credentials;

const SERVER_PATHS: &[&str] = &[
    "/usr/libexec/gnome-terminal-server",
    "/usr/lib/gnome-terminal/gnome-terminal-server",
];

#[derive(Clone, Copy, PartialEq, Eq)]
struct ProcessIdentity {
    uid: u32,
    proc_dev: u64,
    proc_ino: u64,
    exe_dev: u64,
    exe_ino: u64,
    started: u64,
}

pub(crate) struct NativePeer {
    pid: u32,
    identity: ProcessIdentity,
    lifetime: OwnedFd,
}

impl NativePeer {
    pub(crate) fn authenticate(stream: &UnixStream) -> io::Result<Self> {
        let uid = unsafe { libc::geteuid() };
        for path in SERVER_PATHS {
            let path = Path::new(path);
            if fs::metadata(path)
                .is_ok_and(|file| file.is_file() && file.uid() == 0 && file.mode() & 0o022 == 0)
                && let Ok(peer) = Self::authenticate_executable(stream, uid, path)
            {
                return Ok(peer);
            }
        }
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }

    fn authenticate_executable(
        stream: &UnixStream,
        uid: u32,
        executable: &Path,
    ) -> io::Result<Self> {
        let credentials = peer_credentials(stream)?;
        if credentials.uid != uid {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        let pid = credentials.pid as u32;
        let identity = process_identity(pid)?;
        let expected = fs::metadata(executable)?;
        if identity.uid != uid
            || !expected.is_file()
            || (identity.exe_dev, identity.exe_ino) != (expected.dev(), expected.ino())
        {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        // A pidfd tracks this process after exit/recycling, not whichever process
        // might later own its numeric PID. It is never used to signal or getfd.
        // https://man7.org/linux/man-pages/man2/pidfd_open.2.html
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, credentials.pid, 0_u32) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let peer = Self {
            pid,
            identity,
            lifetime: unsafe { OwnedFd::from_raw_fd(fd as i32) },
        };
        peer.revalidate()?;
        Ok(peer)
    }

    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    pub(crate) fn revalidate(&self) -> io::Result<()> {
        let mut event = libc::pollfd {
            fd: self.lifetime.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut event, 1, 0) };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        if result != 0 || process_identity(self.pid)? != self.identity {
            return Err(io::Error::from(io::ErrorKind::NotConnected));
        }
        Ok(())
    }
}

fn process_identity(pid: u32) -> io::Result<ProcessIdentity> {
    let root = PathBuf::from(format!("/proc/{pid}"));
    let before = fs::metadata(&root)?;
    let executable = fs::metadata(root.join("exe"))?;
    let mut text = String::new();
    File::open(root.join("stat"))?
        .take(4097)
        .read_to_string(&mut text)?;
    if text.len() > 4096 {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    let started = start_time(&text).ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
    let after = fs::metadata(&root)?;
    if (before.uid(), before.dev(), before.ino()) != (after.uid(), after.dev(), after.ino()) {
        return Err(io::Error::from(io::ErrorKind::NotConnected));
    }
    Ok(ProcessIdentity {
        uid: before.uid(),
        proc_dev: before.dev(),
        proc_ino: before.ino(),
        exe_dev: executable.dev(),
        exe_ino: executable.ino(),
        started,
    })
}

fn start_time(text: &str) -> Option<u64> {
    // comm (field 2) can contain spaces/newlines/parentheses. Discard it; never
    // retain/log it. The remaining state and starttime are fields 3 and 22.
    let (_, fields) = text.rsplit_once(')')?;
    let fields: Vec<_> = fields.split_whitespace().take(20).collect();
    if !matches!(*fields.first()?, "R" | "S" | "D" | "T" | "t" | "I") {
        return None;
    }
    fields
        .get(19)?
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
}

#[cfg(test)]
#[path = "gnome_peer_tests.rs"]
mod tests;
