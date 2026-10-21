"""Reject unbounded or desktop-inapplicable reconnect requests before startup."""
from pathlib import Path
import subprocess
import sys
import unittest


class ReconnectControllerTest(unittest.TestCase):
    def test_explicit_default_count_is_also_native_only(self):
        result = subprocess.run([sys.executable, str(Path(__file__).with_name("test.py")),
                                 "--desktop", "--reconnect-cycles=1", "--startup-pairs=0"],
                                capture_output=True, text=True, timeout=3)
        self.assertEqual(result.returncode, 2)
        self.assertIn("--reconnect-cycles requires native mode and a value from 1 to 20", result.stderr)

    def test_invalid_counts_are_refused_before_build_or_display(self):
        runner = Path(__file__).with_name("test.py")
        for arguments in (("--reconnect-cycles=0",), ("--reconnect-cycles=21",),
                          ("--desktop", "--reconnect-cycles=2")):
            result = subprocess.run([sys.executable, str(runner), *arguments],
                                    capture_output=True, text=True, timeout=3)
            self.assertEqual(result.returncode, 2)
            self.assertIn("--reconnect-cycles requires native mode and a value from 1 to 20",
                          result.stderr)


if __name__ == "__main__":
    unittest.main()
