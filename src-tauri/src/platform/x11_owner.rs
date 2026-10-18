//! X server-owned resource identity, never _NET_WM_PID or a client hint.

use std::{
    io::{self, IoSlice},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::net::UnixStream,
    },
    time::{Duration, Instant},
};
use x11rb::{
    protocol::{
        res::{self, Client, ClientIdMask, ClientIdSpec, ClientIdValue},
        xproto::ConnectionExt,
    },
    reexports::x11rb_protocol::{
        parse_display::{ConnectAddress, parse_display},
        xauth::get_auth,
    },
    rust_connection::{DefaultStream, PollMode, RustConnection, Stream},
    utils::RawFdContainer,
};

const IO_BUDGET: Duration = Duration::from_millis(100);

fn version_supported(major: u16, minor: u16) -> bool {
    (major, minor) == (1, 2)
}

fn client_base(window: u32, clients: &[Client]) -> Option<u32> {
    if window == 0 || clients.len() > 4096 {
        return None;
    }
    let mut matches = clients.iter().filter(|client| {
        client.resource_base != 0
            && client.resource_base & client.resource_mask == 0
            && window & !client.resource_mask == client.resource_base
    });
    let client = matches.next()?;
    matches.next().is_none().then_some(client.resource_base)
}

fn accepts_server(credentials: libc::ucred, uid: u32) -> bool {
    credentials.pid > 0 && (credentials.uid == 0 || credentials.uid == uid)
}

fn trusted_server(socket: &UnixStream) -> bool {
    let mut credentials = std::mem::MaybeUninit::<libc::ucred>::uninit();
    let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            credentials.as_mut_ptr().cast(),
            &mut size,
        )
    };
    result == 0
        && size as usize == std::mem::size_of::<libc::ucred>()
        && accepts_server(unsafe { credentials.assume_init() }, unsafe {
            libc::geteuid()
        })
}

fn parse_owner(window: u32, ids: &[ClientIdValue]) -> Option<u32> {
    let [id] = ids else {
        return None;
    };
    let [pid] = id.value.as_slice() else {
        return None;
    };
    (window != 0
        && id.spec.client == window
        && id.spec.mask == ClientIdMask::LOCAL_CLIENT_PID
        && (1..=i32::MAX as u32).contains(pid))
    .then_some(*pid)
}

fn wait_fd(fd: i32, events: i16, deadline: Instant) -> io::Result<()> {
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|v| !v.is_zero())
            .ok_or_else(|| io::Error::from(io::ErrorKind::TimedOut))?;
        let mut event = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        let result = unsafe {
            libc::poll(
                &mut event,
                1,
                remaining
                    .as_millis()
                    .saturating_add(1)
                    .min(i32::MAX as u128) as i32,
            )
        };
        if Instant::now() >= deadline {
            return Err(io::Error::from(io::ErrorKind::TimedOut));
        }
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if event.revents & events != 0 {
            return Ok(());
        }
        if event.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(io::Error::from(io::ErrorKind::NotConnected));
        }
    }
}

struct DeadlineStream {
    inner: DefaultStream,
    deadline: Instant,
}

impl DeadlineStream {
    fn check(&self) -> io::Result<()> {
        if Instant::now() >= self.deadline {
            Err(io::Error::from(io::ErrorKind::TimedOut))
        } else {
            Ok(())
        }
    }
}

impl Stream for DeadlineStream {
    fn poll(&self, mode: PollMode) -> io::Result<()> {
        let events = if mode.readable() { libc::POLLIN } else { 0 }
            | if mode.writable() { libc::POLLOUT } else { 0 };
        wait_fd(self.inner.as_raw_fd(), events, self.deadline)
    }
    fn read(&self, buf: &mut [u8], fds: &mut Vec<RawFdContainer>) -> io::Result<usize> {
        self.check()?;
        let count = self.inner.read(buf, fds)?;
        if !fds.is_empty() {
            fds.clear();
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        Ok(count)
    }
    fn write(&self, buf: &[u8], fds: &mut Vec<RawFdContainer>) -> io::Result<usize> {
        self.check()?;
        if !fds.is_empty() {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        self.inner.write(buf, fds)
    }
    fn write_vectored(
        &self,
        buf: &[IoSlice<'_>],
        fds: &mut Vec<RawFdContainer>,
    ) -> io::Result<usize> {
        self.check()?;
        if !fds.is_empty() {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        self.inner.write_vectored(buf, fds)
    }
}

fn connect_local(path: &str, abstract_socket: bool, deadline: Instant) -> io::Result<UnixStream> {
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as _;
    let offset = usize::from(abstract_socket);
    if path.as_bytes().contains(&0) || path.len() + offset >= address.sun_path.len() {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    for (destination, byte) in address.sun_path[offset..].iter_mut().zip(path.bytes()) {
        *destination = byte as _;
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
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    let length = std::mem::offset_of!(libc::sockaddr_un, sun_path) + path.len() + 1;
    let result = unsafe {
        libc::connect(
            fd,
            (&address as *const libc::sockaddr_un).cast(),
            length as _,
        )
    };
    if result != 0 {
        let error = io::Error::last_os_error();
        // AF_UNIX EAGAIN means a full queue, not a pending connect: refuse.
        if error.raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(error);
        }
        wait_fd(fd, libc::POLLOUT, deadline)?;
        let mut error: i32 = 0;
        let mut length = std::mem::size_of_val(&error) as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                (&mut error as *mut i32).cast(),
                &mut length,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error));
        }
    }
    Ok(UnixStream::from(owned))
}

/// Worker-only, metadata-only lookup. Remote/TCP displays and unavailable XRes
/// stay unresolved. Xauthority is read by the installed x11rb implementation;
/// its trusted local configuration is not an untrusted capture input.
pub(crate) fn local_owner(window: u32) -> Option<u32> {
    if window == 0 {
        return None;
    }
    let deadline = Instant::now() + IO_BUDGET;
    let display = parse_display(None).ok()?;
    let path = display
        .connect_instruction()
        .find_map(|address| match address {
            ConnectAddress::Socket(path) => Some(path),
            _ => None,
        })?;
    let socket = connect_local(&path, true, deadline)
        .or_else(|_| connect_local(&path, false, deadline))
        .ok()?;
    if !trusted_server(&socket) {
        return None;
    }
    let (inner, (family, address)) = DefaultStream::from_unix_stream(socket).ok()?;
    let (name, auth) = get_auth(family, &address, display.display)
        .ok()?
        .unwrap_or_default();
    let connection = RustConnection::connect_to_stream_with_auth_info(
        DeadlineStream { inner, deadline },
        display.screen.into(),
        name,
        auth,
    )
    .ok()?;
    let version = res::query_version(&connection, 1, 2).ok()?.reply().ok()?;
    if !version_supported(version.server_major, version.server_minor) {
        return None;
    }
    // The XRes specification accepts an XID in a client's range, even when the
    // resource no longer exists. Require an actual live Window on both sides.
    connection
        .get_window_attributes(window)
        .ok()?
        .reply()
        .ok()?;
    // Xorg returns the owning client's resource_base, not necessarily the
    // window XID. Derive that base from the server's ranges; never accept an
    // arbitrary nonmatching ID returned by QueryClientIds.
    let clients = res::query_clients(&connection).ok()?.reply().ok()?.clients;
    let base = client_base(window, &clients)?;
    let ids = res::query_client_ids(
        &connection,
        &[ClientIdSpec {
            client: base,
            mask: ClientIdMask::LOCAL_CLIENT_PID,
        }],
    )
    .ok()?
    .reply()
    .ok()?
    .ids;
    let pid = parse_owner(base, &ids)?;
    connection
        .get_window_attributes(window)
        .ok()?
        .reply()
        .ok()?;
    (Instant::now() < deadline).then_some(pid)
}

#[cfg(test)]
#[path = "x11_owner_tests.rs"]
mod tests;
