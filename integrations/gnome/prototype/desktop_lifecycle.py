"""Controller for the owned desktop fixture, never a production command."""

import os
from contextlib import contextmanager
from pathlib import Path
import re
import select
import signal
import stat
import time


def active_window_is(property_value, expected):
    """An absent/changing property is not restored focus; require exact XID."""
    active = re.fullmatch(r"_NET_ACTIVE_WINDOW\(WINDOW\): window id # (0x[0-9a-fA-F]+)", property_value)
    return active is not None and int(active[1], 16) == int(expected)


def read_generation(connection):
    """Observe one already-admitted generation; never force a reconnect."""
    deadline = time.monotonic() + 1
    connection.settimeout(1)
    connection.sendall(b"G")
    response = bytearray()
    while len(response) < 17:
        remaining = deadline - time.monotonic()
        assert remaining > 0, "Generation observation exceeded its deadline"
        connection.settimeout(remaining)
        part = connection.recv(17 - len(response))
        assert part, "Generation observation closed before its acknowledgement"
        response.extend(part)
    assert response[0] == ord("G") and any(response[1:]), "Invalid generation acknowledgement"
    return bytes(response[1:])


def stop_native_broker(connection):
    """One fixed test-only stop, acknowledged only after broker shutdown joins."""
    previous = connection.gettimeout()
    connection.settimeout(3)
    try:
        connection.sendall(b"B")
        assert connection.recv(1) == b"B", "Native broker stop was not acknowledged"
    finally:
        connection.settimeout(previous)


def _pin_owned_app(process, executable, runtime, *, expected_cwd=None):
    """Validate the private direct child before returning a live kernel pin."""
    runtime = Path(runtime)
    root = runtime.parent
    assert root.is_absolute() and root.name.startswith("lyn-gnome-run-")
    assert runtime.name == "runtime"
    for directory in (root, runtime):
        metadata = directory.lstat()
        assert stat.S_ISDIR(metadata.st_mode) and metadata.st_uid == os.geteuid()
        assert metadata.st_mode & 0o7777 == 0o700
        assert directory.resolve(strict=True) == directory
    assert process.poll() is None, "Fixture app is no longer running"
    pin = os.pidfd_open(process.pid)
    try:
        poller = select.poll()
        poller.register(pin, select.POLLIN)
        assert not poller.poll(0), "Fixture app already exited"
        target = Path(f"/proc/{process.pid}")
        def fields():
            return (target / "stat").read_text().rsplit(")", 1)[1].split()
        before = fields()
        assert target.stat().st_uid == os.geteuid()
        assert (target / "exe").samefile(executable)
        assert int(before[1]) == os.getpid(), "App is not this controller's child"
        assert int(before[2]) == os.getpgrp() and int(before[3]) == os.getsid(0)
        expected_cwd = Path.cwd() if expected_cwd is None else Path(expected_cwd)
        assert expected_cwd.is_absolute()
        assert (target / "cwd").resolve(strict=True) == expected_cwd.resolve(strict=True)
        after = fields()
        assert all(before[index] == after[index] for index in (1, 2, 3, 4, 19))
        assert target.stat().st_uid == os.geteuid() and (target / "exe").samefile(executable)
        assert not poller.poll(0), "Fixture app exited before signalling"
        return pin
    except BaseException:
        os.close(pin)
        raise


def kill_owned_app(process, executable, runtime, *, expected_cwd=None):
    """Return the retained pidfd after SIGKILL/wait, or refuse before signalling."""
    pin = _pin_owned_app(process, executable, runtime, expected_cwd=expected_cwd)
    try:
        signal.pidfd_send_signal(pin, signal.SIGKILL)
        assert process.wait(timeout=5) == -signal.SIGKILL, "App did not die from the requested SIGKILL"
        poller = select.poll()
        poller.register(pin, select.POLLIN)
        events = poller.poll(1000)
        assert len(events) == 1 and events[0][1] & select.POLLIN
        assert not events[0][1] & (select.POLLERR | select.POLLNVAL)
        return pin
    except BaseException:
        os.close(pin)
        raise


@contextmanager
def pause_owned_app(process, executable, runtime, *, expected_cwd=None):
    """Pause only the pinned private child, always resume it on controller exit."""
    pin = _pin_owned_app(process, executable, runtime, expected_cwd=expected_cwd)
    requested = False
    try:
        initial_state = Path(f"/proc/{process.pid}/stat").read_text().rsplit(")", 1)[1].split()[0]
        assert initial_state not in ("T", "t"), "Do not resume a child already stopped by another action"
        # Delivery may succeed just before Python raises an interruption.
        # Own the matching resume before crossing the signalling syscall.
        requested = True
        signal.pidfd_send_signal(pin, signal.SIGSTOP)
        poller = select.poll()
        poller.register(pin, select.POLLIN)
        deadline = time.monotonic() + 1
        while True:
            assert not poller.poll(0), "Private child exited before observed stop"
            state = Path(f"/proc/{process.pid}/stat").read_text().rsplit(")", 1)[1].split()[0]
            assert time.monotonic() < deadline, "Private child stop observation exceeded deadline"
            if state == "T":
                break
            time.sleep(.005)
        yield pin
    finally:
        try:
            if requested:
                try:
                    signal.pidfd_send_signal(pin, signal.SIGCONT)
                except ProcessLookupError:
                    pass
        finally:
            os.close(pin)
