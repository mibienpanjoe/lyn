"""Synthetic foreground process: one cwd change, no terminal input or output."""

import os
from pathlib import Path
import select
import signal
import stat
import sys
import time


def destination(target, runtime, current):
    target, runtime, current = map(Path, (target, runtime, current))
    root = runtime.parent
    assert root.is_absolute() and root.name.startswith("lyn-gnome-run-")
    for path in (root, runtime):
        metadata = path.lstat()
        assert stat.S_ISDIR(metadata.st_mode) and metadata.st_uid == os.getuid()
        assert metadata.st_mode & 0o077 == 0 and path.resolve(strict=True) == path
    assert current.name == "alpha" and target.name == "beta"
    for path in (current, target):
        assert path.is_absolute() and path.parent == root
        metadata = path.lstat()
        assert stat.S_ISDIR(metadata.st_mode) and metadata.st_uid == os.getuid()
        assert path.resolve(strict=True) == path
    return target


def change_cwd(pid, parent, original, target, tty_device):
    destination(target, Path(original).parent / "runtime", original)
    process = Path(f"/proc/{pid}")
    pin = os.pidfd_open(pid)
    try:
        poller = select.poll()
        poller.register(pin, select.POLLIN)
        def fields():
            return (process / "stat").read_text().rsplit(")", 1)[1].split()
        before = fields()
        assert process.stat().st_uid == os.getuid()
        assert (process / "exe").samefile("/usr/bin/python3")
        assert int(before[1]) == parent and int(before[2]) == pid
        assert int(before[4]) == tty_device
        # Unit fixtures have no TTY; the desktop caller requires real devpts.
        assert not tty_device or int(before[5]) == pid
        assert (process / "cwd").resolve(strict=True) == Path(original)
        deadline = time.monotonic() + 2
        while True:
            assert not poller.poll(0), "Fixture process exited before signalling"
            caught = next(line.split()[1] for line in (process / "status").read_text().splitlines()
                          if line.startswith("SigCgt:"))
            if int(caught, 16) & (1 << (signal.SIGUSR1 - 1)):
                break
            assert time.monotonic() < deadline, "Fixture signal handler did not become ready"
            time.sleep(.005)
        after = fields()
        assert all(before[index] == after[index] for index in (1, 2, 3, 4, 5, 19))
        assert not poller.poll(0) and time.monotonic() < deadline
        signal.pidfd_send_signal(pin, signal.SIGUSR1)
        while (process / "cwd").resolve(strict=True) != Path(target):
            assert not poller.poll(0) and time.monotonic() < deadline, "Fixture cwd did not change"
            time.sleep(.005)
        assert fields()[19] == before[19] and not poller.poll(0) and time.monotonic() < deadline
    finally:
        os.close(pin)


def run(target):
    target = destination(target, os.environ["XDG_RUNTIME_DIR"], Path.cwd())
    def move(_signal, _frame):
        os.chdir(target)
        signal.signal(signal.SIGUSR1, signal.SIG_IGN)
    signal.signal(signal.SIGUSR1, move)
    # Bound this fixture even though VTE puts its child in a separate session.
    signal.alarm(45)
    while True:
        signal.pause()


if __name__ == "__main__":
    run(sys.argv[1])
