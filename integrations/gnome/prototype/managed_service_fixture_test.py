"""Address grammar only; never creates a bus, display or service."""

from pathlib import Path
import unittest

from managed_service_fixture import private_bus_address


class PrivateBusTests(unittest.TestCase):
    def test_only_exact_path_and_optional_guid_are_permitted(self):
        runtime = Path("/tmp/lyn-gnome-run-test/runtime")
        address = f"unix:path={runtime}/session-bus"
        self.assertTrue(private_bus_address(address, runtime))
        self.assertTrue(private_bus_address(address + ",guid=" + "a" * 32, runtime))
        for suffix in ("-other", ",path=/run/user/1000/bus", ",abstract=foreign", ",guid=bad",
                       ",guid=" + "a" * 32 + ",path=/run/user/1000/bus", ";unix:path=/other", "/../other"):
            self.assertFalse(private_bus_address(address + suffix, runtime), suffix)
        encoded = Path("/tmp/lyn-gnome-run-%2f..%2f/runtime")
        self.assertFalse(private_bus_address(f"unix:path={encoded}/session-bus", encoded))


if __name__ == "__main__":
    unittest.main()
