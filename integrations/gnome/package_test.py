"""Load a verified package's native bytes only inside the private GNOME fixture.

The packaged GUI is not launched. This is resource load/backend-save evidence,
not installed Tauri IPC, real-manager consumption or package update acceptance.
"""
import argparse
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile

from build import ROOT, module_flags
from package_resources import recover_bundle

REPO = ROOT.parents[1]
sys.path.insert(0, str(REPO / "scripts"))
from verify_linux_native import bounded_file, read_archive_binary, debian_binary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--receipt", required=True, type=Path)
    parser.add_argument("--portable", required=True, type=Path)
    parser.add_argument("--deb", required=True, type=Path)
    args = parser.parse_args()
    receipt = json.loads(bounded_file(args.receipt, 16384))
    with tempfile.TemporaryDirectory(prefix="lyn-package-runtime-") as directory:
        output = Path(directory)
        with tarfile.open(args.portable, "r:gz") as archive:
            raw = read_archive_binary(archive, f"lyn-{receipt['version']}-linux-x86_64/lyn")
        portable_module = recover_bundle(raw, receipt, "binarySha256", output / "portable")
        data = subprocess.check_output(["dpkg-deb", "--fsys-tarfile", str(args.deb)], timeout=10)
        with tarfile.open(fileobj=io.BytesIO(data)) as archive:
            packaged = read_archive_binary(archive, "usr/bin/lyn")
        if packaged != debian_binary(raw):
            raise ValueError("Debian executable differs beyond the known Tauri marker")
        debian_module = recover_bundle(packaged, receipt, "debianBinarySha256", output / "debian")
        if portable_module != debian_module:
            raise ValueError("Packages contain different native resources")
        # No build.py native module build: only the package-extracted producer.
        sdk_value = os.environ.get("LYN_GNOME_SDK_ROOT")
        sdk = Path(sdk_value).resolve(strict=True) if sdk_value else None
        preserved = output / "libexisting-module.so"
        subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                        "-fPIC", "-fstack-protector-strong", "-shared",
                        str(ROOT / "prototype/preserved_module.c"), "-o", str(preserved),
                        *module_flags(sdk)], check=True, timeout=10)
        compiled = subprocess.run(["cargo", "test", "--manifest-path", str(REPO / "src-tauri/Cargo.toml"),
                                   "--lib", "--no-run", "--message-format=json"],
                                  cwd=REPO, check=True, stdout=subprocess.PIPE, text=True)
        messages = [json.loads(line) for line in compiled.stdout.splitlines()]
        probes = [item["executable"] for item in messages if item.get("reason") == "compiler-artifact"
                  and item["target"]["name"] == "lyn_lib" and item["profile"]["test"] and item.get("executable")]
        if len(probes) != 1:
            raise ValueError("Expected the current Cargo library test executable")
        module = output / "debian/liblyn-gnome-native.so"
        subprocess.run([sys.executable, str(ROOT / "prototype/isolated_run.py"), str(sdk) if sdk else "-",
                        str(ROOT / "prototype/native_activation_test.py"), str(module), str(preserved),
                        probes[0], str(sdk) if sdk else "-", str(module), "--packaged-smoke"],
                       cwd=output, check=True)
        print("PASS: matching Debian/portable embedded resources, no native-module rebuild or package install", flush=True)


if __name__ == "__main__":
    main()
