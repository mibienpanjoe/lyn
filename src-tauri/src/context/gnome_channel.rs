//! Bounded native metadata exchange. No session locks, PTY access or persistence.

use super::{
    gnome_peer::NativePeer,
    gnome_protocol::{
        Candidate, Correlation, FRAME_BYTES, Frame, PendingQuery, REPLY_BUDGET_US, ReplyReject,
    },
};
use std::{
    io,
    net::Shutdown,
    os::{fd::AsRawFd, unix::net::UnixStream},
};
use uuid::Uuid;

const HANDSHAKE_BUDGET_US: u64 = 500_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChannelError {
    Unavailable,
    Disconnected,
    Timeout,
    Protocol,
    Reply(ReplyReject),
}

/// One admitted server connection, serialized by exclusive ownership. Results
/// are still untrusted native evidence until window/PTY/process attestation.
pub(crate) struct NativeChannel {
    stream: Option<UnixStream>,
    peer: NativePeer,
    generation: Uuid,
}

impl NativeChannel {
    pub(crate) fn connect(stream: UnixStream) -> Result<Self, ChannelError> {
        let deadline = monotonic_us()?
            .checked_add(HANDSHAKE_BUDGET_US)
            .ok_or(ChannelError::Timeout)?;
        let peer = NativePeer::authenticate(&stream).map_err(|_| ChannelError::Unavailable)?;
        Self::handshake(stream, peer, deadline)
    }

    fn handshake(
        mut stream: UnixStream,
        peer: NativePeer,
        deadline: u64,
    ) -> Result<Self, ChannelError> {
        stream
            .set_nonblocking(true)
            .map_err(|_| ChannelError::Unavailable)?;
        if read_frame(&mut stream, deadline)? != Frame::Hello {
            return Err(ChannelError::Protocol);
        }
        ensure_idle(&stream, deadline)?;
        let generation = Uuid::new_v4();
        write_frame(&mut stream, Frame::Bind { generation }, deadline)?;
        if read_frame(&mut stream, deadline)? != (Frame::Ready { generation }) {
            return Err(ChannelError::Protocol);
        }
        ensure_idle(&stream, deadline)?;
        peer.revalidate().map_err(|_| ChannelError::Disconnected)?;
        check_deadline(deadline)?;
        Ok(Self {
            stream: Some(stream),
            peer,
            generation,
        })
    }

    pub(crate) fn query(
        &mut self,
        window_id: u32,
        invoked_us: u64,
    ) -> Result<Candidate, ChannelError> {
        if self.stream.is_none() {
            return Err(ChannelError::Disconnected);
        }
        let result = self.query_once(window_id, invoked_us);
        // An explicit refusal is a complete correlated reply, not a broken
        // transport. Other errors invalidate all future use of this generation.
        if result.is_err()
            && !matches!(result, Err(ChannelError::Reply(ReplyReject::Unresolved(_))))
        {
            if let Some(stream) = self.stream.take() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
        result
    }

    fn query_once(&mut self, window_id: u32, invoked_us: u64) -> Result<Candidate, ChannelError> {
        // A clock-domain mistake must not turn the 120ms bound into an
        // arbitrarily distant deadline. Invocation is always in the past.
        if invoked_us > monotonic_us()? {
            return Err(ChannelError::Protocol);
        }
        let correlation = Correlation {
            generation: self.generation,
            request_id: Uuid::new_v4(),
            window_id,
        };
        let mut pending =
            PendingQuery::new(correlation, invoked_us).ok_or(ChannelError::Protocol)?;
        let deadline = invoked_us
            .checked_add(REPLY_BUDGET_US)
            .ok_or(ChannelError::Timeout)?;
        check_deadline(deadline)?;
        self.peer
            .revalidate()
            .map_err(|_| ChannelError::Disconnected)?;
        let stream = self.stream.as_mut().ok_or(ChannelError::Disconnected)?;
        ensure_idle(stream, deadline)?;
        write_frame(
            stream,
            Frame::Query {
                correlation,
                invoked_us,
            },
            deadline,
        )?;
        let reply = read_frame(stream, deadline)?;
        ensure_idle(stream, deadline)?;
        self.peer
            .revalidate()
            .map_err(|_| ChannelError::Disconnected)?;
        pending
            .accept(reply, self.generation, monotonic_us()?)
            .map_err(ChannelError::Reply)
    }

    pub(crate) fn peer_pid(&self) -> u32 {
        self.peer.pid()
    }
}

pub(crate) fn monotonic_us() -> Result<u64, ChannelError> {
    let mut time = std::mem::MaybeUninit::<libc::timespec>::uninit();
    // Same Linux CLOCK_MONOTONIC domain as the target GLib implementation.
    // https://man7.org/linux/man-pages/man2/clock_gettime.2.html
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, time.as_mut_ptr()) } != 0 {
        return Err(ChannelError::Unavailable);
    }
    let time = unsafe { time.assume_init() };
    let seconds = u64::try_from(time.tv_sec).map_err(|_| ChannelError::Unavailable)?;
    let nanos = u64::try_from(time.tv_nsec).map_err(|_| ChannelError::Unavailable)?;
    if nanos >= 1_000_000_000 {
        return Err(ChannelError::Unavailable);
    }
    seconds
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_add(nanos / 1000))
        .ok_or(ChannelError::Unavailable)
}

fn check_deadline(deadline: u64) -> Result<u64, ChannelError> {
    deadline
        .checked_sub(monotonic_us()?)
        .filter(|remaining| *remaining > 0)
        .ok_or(ChannelError::Timeout)
}

fn wait(stream: &UnixStream, events: i16, deadline: u64) -> Result<(), ChannelError> {
    loop {
        let remaining = check_deadline(deadline)?;
        let mut event = libc::pollfd {
            fd: stream.as_raw_fd(),
            events,
            revents: 0,
        };
        let milliseconds = remaining.div_ceil(1000).min(i32::MAX as u64) as i32;
        // One socket only, and no deadline reset when interrupted or fragmented.
        let result = unsafe { libc::poll(&mut event, 1, milliseconds) };
        if result < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(ChannelError::Disconnected);
        }
        check_deadline(deadline)?;
        if event.revents & events != 0 {
            return Ok(());
        }
        if event.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            return Err(ChannelError::Disconnected);
        }
    }
}

fn retryable() -> bool {
    matches!(
        io::Error::last_os_error().kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

fn read_frame(stream: &mut UnixStream, deadline: u64) -> Result<Frame, ChannelError> {
    let mut bytes = [0; FRAME_BYTES];
    let mut received = 0;
    while received < bytes.len() {
        wait(stream, libc::POLLIN, deadline)?;
        let mut buffer = libc::iovec {
            iov_base: bytes[received..].as_mut_ptr().cast(),
            iov_len: bytes.len() - received,
        };
        let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
        message.msg_iov = &mut buffer;
        message.msg_iovlen = 1;
        // No ancillary buffer: Linux discards/closes supplied SCM_RIGHTS and
        // flags MSG_CTRUNC. Never install, duplicate or inspect a passed FD.
        // https://man7.org/linux/man-pages/man7/unix.7.html
        let count = unsafe {
            libc::recvmsg(
                stream.as_raw_fd(),
                &mut message,
                libc::MSG_DONTWAIT | libc::MSG_CMSG_CLOEXEC,
            )
        };
        if count < 0 {
            if retryable() {
                continue;
            }
            return Err(ChannelError::Disconnected);
        }
        if count == 0 {
            return Err(ChannelError::Disconnected);
        }
        if message.msg_flags & (libc::MSG_CTRUNC | libc::MSG_TRUNC) != 0 {
            return Err(ChannelError::Protocol);
        }
        received += count as usize;
    }
    check_deadline(deadline)?;
    Frame::decode(&bytes).map_err(|_| ChannelError::Protocol)
}

fn write_frame(stream: &mut UnixStream, frame: Frame, deadline: u64) -> Result<(), ChannelError> {
    let bytes = frame.encode();
    let mut written = 0;
    while written < bytes.len() {
        wait(stream, libc::POLLOUT, deadline)?;
        let count = unsafe {
            libc::send(
                stream.as_raw_fd(),
                bytes[written..].as_ptr().cast(),
                bytes.len() - written,
                libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
            )
        };
        if count < 0 {
            if retryable() {
                continue;
            }
            return Err(ChannelError::Disconnected);
        }
        if count == 0 {
            return Err(ChannelError::Disconnected);
        }
        written += count as usize;
    }
    check_deadline(deadline)?;
    Ok(())
}

fn ensure_idle(stream: &UnixStream, deadline: u64) -> Result<(), ChannelError> {
    let mut byte = 0_u8;
    loop {
        check_deadline(deadline)?;
        let count = unsafe {
            libc::recv(
                stream.as_raw_fd(),
                (&mut byte as *mut u8).cast(),
                1,
                libc::MSG_PEEK | libc::MSG_DONTWAIT,
            )
        };
        if count > 0 {
            return Err(ChannelError::Protocol);
        }
        if count == 0 {
            return Err(ChannelError::Disconnected);
        }
        match io::Error::last_os_error().kind() {
            io::ErrorKind::WouldBlock => return Ok(()),
            io::ErrorKind::Interrupted => continue,
            _ => return Err(ChannelError::Disconnected),
        }
    }
}

#[cfg(test)]
#[path = "gnome_channel_tests.rs"]
mod tests;
