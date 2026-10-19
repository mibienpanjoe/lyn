"""Bounded address lookup only: no peer admission or capture acceptance."""

import ctypes
import os
from pathlib import Path
import socket
import stat
import sys
import tempfile
import unittest


library = ctypes.CDLL(sys.argv.pop(1))
discover = library.lyn_native_discover
discover.argtypes = [ctypes.c_char_p, ctypes.c_void_p]
discover.restype = ctypes.c_bool
CURRENT = "lyn-gnome-v1.current"
INSTANCE = "gn-" + "a" * 32


class DiscoveryTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="lyn-dis-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.instance = self.root / INSTANCE
        self.instance.mkdir(mode=0o700)
        self.endpoint = self.instance / "lyn-gnome-v1.sock"
        self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.addCleanup(self.listener.close)
        self.listener.bind(str(self.endpoint))
        self.endpoint.chmod(0o600)
        self.listener.listen(1)
        self.record = self.root / CURRENT
        self.record.write_bytes((INSTANCE + "\n").encode())
        self.record.chmod(0o600)

    def lookup(self, root=None):
        result = ctypes.create_string_buffer(b"unchanged", 108)
        before = result.raw
        found = discover(os.fsencode(self.root if root is None else root), result)
        if not found:
            self.assertEqual(result.raw, before, "failure rewrote output")
        return found, result.value

    def test_exact_private_record_and_final_dot_normalization(self):
        for root in (str(self.root), str(self.root) + "/", str(self.root) + "/./"):
            self.assertEqual(self.lookup(root), (True, os.fsencode(self.endpoint)))

    def test_missing_and_malformed_records(self):
        self.record.unlink()
        self.assertFalse(self.lookup()[0])
        for data in (b"", b"../escape\n", b"gn-" + b"A" * 32 + b"\n",
                     b"gn-" + b"a" * 31 + b"\0\n", (INSTANCE + "\nextra").encode(),
                     (INSTANCE + "\n\n").encode(), b"a" * 1000000):
            with self.subTest(length=len(data)):
                self.record.write_bytes(data)
                self.record.chmod(0o600)
                self.assertFalse(self.lookup()[0])

    def test_permissions_types_and_hardlinks(self):
        for path, mode in ((self.root, 0o755), (self.instance, 0o750),
                           (self.endpoint, 0o660), (self.record, 0o640),
                           (self.record, 0o4600)):
            with self.subTest(path=path.name, mode=mode):
                before = stat.S_IMODE(path.lstat().st_mode)
                path.chmod(mode)
                self.assertFalse(self.lookup()[0])
                path.chmod(before)
        link = self.root / "linked-record"
        os.link(self.record, link)
        self.assertFalse(self.lookup()[0])
        link.unlink()
        self.record.unlink()
        os.mkfifo(self.record, 0o600)
        self.assertFalse(self.lookup()[0], "FIFO lookup must not block")
        self.record.unlink()
        self.record.mkdir(mode=0o700)
        self.assertFalse(self.lookup()[0])

    def test_refuses_symlinks_at_every_discovery_boundary(self):
        alias = self.root / "runtime-alias"
        alias.symlink_to(self.root, target_is_directory=True)
        for root in (str(alias), str(alias) + "/", str(alias) + "/./"):
            self.assertFalse(self.lookup(root)[0])
        for path in (self.record, self.instance, self.endpoint):
            with self.subTest(path=path.name):
                saved = path.with_name(path.name + "-saved")
                path.rename(saved)
                path.symlink_to(saved, target_is_directory=saved.is_dir())
                self.assertFalse(self.lookup()[0])
                path.unlink()
                saved.rename(path)

    def test_missing_instance_socket_and_non_socket(self):
        self.endpoint.unlink()
        self.assertFalse(self.lookup()[0])
        self.endpoint.write_bytes(b"not a socket")
        self.endpoint.chmod(0o600)
        self.assertFalse(self.lookup()[0])
        self.endpoint.unlink()
        self.instance.rmdir()
        self.assertFalse(self.lookup()[0])

    def test_relative_overlong_and_null_runtime(self):
        for root in ("relative", "/" + "x" * 200, None):
            result = ctypes.create_string_buffer(b"unchanged", 108)
            before = result.raw
            self.assertFalse(discover(None if root is None else os.fsencode(root), result))
            self.assertEqual(result.raw, before)
        self.assertFalse(discover(os.fsencode(self.root), None))

    def test_repeated_reads_do_not_leak_and_do_not_delete_stale_socket(self):
        self.listener.close()
        before = self.endpoint.lstat()
        baseline = len(os.listdir("/proc/self/fd"))
        for _ in range(100):
            self.assertTrue(self.lookup()[0], "address hint is not a liveness claim")
        self.assertEqual(len(os.listdir("/proc/self/fd")), baseline)
        after = self.endpoint.lstat()
        self.assertEqual((before.st_dev, before.st_ino), (after.st_dev, after.st_ino))


if __name__ == "__main__":
    unittest.main()
