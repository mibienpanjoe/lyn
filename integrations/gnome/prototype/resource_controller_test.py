"""Owned-child cleanup must remain bounded even before metadata admission."""

import signal
import subprocess
import unittest
from unittest.mock import Mock, patch

from resource_test import stop_server


class CleanupTest(unittest.TestCase):
    def test_before_admission_stalled_owned_child_is_killed_and_reaped(self):
        server = Mock()
        server.poll.return_value = None
        server.wait.side_effect = [subprocess.TimeoutExpired("fixture", 3), 0]
        stop_server(server, None)
        server.terminate.assert_called_once()
        server.kill.assert_called_once()
        self.assertEqual(server.wait.call_count, 2)

    def test_pinned_child_exit_race_is_still_reaped_and_pin_closed(self):
        server = Mock()
        server.poll.return_value = None
        with patch("resource_test.signal.pidfd_send_signal", side_effect=ProcessLookupError) as send, \
                patch("resource_test.os.close") as close:
            stop_server(server, 42)
        send.assert_called_once_with(42, signal.SIGTERM)
        server.wait.assert_called_once_with(timeout=3)
        close.assert_called_once_with(42)
        server.terminate.assert_not_called()
        server.kill.assert_not_called()

    def test_pin_closed_after_final_wait_failure(self):
        server = Mock()
        server.poll.return_value = None
        server.wait.side_effect = subprocess.TimeoutExpired("fixture", 3)
        with patch("resource_test.signal.pidfd_send_signal"), patch("resource_test.os.close") as close:
            with self.assertRaises(subprocess.TimeoutExpired):
                stop_server(server, 42)
        close.assert_called_once_with(42)


if __name__ == "__main__":
    unittest.main()
