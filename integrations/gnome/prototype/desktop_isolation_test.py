"""Refuse an unowned display before the desktop fixture launches a WM."""

import os
from pathlib import Path
import socket
import struct
import sys
import tempfile
import unittest
from unittest.mock import MagicMock, patch

from desktop_isolation import expected_xvfb, require_owned_display


class DisplayIsolationTest(unittest.TestCase):
    def verify(self, *, display=":93", uid=None, executable=None, group=None,
               session=None, dead=False):
        connection = MagicMock()
        connection.__enter__.return_value = connection
        connection.getsockopt.return_value = struct.pack("3i", os.getpid(),
                                                        os.getuid() if uid is None else uid, os.getgid())
        poller = MagicMock()
        poller.poll.return_value = [(7, 1)] if dead else []
        # Real /proc reads and pidfd: only the expected fixture executable and
        # group/session are replaced. No real X server or display is contacted.
        with patch.dict(os.environ, {"DISPLAY": display}), \
                patch("desktop_isolation.socket.socket", return_value=connection), \
                patch("desktop_isolation.select.poll", return_value=poller), \
                patch("desktop_isolation.expected_xvfb", return_value=Path(executable or sys.executable)), \
                patch("desktop_isolation.os.getpgrp", return_value=os.getpgrp() if group is None else group), \
                patch("desktop_isolation.os.getsid", return_value=os.getsid(0) if session is None else session):
            require_owned_display(None)
        return connection

    def test_owned_live_server(self):
        self.verify().connect.assert_called_once_with("/tmp/.X11-unix/X93")

    def test_remote_or_malformed_display(self):
        for display in ("localhost:93", "host:0", ":93/../../0", ":93.1", ""):
            with self.subTest(display=display), self.assertRaises(AssertionError):
                self.verify(display=display)

    def test_foreign_uid(self):
        with self.assertRaises(AssertionError):
            self.verify(uid=os.getuid() + 1)

    def test_wrong_executable(self):
        with self.assertRaises(AssertionError):
            self.verify(executable="/usr/bin/true")

    def test_foreign_group_or_session(self):
        for mismatch in ({"group": os.getpgrp() + 1}, {"session": os.getsid(0) + 1}):
            with self.subTest(mismatch=mismatch), self.assertRaises(AssertionError):
                self.verify(**mismatch)

    def test_dead_server(self):
        with self.assertRaises(AssertionError):
            self.verify(dead=True)

    def test_header_only_sdk_uses_system_xvfb(self):
        with tempfile.TemporaryDirectory(prefix="lyn-display-sdk-") as directory:
            self.assertEqual(expected_xvfb(Path(directory)), Path("/usr/bin/Xvfb"))

    def test_pidfd_unavailable_has_no_fallback(self):
        with patch("desktop_isolation.os.pidfd_open", side_effect=PermissionError), \
                self.assertRaises(PermissionError):
            self.verify()

    def test_changed_starttime_is_refused(self):
        before = Path(f"/proc/{os.getpid()}/stat").read_text()
        prefix, suffix = before.rsplit(")", 1)
        fields = suffix.split()
        fields[19] = str(int(fields[19]) + 1)
        after = prefix + ") " + " ".join(fields)
        with patch("desktop_isolation.Path.read_text", side_effect=[before, after]), \
                self.assertRaises(AssertionError):
            self.verify()


if __name__ == "__main__":
    unittest.main()
