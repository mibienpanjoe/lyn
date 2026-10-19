"""Metadata-only admission of the private fixture's X server, before GTK/WM."""

import os
from pathlib import Path
import re
import select
import socket
import struct


def expected_xvfb(sdk):
    candidate = sdk / "root/usr/bin/Xvfb" if sdk else Path("/usr/bin/Xvfb")
    return candidate if candidate.is_file() else Path("/usr/bin/Xvfb")


def require_owned_display(sdk):
    display = re.fullmatch(r":([0-9]{1,5})(?:\.0)?", os.environ.get("DISPLAY", ""))
    assert display, "Only the private local Xvfb display is permitted"
    # No X11 setup/authentication or display mutation: inspect the Unix peer.
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(1)
        connection.connect(f"/tmp/.X11-unix/X{int(display[1])}")
        pid, uid, _ = struct.unpack("3i", connection.getsockopt(
            socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize("3i")))
        assert pid > 0 and uid == os.getuid(), "X server is not fixture-owned"
        pin = os.pidfd_open(pid)
        try:
            poller = select.poll()
            poller.register(pin, select.POLLIN)
            assert not poller.poll(0), "X server exited"
            process = Path(f"/proc/{pid}")
            before = (process / "stat").read_text()
            fields = before.rsplit(")", 1)[1].split()
            assert int(fields[2]) == os.getpgrp() and int(fields[3]) == os.getsid(0), \
                "X server is outside the private fixture group/session"
            assert (process / "exe").samefile(expected_xvfb(sdk)), "Not the fixture Xvfb executable"
            after = (process / "stat").read_text().rsplit(")", 1)[1].split()
            assert fields[2:4] == after[2:4] and fields[19] == after[19]
            assert not poller.poll(0), "X server exited during admission"
        finally:
            os.close(pin)
