//! Private native endpoint ownership and cancellable, non-polling-idle accept.

use std::{
    fs, io,
    net::Shutdown,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{
            ffi::OsStrExt,
            fs::{FileTypeExt, MetadataExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
};

const SOCKET_NAME: &str = "lyn-gnome-v1.sock";
const PENDING_CONNECTIONS: i32 = 16;

// Bind without listening: std::UnixListener::bind starts listening with a
// larger backlog before we can enforce our admission bound.
fn bind_unlistening(path: &Path) -> io::Result<UnixListener> {
    let bytes = path.as_os_str().as_bytes();
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as _;
    if bytes.len() >= address.sun_path.len() || bytes.contains(&0) {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    for (destination, byte) in address.sun_path.iter_mut().zip(bytes) {
        *destination = *byte as _;
    }
    let fd = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
            0,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // Own the descriptor immediately, including every bind failure path.
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    if unsafe {
        libc::bind(
            owned.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            std::mem::size_of_val(&address) as _,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(UnixListener::from(owned))
}

struct OwnedEndpoint {
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl Drop for OwnedEndpoint {
    fn drop(&mut self) {
        // No stale-socket sweep. Never remove a different file or symlink that
        // replaced our node while the listener was running.
        if fs::symlink_metadata(&self.path).is_ok_and(|metadata| {
            metadata.file_type().is_socket()
                && metadata.dev() == self.device
                && metadata.ino() == self.inode
        }) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(crate) struct NativeListener {
    endpoint: OwnedEndpoint,
    listener: UnixListener,
    wake: UnixStream,
}

pub(crate) struct NativeListenerStop(UnixStream);

impl NativeListenerStop {
    pub(crate) fn stop(&self) -> io::Result<()> {
        match self.0.shutdown(Shutdown::Both) {
            Err(error) if error.kind() == io::ErrorKind::NotConnected => Ok(()),
            result => result,
        }
    }
}

impl NativeListener {
    pub(crate) fn bind(runtime: &Path) -> io::Result<(Self, NativeListenerStop)> {
        if !runtime.is_absolute() {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        // Strip trailing separators/CurDir without resolving symlinks: lstat
        // on `link/` or `link/.` otherwise follows the final directory link.
        let runtime: PathBuf = runtime.components().collect();
        let metadata = fs::symlink_metadata(&runtime)?;
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
        {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        let path = runtime.join(SOCKET_NAME);
        // Bind itself arbitrates ownership. An occupied path (including a stale
        // socket) is unavailable, not permission to unlink another endpoint.
        let listener = bind_unlistening(&path)?;
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        let endpoint = OwnedEndpoint {
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        fs::set_permissions(&endpoint.path, fs::Permissions::from_mode(0o600))?;
        // Start listening only after permissions are set, with the native
        // channel's admission bound from the first possible connection.
        if unsafe { libc::listen(listener.as_raw_fd(), PENDING_CONNECTIONS) } != 0 {
            return Err(io::Error::last_os_error());
        }
        listener.set_nonblocking(true)?;
        let (wake, stop) = UnixStream::pair()?;
        Ok((
            Self {
                endpoint,
                listener,
                wake,
            },
            NativeListenerStop(stop),
        ))
    }

    pub(crate) fn socket_path(&self) -> &Path {
        &self.endpoint.path
    }

    /// Run on the broker's worker, never the GTK/capture-session thread. Only
    /// accept readiness is observed here: no provider payload is read/admitted.
    pub(crate) fn accept(&mut self) -> io::Result<Option<UnixStream>> {
        loop {
            let mut events = [
                libc::pollfd {
                    fd: self.listener.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: self.wake.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            // Block indefinitely while idle, but wake immediately on stop/EOF.
            // https://man7.org/linux/man-pages/man2/poll.2.html
            let result = unsafe { libc::poll(events.as_mut_ptr(), events.len() as _, -1) };
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            // Stop takes priority over an already queued client.
            if events[1].revents != 0 {
                return Ok(None);
            }
            if events[0].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                return Err(io::Error::from(io::ErrorKind::ConnectionAborted));
            }
            if events[0].revents & libc::POLLIN == 0 {
                continue;
            }
            match self.listener.accept() {
                Ok((stream, _)) => return Ok(Some(stream)),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::Interrupted
                            | io::ErrorKind::ConnectionAborted
                    ) =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

#[cfg(test)]
#[path = "gnome_listener_tests.rs"]
mod tests;
