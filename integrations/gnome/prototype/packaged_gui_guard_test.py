"""Refuse an incomplete filesystem isolation proof before any packaged GUI."""
import unittest
from packaged_gui_guard import validate


class GuardTests(unittest.TestCase):
    def test_all_required_evidence_is_mandatory(self):
        valid = dict(uid=1000, gid=1000, private_mount=True, private_pid=True,
                     private_network=True, root_readonly=True, home_replaced=True,
                     home_unchanged=True, checkout_hidden=True, home_empty=True,
                     tmp_replaced=True, run_replaced=True, varlib_replaced=True, writes_private=True)
        validate(valid)
        for key in valid:
            invalid = {**valid, key: 0 if key in ('uid', 'gid') else False}
            with self.assertRaises(RuntimeError, msg=key):
                validate(invalid)


if __name__ == '__main__':
    unittest.main()
