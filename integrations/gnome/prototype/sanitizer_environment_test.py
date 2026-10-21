"""Caller sanitizer settings cannot turn fixture failures into success."""
import unittest

from sanitizer_environment import sanitizer_environment


class SanitizerEnvironmentTest(unittest.TestCase):
    def test_inherited_suppression_and_disabled_detection_cannot_weaken_checks(self):
        original = {"LSAN_OPTIONS": "suppressions=/tmp/not-allowed:exitcode=0",
                    "ASAN_OPTIONS": "detect_leaks=0", "UBSAN_OPTIONS": "halt_on_error=0"}
        result = sanitizer_environment(original)
        keys = {option.split("=", 1)[0] for option in result["LSAN_OPTIONS"].split(":")}
        self.assertNotIn("suppressions", keys)
        self.assertIn("exitcode=23", result["LSAN_OPTIONS"])
        self.assertIn("detect_leaks=1", result["ASAN_OPTIONS"])
        self.assertIn("halt_on_error=1", result["UBSAN_OPTIONS"])
        self.assertEqual(original["ASAN_OPTIONS"], "detect_leaks=0")


if __name__ == "__main__":
    unittest.main()
