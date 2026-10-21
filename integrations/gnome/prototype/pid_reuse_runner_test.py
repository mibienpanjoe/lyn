"""Completion witnesses cannot turn skipped/zero-test runs into evidence."""
import unittest
from pid_reuse_runner import completed


class WitnessTest(unittest.TestCase):
    def test_requires_one_executed_test_and_its_fresh_nonce(self):
        output = "\nLYN_PID_REUSE_PROVED:nonce\ntest result: ok. 1 passed; 0 failed; 0 ignored;\n"
        self.assertTrue(completed(output, "nonce"))
        for refused in ("", output.replace("1 passed", "0 passed"),
                        output.replace("nonce", "old"), output.replace("0 failed", "1 failed"),
                        output + "LYN_PID_REUSE_PROVED:nonce\n"):
            self.assertFalse(completed(refused, "nonce"))


if __name__ == "__main__":
    unittest.main()
