"""Private process/control guards, not desktop or GNOME acceptance."""

from contextlib import contextmanager
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from desktop_lifecycle import kill_owned_app, read_generation


class LifecycleTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="lyn-gnome-run-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.runtime = self.root / "runtime"
        self.runtime.mkdir(mode=0o700)

    @contextmanager
    def child(self):
        child = subprocess.Popen([sys.executable, "-c",
                                  "import signal,time; signal.alarm(5); time.sleep(30)"])
        pin = os.pidfd_open(child.pid)
        try:
            yield child
        finally:
            if child.poll() is None:
                signal.pidfd_send_signal(pin, signal.SIGKILL)
                child.wait(timeout=3)
            os.close(pin)

    def test_kills_only_owned_pinned_process_and_observes_exit(self):
        with self.child() as child:
            pin = kill_owned_app(child, Path(sys.executable), self.runtime)
            try:
                self.assertEqual(child.returncode, -signal.SIGKILL)
            finally:
                os.close(pin)

    def test_wrong_executable_and_parent_refuse_without_signalling(self):
        with self.child() as child:
            with self.assertRaises(AssertionError):
                kill_owned_app(child, Path("/usr/bin/true"), self.runtime)
            parent = os.getpid()
            with patch("desktop_lifecycle.os.getpid", return_value=parent + 1):
                with self.assertRaises(AssertionError):
                    kill_owned_app(child, Path(sys.executable), self.runtime)
            self.assertIsNone(child.poll())

    def test_public_and_symlinked_runtime_refuse_without_signalling(self):
        with self.child() as child:
            self.runtime.chmod(0o755)
            with self.assertRaises(AssertionError):
                kill_owned_app(child, Path(sys.executable), self.runtime)
            self.runtime.chmod(0o700)
            real_runtime = self.root / "real-runtime"
            self.runtime.rename(real_runtime)
            self.runtime.symlink_to(real_runtime, target_is_directory=True)
            with self.assertRaises(AssertionError):
                kill_owned_app(child, Path(sys.executable), self.runtime)
            self.assertIsNone(child.poll())

    def test_generation_is_fixed_read_only_request_and_exact_acknowledgement(self):
        with self.socket_pair() as pair:
            controller, peer = pair
            generation = bytes(range(1, 17))
            peer.sendall(b"G" + generation)
            self.assertEqual(read_generation(controller), generation)
            self.assertEqual(peer.recv(2), b"G")

    @contextmanager
    def socket_pair(self):
        controller, peer = socket.socketpair()
        try:
            yield controller, peer
        finally:
            controller.close()
            peer.close()

    def test_generation_rejects_wrong_tag_zero_uuid_and_eof(self):
        for response in (b"N" + bytes(range(16)), b"G" + bytes(16), b""):
            with self.subTest(response=response), self.socket_pair() as pair:
                controller, peer = pair
                peer.sendall(response)
                peer.shutdown(socket.SHUT_WR)
                with self.assertRaises(AssertionError):
                    read_generation(controller)


if __name__ == "__main__":
    unittest.main()
