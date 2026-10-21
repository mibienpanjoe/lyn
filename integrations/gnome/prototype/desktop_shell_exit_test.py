"""Private FIFO control only; no terminal or desktop interaction."""

import os
from pathlib import Path
import tempfile
import unittest

from desktop_shell_exit import release_shell


class ShellExitTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="lyn-gnome-run-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.runtime = self.root / "runtime"
        self.runtime.mkdir(mode=0o700)
        self.fifo = self.runtime / "shell-exit.fifo"

    def test_writes_one_fixed_byte_only_to_private_fifo(self):
        os.mkfifo(self.fifo, 0o600)
        reader = os.open(self.fifo, os.O_RDONLY | os.O_NONBLOCK)
        self.addCleanup(os.close, reader)
        release_shell(self.runtime)
        self.assertEqual(os.read(reader, 2), b"\n")

    def test_missing_reader_never_blocks_or_creates_another_file(self):
        os.mkfifo(self.fifo, 0o600)
        with self.assertRaises(OSError):
            release_shell(self.runtime)
        self.assertTrue(self.fifo.is_fifo())

    def test_public_directory_and_fifo_refuse_before_write(self):
        os.mkfifo(self.fifo, 0o600)
        reader = os.open(self.fifo, os.O_RDONLY | os.O_NONBLOCK)
        self.addCleanup(os.close, reader)
        for path in (self.root, self.runtime, self.fifo):
            original = path.stat().st_mode & 0o777
            path.chmod(original | 0o004)
            with self.subTest(path=path), self.assertRaises(AssertionError):
                release_shell(self.runtime)
            path.chmod(original)
        self.assertEqual(os.read(reader, 2), b"")

    def test_regular_file_and_symlink_refuse_without_modifying_target(self):
        self.fifo.touch(mode=0o600)
        with self.assertRaises(AssertionError):
            release_shell(self.runtime)

        self.assertEqual(self.fifo.read_bytes(), b"")
        self.fifo.unlink()
        target = self.runtime / "other-fifo"
        os.mkfifo(target, 0o600)
        self.fifo.symlink_to(target)
        with self.assertRaises(AssertionError):
            release_shell(self.runtime)

    def test_symlinked_runtime_refuses_before_opening_fifo(self):
        other = self.root / "private-runtime"
        self.runtime.rename(other)
        self.runtime.symlink_to(other, target_is_directory=True)
        with self.assertRaises(OSError):
            release_shell(self.runtime)


if __name__ == "__main__":
    unittest.main()
