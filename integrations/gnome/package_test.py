"""Load a verified package's native bytes only inside the private GNOME fixture.

The packaged GUI is not launched. This is resource load/backend-save evidence,
not installed Tauri IPC, real-manager consumption or package-manager transactions.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from build import ROOT, module_flags
from package_resources import recover_bundle, read_packages

REPO = ROOT.parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--receipt", required=True, type=Path)
    parser.add_argument("--portable", required=True, type=Path)
    parser.add_argument("--deb", required=True, type=Path)
    parser.add_argument("--previous-receipt", type=Path)
    parser.add_argument("--previous-portable", type=Path)
    parser.add_argument("--previous-deb", type=Path)
    args = parser.parse_args()
    previous = [args.previous_receipt, args.previous_portable, args.previous_deb]
    if any(previous) and not all(previous):
        parser.error("Supply all three previous-package inputs together")
    packages = [read_packages(args.receipt, args.portable, args.deb)]
    if all(previous):
        packages.insert(0, read_packages(*previous))
        if packages[0][0]['version'] == packages[1][0]['version']:
            parser.error("Roundtrip requires distinct application versions")
        if packages[0][0]['moduleSha256'] == packages[1][0]['moduleSha256']:
            parser.error("Native roundtrip requires distinct native payloads")
    with tempfile.TemporaryDirectory(prefix="lyn-package-runtime-") as directory:
        output = Path(directory)
        modules = []
        for index, (receipt, binaries) in enumerate(packages):
            recovered = output / f"package-{index}"
            recover_bundle(binaries['debian'], receipt, 'debianBinarySha256', recovered)
            modules.append(recovered / 'liblyn-gnome-native.so')
        module, updated_module = modules[0], modules[-1]
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
        subprocess.run([sys.executable, str(ROOT / "prototype/isolated_run.py"), str(sdk) if sdk else "-",
                        str(ROOT / "prototype/native_activation_test.py"), str(module), str(preserved),
                        probes[0], str(sdk) if sdk else "-", str(updated_module),
                        "--packaged-roundtrip" if len(packages) == 2 else "--packaged-smoke"],
                       cwd=output, check=True)
        if len(packages) == 2:
            print(json.dumps(dict(check="packaged_native_roundtrip", previousVersion=packages[0][0]['version'],
                                  currentVersion=packages[1][0]['version'],
                                  previousModuleSha256=packages[0][0]['moduleSha256'],
                                  currentModuleSha256=packages[1][0]['moduleSha256'])), flush=True)
        print("PASS: matching Debian/portable embedded resources, no native-module rebuild or package install", flush=True)


if __name__ == "__main__":
    main()
