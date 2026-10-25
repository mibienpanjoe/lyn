"""Recover only verified embedded bytes for the developer's private fixture.

This receipt is content evidence, never production activation authority. Rust
still validates the closed manifest/ELF before storing a fixture module.
"""
import hashlib
import json


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
