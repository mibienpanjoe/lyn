import inspect
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


class Boss:
    active_window = Window()


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

    def test_on_load_matches_kitty_global_watcher_contract(self):
        self.assertEqual(
            list(inspect.signature(watcher.on_load).parameters),
            ["boss", "_data"],
        )

    def test_on_load_seeds_the_already_focused_kitty_pane(self):
        with (
            patch.object(watcher, "send_message") as send,
            patch.object(watcher.threading, "Thread") as thread,
        ):
            watcher.on_load(Boss(), {})

        self.assertEqual(
            send.call_args.args[0], watcher.create_message(77, 4242, "focused")
        )
        thread.assert_called_once()

    def test_os_focus_loss_keeps_the_exact_kitty_pane_available_for_capture(self):
        with patch.object(watcher, "send_message") as send:
            watcher.on_focus_change(None, Window(), {"focused": True})
            watcher.on_focus_change(None, Window(), {"focused": False})

        self.assertEqual(send.call_args_list[0].args[0]["state"], "focused")
        self.assertEqual(send.call_args_list[0].args[0]["terminalSessionId"], 77)
        self.assertEqual(send.call_count, 1)

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
