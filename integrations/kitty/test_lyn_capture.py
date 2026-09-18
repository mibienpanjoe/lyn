import unittest
from unittest.mock import patch

import lyn_capture
from test_lyn_context_watcher import Boss, Window


class CaptureKittenTests(unittest.TestCase):
    def test_handle_result_has_no_overlay(self):
        self.assertTrue(lyn_capture.handle_result.no_ui)

    def test_handle_result_invokes_the_target_pane_not_a_global_last_pane(self):
        boss = Boss()
        boss.window_id_map = {11: Window()}

        with patch.object(lyn_capture, "send_message") as send:
            lyn_capture.handle_result([], "", 11, boss)

        sent = send.call_args.args[0]
        self.assertEqual(sent["kind"], "invoke")
        self.assertEqual(sent["terminalSessionId"], 77)
        self.assertEqual(sent["processId"], 4242)
        self.assertNotIn("cwd", sent)
        self.assertNotIn("windowId", sent)


if __name__ == "__main__":
    unittest.main()
