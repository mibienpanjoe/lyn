import unittest
from unittest.mock import patch

import lyn_context_watcher as watcher


class Child:
    pid = 4242


class Window:
    id = 77
    child = Child()


class OtherChild:
    pid = 4343


class OtherWindow:
    id = 88
    child = OtherChild()


class KittyWatcherTests(unittest.TestCase):
    def setUp(self):
        watcher.reset_for_test()

    def test_message_contains_only_bounded_correlation_fields(self):
        message = watcher.create_message(77, 4242, "focused")

        self.assertEqual(
            set(message),
            {"version", "terminalSessionId", "processId", "state"},
        )
        self.assertNotIn("cwd", message)
        self.assertNotIn("title", message)
        self.assertNotIn("command", message)
        self.assertNotIn("environment", message)

    def test_focus_changes_report_and_revoke_the_exact_kitty_pane(self):
        with patch.object(watcher, "send_message") as send:
            watcher.on_focus_change(None, Window(), {"focused": True})
            watcher.on_focus_change(None, Window(), {"focused": False})

        self.assertEqual(send.call_args_list[0].args[0]["state"], "focused")
        self.assertEqual(send.call_args_list[1].args[0]["state"], "ended")
        self.assertEqual(send.call_args_list[0].args[0]["terminalSessionId"], 77)

    def test_close_revokes_a_previously_focused_pane(self):
        with patch.object(watcher, "send_message") as send:
            watcher.on_focus_change(None, Window(), {"focused": True})
            watcher.on_close(None, Window(), {})

        self.assertEqual(send.call_args_list[-1].args[0]["state"], "ended")

    def test_new_focus_revokes_any_previous_global_kitty_focus(self):
        with patch.object(watcher, "send_message") as send:
            watcher.on_focus_change(None, Window(), {"focused": True})
            watcher.on_focus_change(None, OtherWindow(), {"focused": True})

        self.assertEqual(
            [(call.args[0]["terminalSessionId"], call.args[0]["state"]) for call in send.call_args_list],
            [(77, "focused"), (77, "ended"), (88, "focused")],
        )


if __name__ == "__main__":
    unittest.main()
