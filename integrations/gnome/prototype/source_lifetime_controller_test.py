"""Keep comparison processes isolated and preserve sanitizer failure status."""
from pathlib import Path
import subprocess
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from source_lifetime_test import lifetime_environment, run


class LifetimeControllerTest(unittest.TestCase):
    def test_inherited_modules_preloads_and_sanitizer_overrides_are_removed(self):
        original = {"GTK_MODULES": "lyn:other", "GTK3_MODULES": "/tmp/not-ours.so",
                    "LD_PRELOAD": "/tmp/not-ours.so", "LD_AUDIT": "/tmp/not-ours.so",
                    "ASAN_OPTIONS": "detect_leaks=0", "LSAN_OPTIONS": "exitcode=0:suppressions=/tmp/no",
                    "DISPLAY": ":123"}
        environment = lifetime_environment(original)
        self.assertEqual(environment["GTK_MODULES"], "")
        self.assertEqual(environment["GTK3_MODULES"], "")
        self.assertNotIn("LD_PRELOAD", environment)
        self.assertNotIn("LD_AUDIT", environment)
        self.assertIn("detect_leaks=1", environment["ASAN_OPTIONS"])
        self.assertIn("exitcode=23", environment["LSAN_OPTIONS"])
        self.assertNotIn("suppressions=", environment["LSAN_OPTIONS"].replace("print_suppressions=1", ""))
        self.assertEqual(environment["DISPLAY"], ":123")
        self.assertEqual(original["GTK_MODULES"], "lyn:other")

    def test_child_status_is_retained_after_display_guard(self):
        for child_status, expected in ((0, 0), (23, 23), (-6, 134)):
            with patch("source_lifetime_test.require_owned_display") as guard, patch(
                    "source_lifetime_test.subprocess.run", return_value=SimpleNamespace(returncode=child_status)) as child:
                self.assertEqual(run(Path(sys.executable), None), expected)
                guard.assert_called_once_with(None)
                self.assertEqual(child.call_args.kwargs["timeout"], 45)
                self.assertFalse(child.call_args.kwargs["check"])
                self.assertEqual(child.call_args.kwargs["env"]["GTK_MODULES"], "")

    def test_desktop_mode_is_refused_before_startup(self):
        result = subprocess.run([sys.executable, str(Path(__file__).with_name("test.py")),
                                 "--desktop", "--source-lifetimes", "--startup-pairs=0"],
                                capture_output=True, text=True, timeout=3)
        self.assertEqual(result.returncode, 2)
        self.assertIn("--source-lifetimes requires native mode", result.stderr)


if __name__ == "__main__":
    unittest.main()
