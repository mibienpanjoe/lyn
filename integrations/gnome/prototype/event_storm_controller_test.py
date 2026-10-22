"""The storm oracle must reject wrong-source and late metadata responses."""
from pathlib import Path
import struct
import subprocess
import sys
import unittest

from event_storm_test import checked_reply, action_path, selection_observed


class StormControllerTest(unittest.TestCase):
    def setUp(self):
        vectors = Path(__file__).parent.parent / "wire_vectors.txt"
        self.frames = {name: bytes.fromhex(data) for name, data in
                       (line.split() for line in vectors.read_text().splitlines())}
        self.request = bytearray(self.frames["query"])
        struct.pack_into("!Q", self.request, 60, 200)
        self.sources = {17: {"sourceId": 17, "ptyNumber": 0, "foregroundPgid": 42,
                             "registeredAtUs": 100, "changedAtUs": 150, "revision": 3}}

    def test_valid_candidate_and_correlated_changed_are_distinct(self):
        self.assertEqual(checked_reply(self.frames["candidate"], self.request, self.sources, 300), 1)
        self.assertEqual(checked_reply(self.frames["changed"], self.request, self.sources, 300), 3)

    def test_wrong_identity_correlation_fields_and_times_are_refused(self):
        for offset, value in ((59, 18), (39, 99), (96, 1), (67, 199), (75, 201)):
            frame = bytearray(self.frames["candidate"])
            frame[offset] = value
            with self.assertRaises(AssertionError):
                checked_reply(frame, self.request, self.sources, 300)
        with self.assertRaises(AssertionError):
            checked_reply(self.frames["candidate"], self.request, self.sources, 120201)
        wrong = bytearray(self.frames["changed"])
        wrong[59] = 17
        with self.assertRaises(AssertionError):
            checked_reply(wrong, self.request, self.sources, 300)

    def test_action_path_is_confined_to_the_fixture_application(self):
        prop = '_GTK_WINDOW_OBJECT_PATH(UTF8_STRING) = "/org/lyn/NativeEventStorm/window/1"\n'
        self.assertEqual(action_path(prop), "/org/lyn/NativeEventStorm/window/1")
        for wrong in (prop.replace("NativeEventStorm", "Other"), prop + prop,
                      prop.replace("/1", "/1/child")):
            with self.assertRaises(AssertionError):
                action_path(wrong)

    def test_selection_requires_fresh_exact_sources_and_expected_active_tab(self):
        sources = [{"sourceId": 17, "active": False, "observedAtUs": 200},
                   {"sourceId": 18, "active": True, "observedAtUs": 200}]
        self.assertTrue(selection_observed(sources, {17, 18}, 18, 200))
        self.assertFalse(selection_observed(sources, {17, 18}, 17, 200))
        self.assertFalse(selection_observed(sources, {17, 18}, 18, 201))
        self.assertFalse(selection_observed(sources, {17, 19}, 18, 200))
        self.assertFalse(selection_observed(sources[:1], {17, 18}, 17, 200))

    def test_storm_mode_is_refused_with_desktop_before_startup(self):
        result = subprocess.run([sys.executable, str(Path(__file__).with_name("test.py")),
                                 "--desktop", "--event-storm", "--startup-pairs=0"],
                                capture_output=True, text=True, timeout=3)
        self.assertEqual(result.returncode, 2)
        self.assertIn("--event-storm requires native mode", result.stderr)


if __name__ == "__main__":
    unittest.main()
