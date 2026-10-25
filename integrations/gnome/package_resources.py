"""Recover only verified embedded bytes for the developer's private fixture.

This receipt is content evidence, never production activation authority. Rust
still validates the closed manifest/ELF before storing a fixture module.
"""
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'scripts'))
from verify_linux_native import bounded_file, read_archive_binary, debian_binary


def recover_bundle(binary, receipt, digest_name, directory):
    offset, length = receipt["moduleOffset"], receipt["moduleLength"]
    if (type(offset) is not int or type(length) is not int or offset < 0
            or not 1 <= length <= 1024 * 1024 or offset + length > len(binary)):
        raise ValueError("Invalid embedded resource bounds")
    if hashlib.sha256(binary).hexdigest() != receipt[digest_name]:
        raise ValueError("Package executable no longer matches its receipt")
    module = binary[offset:offset + length]
    manifest = receipt["nativeManifest"]
    digest = hashlib.sha256(module).hexdigest()
    if (digest != receipt["moduleSha256"] or manifest["sha256"] != digest
            or manifest["length"] != length or binary.count(module) != 1):
        raise ValueError("Embedded bytes no longer match their native manifest")
    directory.mkdir(mode=0o700)  # Never replace an occupied output.
    (directory / "liblyn-gnome-native.so").write_bytes(module)
    (directory / "manifest.json").write_text(json.dumps(manifest, sort_keys=True) + "\n")
    return module


def read_packages(receipt_path, portable_path, deb_path):
    receipt = json.loads(bounded_file(receipt_path, 16384))
    with tarfile.open(portable_path, 'r:gz') as archive:
        portable = read_archive_binary(archive, f"lyn-{receipt['version']}-linux-x86_64/lyn")
    data = subprocess.check_output(['dpkg-deb', '--fsys-tarfile', str(deb_path)], timeout=10)
    with tarfile.open(fileobj=io.BytesIO(data)) as archive:
        deb = read_archive_binary(archive, 'usr/bin/lyn')
    if deb != debian_binary(portable):
        raise ValueError('Package binaries differ beyond the expected Tauri marker')
    with tempfile.TemporaryDirectory(prefix='lyn-package-check-') as directory:
        root = Path(directory)
        recover_bundle(portable, receipt, 'binarySha256', root / 'portable')
        recover_bundle(deb, receipt, 'debianBinarySha256', root / 'debian')
    return receipt, {'portable': portable, 'debian': deb}
