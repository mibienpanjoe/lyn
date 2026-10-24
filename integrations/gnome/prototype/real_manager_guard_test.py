"""No manager/cgroup writes: guard and owned-child cleanup regressions."""

import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from real_manager_runtime_test import parse_group, remove_children


class Guards(unittest.TestCase):
    def test_unified_group_requires_one_canonical_record(self):
        self.assertEqual(parse_group("0::/\n"), "/")
        self.assertEqual(parse_group("0::/user.slice/owned\n"), "/user.slice/owned")
        for value in ("", "0::/", "1::/x\n", "0::relative\n", "0::/x\n0::/y\n",
                      "0::/x/../y\n", "0::/x//y\n", "0::/x/\n", "0::/./x\n"):
            with self.subTest(value=value), self.assertRaises(RuntimeError):
                parse_group(value)

    def fixture(self):
        directory = tempfile.TemporaryDirectory(prefix="lyn-manager-guard-")
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        fd = self.pin(root)
        return root, fd

    def pin(self, root):
        fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
        self.addCleanup(os.close, fd)
        return fd

    def test_removes_only_pinned_known_empty_children(self):
        root, fd = self.fixture()
        (root / "init.scope").mkdir()
        pin = self.pin(root / "init.scope")
        remove_children(fd, {"init.scope": pin})
        self.assertFalse((root / "init.scope").exists())

    def test_unknown_child_refuses_before_any_removal(self):
        root, fd = self.fixture()
        (root / "init.scope").mkdir()
        (root / "unknown").mkdir()
        pin = self.pin(root / "init.scope")
        with self.assertRaisesRegex(RuntimeError, "unknown"):
            remove_children(fd, {"init.scope": pin})
        self.assertTrue((root / "init.scope").is_dir())
        self.assertTrue((root / "unknown").is_dir())

    def test_known_but_unrecorded_child_refuses(self):
        root, fd = self.fixture()
        (root / "app.slice").mkdir()
        with self.assertRaisesRegex(RuntimeError, "unrecorded"):
            remove_children(fd, {})
        self.assertTrue((root / "app.slice").is_dir())

    def test_replaced_child_refuses(self):
        root, fd = self.fixture()
        child = root / "init.scope"
        child.mkdir()
        pin = self.pin(child)
        child.rmdir()
        child.mkdir()
        with self.assertRaisesRegex(RuntimeError, "replaced"):
            remove_children(fd, {"init.scope": pin})
        self.assertTrue(child.is_dir())

    def test_symlink_is_preserved_without_following(self):
        root, fd = self.fixture()
        outside, _ = self.fixture()
        (outside / "preserved").write_text("untouched")
        (root / "init.scope").symlink_to(outside, target_is_directory=True)
        remove_children(fd, {})
        self.assertEqual((outside / "preserved").read_text(), "untouched")
        self.assertTrue((root / "init.scope").is_symlink())

    def test_nonempty_child_is_not_recursively_removed(self):
        root, fd = self.fixture()
        (root / "app.slice").mkdir()
        (root / "app.slice/preserved").write_text("untouched")
        pin = self.pin(root / "app.slice")
        with self.assertRaises(OSError):
            remove_children(fd, {"app.slice": pin})
        self.assertEqual((root / "app.slice/preserved").read_text(), "untouched")

    def test_excess_inventory_refuses_without_removal(self):
        _, fd = self.fixture()
        with patch("real_manager_runtime_test.os.listdir", return_value=["entry"] * 129), \
             patch("real_manager_runtime_test.os.rmdir") as remove:
            with self.assertRaisesRegex(RuntimeError, "too many"):
                remove_children(fd, {})
            remove.assert_not_called()


if __name__ == "__main__":
    unittest.main()
