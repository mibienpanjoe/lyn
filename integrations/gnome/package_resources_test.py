"""Package resource extraction is checked before any private desktop is started."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from package_resources import recover_bundle


class ResourceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="lyn-package-resource-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.module = b"\x7fELF" + b"native-payload" * 8
        self.binary = b"prefix" + self.module + b"suffix"
        self.manifest = {"length": len(self.module), "sha256": hashlib.sha256(self.module).hexdigest()}
        self.receipt = {"binarySha256": hashlib.sha256(self.binary).hexdigest(),
                        "moduleOffset": 6, "moduleLength": len(self.module),
                        "moduleSha256": self.manifest["sha256"], "nativeManifest": self.manifest}

    def test_exact_resource_is_recovered_into_a_fresh_directory(self):
        directory = self.root / "bundle"
        recover_bundle(self.binary, self.receipt, "binarySha256", directory)
        self.assertEqual((directory / "liblyn-gnome-native.so").read_bytes(), self.module)
        self.assertEqual(json.loads((directory / "manifest.json").read_text()), self.manifest)
        with self.assertRaises(FileExistsError):
            recover_bundle(self.binary, self.receipt, "binarySha256", directory)

    def test_changed_package_or_resource_refuses_before_creating_output(self):
        changed = self.binary[:-1] + b"X"
        for binary, receipt in [(changed, self.receipt),
                                (self.binary, {**self.receipt, "moduleSha256": "f" * 64})]:
            destination = self.root / "refused"
            with self.assertRaises(ValueError):
                recover_bundle(binary, receipt, "binarySha256", destination)
            self.assertFalse(destination.exists())

    def test_malformed_bounds_and_mismatching_manifest_refuse(self):
        for fields in [{"moduleOffset": -1}, {"moduleOffset": True},
                       {"moduleLength": 1024 * 1024 + 1}, {"moduleLength": 0},
                       {"moduleOffset": len(self.binary)},
                       {"nativeManifest": {**self.manifest, "length": 1}}]:
            with self.assertRaises(ValueError):
                recover_bundle(self.binary, {**self.receipt, **fields}, "binarySha256", self.root / "refused")
            self.assertFalse((self.root / "refused").exists())


if __name__ == "__main__":
    unittest.main()
