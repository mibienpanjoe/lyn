"""Content verification of local Linux build outputs; no install or execution."""
import hashlib
import io
import json
import os
from unittest.mock import patch
from pathlib import Path
import tarfile
import tempfile
import unittest

from verify_linux_native import verify_binary, verify_archive, read_bundle, debian_binary


class NativePackageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="lyn-package-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.module = b"\x7fELF" + b"native-fixture" * 8
        self.bundle = self.root / "bundle"
        self.bundle.mkdir()
        (self.bundle / "liblyn-gnome-native.so").write_bytes(self.module)
        self.manifest = {"length": len(self.module), "sha256": hashlib.sha256(self.module).hexdigest()}
        (self.bundle / "manifest.json").write_text(json.dumps(self.manifest))
        self.binary = b"fixture-binary:" + self.module + b":end"

    def test_packaging_freezes_cargo_output_despite_inherited_overrides(self):
        from package_linux import build_environment, RELEASE, TARGET
        with patch.dict(os.environ, {"CARGO_TARGET_DIR": "/outside/old", "CARGO_BUILD_TARGET": "another-target"}):
            env = build_environment(self.bundle)
        self.assertEqual(Path(env["CARGO_TARGET_DIR"]) / TARGET / "release", RELEASE)
        self.assertEqual(env["CARGO_BUILD_TARGET"], TARGET)
        self.assertEqual(env["LYN_GNOME_NATIVE_BUNDLE"], str(self.bundle))

    def test_debian_allows_only_the_single_known_tauri_bundle_marker_change(self):
        unknown = b"__TAURI_BUNDLE_TYPE_VAR_UNK"
        deb = b"__TAURI_BUNDLE_TYPE_VAR_DEB"
        self.assertEqual(debian_binary(self.binary + unknown), self.binary + deb)
        for binary in [self.binary, self.binary + unknown + unknown]:
            with self.assertRaises(ValueError):
                debian_binary(binary)

    def test_native_bytes_must_be_present_exactly_once(self):
        verify_binary(self.binary, self.module)
        for binary in [b"ordinary-binary", self.module + self.module]:
            with self.assertRaises(ValueError):
                verify_binary(binary, self.module)

    def test_bundle_hash_and_length_cannot_be_substituted(self):
        self.assertEqual(read_bundle(self.bundle), self.module)
        (self.bundle / "liblyn-gnome-native.so").write_bytes(self.module + b"changed")
        with self.assertRaises(ValueError):
            read_bundle(self.bundle)

    def test_packaged_binary_must_equal_the_checked_release_binary(self):
        for content in [self.binary, self.binary + b"stale", self.binary[:-1] + b"X"]:
            stream = io.BytesIO()
            with tarfile.open(fileobj=stream, mode="w") as archive:
                item = tarfile.TarInfo("./usr/bin/lyn")
                item.size = len(content)
                archive.addfile(item, io.BytesIO(content))
            stream.seek(0)
            with tarfile.open(fileobj=stream) as archive:
                if content == self.binary:
                    verify_archive(archive, "./usr/bin/lyn", self.binary)
                else:
                    with self.assertRaises(ValueError):
                        verify_archive(archive, "./usr/bin/lyn", self.binary)

    def test_debian_plain_member_name_is_equivalent_but_duplicate_spellings_refuse(self):
        for names in [["usr/bin/lyn"], ["./usr/bin/lyn", "usr/bin/lyn"]]:
            stream = io.BytesIO()
            with tarfile.open(fileobj=stream, mode="w") as archive:
                for name in names:
                    item = tarfile.TarInfo(name)
                    item.size = len(self.binary)
                    archive.addfile(item, io.BytesIO(self.binary))
            stream.seek(0)
            with tarfile.open(fileobj=stream) as archive:
                if len(names) == 1:
                    verify_archive(archive, "./usr/bin/lyn", self.binary)
                else:
                    with self.assertRaises(ValueError):
                        verify_archive(archive, "./usr/bin/lyn", self.binary)

    def test_duplicate_or_symlink_binary_is_refused_without_extraction(self):
        for duplicate in [False, True]:
            stream = io.BytesIO()
            with tarfile.open(fileobj=stream, mode="w") as archive:
                item = tarfile.TarInfo("lyn/lyn")
                if duplicate:
                    item.size = len(self.binary)
                    archive.addfile(item, io.BytesIO(self.binary))
                    archive.addfile(item, io.BytesIO(self.binary))
                else:
                    item.type = tarfile.SYMTYPE
                    item.linkname = "/outside"
                    archive.addfile(item)
            stream.seek(0)
            with tarfile.open(fileobj=stream) as archive:
                with self.assertRaises(ValueError):
                    verify_archive(archive, "lyn/lyn", self.binary)


if __name__ == "__main__":
    unittest.main()
