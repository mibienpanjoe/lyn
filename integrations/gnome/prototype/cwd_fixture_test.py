"""Private synthetic cwd changes and pidfd signalling, without a desktop."""

import os
from pathlib import Path
import signal
import subprocess
import tempfile
import unittest

from cwd_fixture import change_cwd, destination


class CwdFixtureTest(unittest.TestCase):
    def test_destination_must_be_private_beta_beside_alpha(self):
        with tempfile.TemporaryDirectory(prefix="lyn-gnome-run-") as directory:
            root = Path(directory)
            for name in ("runtime", "alpha", "beta"):
                (root / name).mkdir(mode=0o700)
            self.assertEqual(destination(root / "beta", root / "runtime", root / "alpha"), root / "beta")
            for target in (Path("beta"), root / "alpha", Path("/tmp")):
                with self.subTest(target=target), self.assertRaises(AssertionError):
                    destination(target, root / "runtime", root / "alpha")
            (root / "beta").rename(root / "beta-real")
            (root / "beta").symlink_to(root / "beta-real")
            with self.assertRaises(AssertionError):
                destination(root / "beta", root / "runtime", root / "alpha")

    def test_wrong_parent_or_tty_cannot_signal_and_real_child_keeps_its_lifetime(self):
        with tempfile.TemporaryDirectory(prefix="lyn-gnome-run-") as directory:
            root = Path(directory)
            for name in ("runtime", "alpha", "beta"):
                (root / name).mkdir(mode=0o700)
            child = subprocess.Popen(["/usr/bin/python3", str(Path(__file__).with_name("cwd_fixture.py")),
                                      str(root / "beta")], cwd=root / "alpha", start_new_session=True,
                                     env=dict(os.environ, XDG_RUNTIME_DIR=str(root / "runtime")))
            pin = os.pidfd_open(child.pid)
            try:
                before = Path(f"/proc/{child.pid}/stat").read_text().rsplit(")", 1)[1].split()[19]
                for parent, tty in ((os.getpid() + 1, 0), (os.getpid(), 1)):
                    with self.subTest(parent=parent, tty=tty), self.assertRaises(AssertionError):
                        change_cwd(child.pid, parent, root / "alpha", root / "beta", tty)
                    self.assertEqual(Path(f"/proc/{child.pid}/cwd").resolve(), root / "alpha")
                change_cwd(child.pid, os.getpid(), root / "alpha", root / "beta", 0)
                self.assertEqual(Path(f"/proc/{child.pid}/cwd").resolve(), root / "beta")
                self.assertEqual(Path(f"/proc/{child.pid}/stat").read_text().rsplit(")", 1)[1].split()[19], before)
                self.assertIsNone(child.poll())
            finally:
                if child.poll() is None:
                    signal.pidfd_send_signal(pin, signal.SIGTERM)
                child.wait(timeout=3)
                os.close(pin)

    def test_another_executable_is_never_signalled(self):
        with tempfile.TemporaryDirectory(prefix="lyn-gnome-run-") as directory:
            root = Path(directory)
            for name in ("runtime", "alpha", "beta"):
                (root / name).mkdir(mode=0o700)
            child = subprocess.Popen(["/usr/bin/sleep", "45"], cwd=root / "alpha", start_new_session=True)
            pin = os.pidfd_open(child.pid)
            try:
                with self.assertRaises(AssertionError):
                    change_cwd(child.pid, os.getpid(), root / "alpha", root / "beta", 0)
                self.assertIsNone(child.poll())
            finally:
                if child.poll() is None:
                    signal.pidfd_send_signal(pin, signal.SIGTERM)
                child.wait(timeout=3)
                os.close(pin)


if __name__ == "__main__":
    unittest.main()
