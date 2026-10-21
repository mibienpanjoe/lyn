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

from desktop_lifecycle import kill_owned_app, pause_owned_app, read_generation, active_window_is


class LifecycleTest(unittest.TestCase):
    def test_focus_absence_or_other_window_never_counts_as_restored(self):
        self.assertTrue(active_window_is("_NET_ACTIVE_WINDOW(WINDOW): window id # 0x2a", 42))
        for value in ("_NET_ACTIVE_WINDOW:  not found.", "", "_NET_ACTIVE_WINDOW(WINDOW): window id # 0x2b",
                      "_NET_ACTIVE_WINDOW(CARDINAL) = 42", "_NET_ACTIVE_WINDOW(WINDOW): window id # 0x2a, 0x2b"):
            self.assertFalse(active_window_is(value, 42))

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="lyn-gnome-run-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.runtime = self.root / "runtime"
        self.runtime.mkdir(mode=0o700)

    @contextmanager
    def child(self, cwd=None):
        child = subprocess.Popen([sys.executable, "-c",
                                  "import signal,time; signal.alarm(5); time.sleep(30)"], cwd=cwd)
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

    def test_pause_is_observed_and_resumed_after_success_or_exception(self):
        for fail in (False, True):
            with self.subTest(fail=fail), self.child() as child:
                try:
                    with pause_owned_app(child, Path(sys.executable), self.runtime) as pin:
                        self.assertEqual(Path(f"/proc/{child.pid}/stat").read_text().rsplit(")", 1)[1].split()[0], "T")
                        self.assertIsInstance(pin, int)
                        if fail:
                            raise ValueError("synthetic controller failure")
                except ValueError:
                    self.assertTrue(fail)
                import time
                deadline = time.monotonic() + 1
                while Path(f"/proc/{child.pid}/stat").read_text().rsplit(")", 1)[1].split()[0] == "T":
                    self.assertLess(time.monotonic(), deadline, "Owned child remained stopped")
                    time.sleep(.005)
                self.assertIsNone(child.poll())

    def test_pause_refuses_wrong_executable_or_public_runtime_without_signalling(self):
        with self.child() as child, patch("desktop_lifecycle.signal.pidfd_send_signal") as send:
            with self.assertRaises(AssertionError):
                with pause_owned_app(child, Path("/usr/bin/true"), self.runtime):
                    self.fail("Wrong executable admitted")
            self.runtime.chmod(0o755)
            with self.assertRaises(AssertionError):
                with pause_owned_app(child, Path(sys.executable), self.runtime):
                    self.fail("Public runtime admitted")
            send.assert_not_called()
            self.assertIsNone(child.poll())

    def test_nested_pause_refuses_without_resuming_the_existing_stop(self):
        with self.child() as child:
            with pause_owned_app(child, Path(sys.executable), self.runtime):
                with self.assertRaises(AssertionError):
                    with pause_owned_app(child, Path(sys.executable), self.runtime):
                        self.fail("Nested pause admitted")
                self.assertEqual(Path(f"/proc/{child.pid}/stat").read_text().rsplit(")", 1)[1].split()[0], "T")

    def test_interruption_after_stop_delivery_still_resumes_owned_child(self):
        real_send = signal.pidfd_send_signal
        def interrupted_send(pin, operation):
            real_send(pin, operation)
            if operation == signal.SIGSTOP:
                raise KeyboardInterrupt("after delivered SIGSTOP")
        with self.child() as child:
            with patch("desktop_lifecycle.signal.pidfd_send_signal", side_effect=interrupted_send):
                with self.assertRaises(KeyboardInterrupt):
                    with pause_owned_app(child, Path(sys.executable), self.runtime):
                        self.fail("Interrupted pause yielded")
            import time
            deadline = time.monotonic() + .25
            while Path(f"/proc/{child.pid}/stat").read_text().rsplit(")", 1)[1].split()[0] == "T":
                self.assertLess(time.monotonic(), deadline, "Interrupted pause left the child stopped")
                time.sleep(.005)

    def test_late_stopped_observation_cannot_bypass_the_deadline(self):
        real_send = signal.pidfd_send_signal
        with self.child() as child:
            def settled_send(pin, operation):
                real_send(pin, operation)
                if operation == signal.SIGSTOP:
                    import time
                    for _ in range(100):
                        if Path(f"/proc/{child.pid}/stat").read_text().rsplit(")", 1)[1].split()[0] == "T":
                            return
                        time.sleep(.001)
                    self.fail("Own child did not stop")
            with patch("desktop_lifecycle.signal.pidfd_send_signal", side_effect=settled_send), \
                    patch("desktop_lifecycle.time.monotonic", side_effect=[0, 2]):
                with self.assertRaisesRegex(AssertionError, "deadline"):
                    with pause_owned_app(child, Path(sys.executable), self.runtime):
                        self.fail("Late T observation admitted")

    def test_wrong_executable_and_parent_refuse_without_signalling(self):
        with self.child() as child:
            with self.assertRaises(AssertionError):
                kill_owned_app(child, Path("/usr/bin/true"), self.runtime)
            parent = os.getpid()
            with patch("desktop_lifecycle.os.getpid", return_value=parent + 1):
                with self.assertRaises(AssertionError):
                    kill_owned_app(child, Path(sys.executable), self.runtime)
            self.assertIsNone(child.poll())

    def test_explicit_fixture_cwd_is_required_and_checked(self):
        with self.child(cwd=self.root) as child:
            with self.assertRaises(AssertionError):
                kill_owned_app(child, Path(sys.executable), self.runtime)
            with self.assertRaises(AssertionError):
                kill_owned_app(child, Path(sys.executable), self.runtime, expected_cwd=self.runtime)
            self.assertIsNone(child.poll())
            pin = kill_owned_app(child, Path(sys.executable), self.runtime, expected_cwd=self.root)
            os.close(pin)
            self.assertEqual(child.returncode, -signal.SIGKILL)

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
