"""Verify native bytes in local Linux packages without extracting or running Lyn.

Rust owns manifest/ELF admission. This check proves byte inclusion and package
identity, not module activation or installed desktop acceptance.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[1]
MAX_BINARY = 512 * 1024 * 1024


def bounded_file(path, limit):
    with path.open("rb") as source:
        content = source.read(limit + 1)
    if len(content) > limit:
        raise ValueError(f"Oversized build output: {path.name}")
    return content


def read_bundle(directory):
    manifest = json.loads(bounded_file(directory / "manifest.json", 4096))
    module = bounded_file(directory / "liblyn-gnome-native.so", 1024 * 1024)
    if (not module or manifest["length"] != len(module)
            or manifest["sha256"] != hashlib.sha256(module).hexdigest()):
        raise ValueError("Native reference bytes do not match their build manifest")
    return module


def verify_binary(binary, module):
    if not module or binary.count(module) != 1:
        raise ValueError("Release binary must contain the exact native module once")


def debian_binary(binary):
    # Tauri patches this one marker for DEB and restores the raw release file.
    # https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle.rs
    unknown = b"__TAURI_BUNDLE_TYPE_VAR_UNK"
    if binary.count(unknown) != 1:
        raise ValueError("Expected exactly one known Tauri bundle marker")
    return binary.replace(unknown, b"__TAURI_BUNDLE_TYPE_VAR_DEB", 1)


def read_archive_binary(archive, name):
    entries = [entry for entry in archive.getmembers() if PurePosixPath(entry.name) == PurePosixPath(name)]
    if len(entries) != 1 or not entries[0].isfile() or entries[0].size > MAX_BINARY:
        raise ValueError("Package requires one bounded regular executable")
    with archive.extractfile(entries[0]) as source:
        binary = source.read(MAX_BINARY + 1)
    if len(binary) != entries[0].size:
        raise ValueError("Package executable size mismatch")
    return binary


def verify_archive(archive, name, binary):
    if read_archive_binary(archive, name) != binary:
        raise ValueError("Packaged binary differs from the verified release binary")


def verify_packages(bundle, release=None):
    version = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())["version"]
    release = release or ROOT / "src-tauri/target/release"
    binary = bounded_file(release / "lyn", MAX_BINARY)
    module = read_bundle(bundle)
    verify_binary(binary, module)
    debian = debian_binary(binary)
    verify_binary(debian, module)
    debs = list((release / "bundle/deb").glob(f"*_{version}_amd64.deb"))
    if len(debs) != 1:
        raise ValueError("Expected exactly one current-version amd64 Debian package")
    # Read package streams only. Never install, extract paths or invoke the app.
    result = subprocess.run(["dpkg-deb", "--fsys-tarfile", str(debs[0])],
                            check=True, stdout=subprocess.PIPE)
    with tarfile.open(fileobj=io.BytesIO(result.stdout)) as archive:
        verify_archive(archive, "./usr/bin/lyn", debian)
    name = f"lyn-{version}-linux-x86_64"
    portable = ROOT / "dist-packages" / f"{name}.tar.gz"
    with tarfile.open(portable, "r:gz") as archive:
        verify_archive(archive, f"{name}/lyn", binary)
    report = {"check": "linux_native_packages", "version": version,
                      "binarySha256": hashlib.sha256(binary).hexdigest(),
                      "debianBinarySha256": hashlib.sha256(debian).hexdigest(),
                      "moduleOffset": binary.index(module),
                      "moduleSha256": hashlib.sha256(module).hexdigest(),
                      "moduleLength": len(module), "deb": debs[0].name,
                      "portable": portable.name,
                      "nativeManifest": json.loads(bounded_file(bundle / "manifest.json", 4096))}
    # Developer evidence only, never consumed as installation authority by Lyn.
    receipt = ROOT / "dist-packages" / f"{name}-native.json"
    receipt.write_text(json.dumps(report, sort_keys=True) + "\n")
    print(json.dumps(report, sort_keys=True))
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", required=True, type=Path)
    parser.add_argument("--release-dir", type=Path)
    args = parser.parse_args()
    verify_packages(args.bundle, args.release_dir)
