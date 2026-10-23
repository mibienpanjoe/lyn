"""Exercise native activation only in private Xvfb/D-Bus/XDG; install nothing."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from build import ROOT, build_bundle, module_flags


def main():
    repo = ROOT.parents[1]
    subprocess.run([sys.executable, str(ROOT / "prototype/managed_service_fixture_test.py")], check=True, timeout=3)
    sdk_value = os.environ.get("LYN_GNOME_SDK_ROOT")
    sdk = Path(sdk_value).resolve(strict=True) if sdk_value else None
    flags = module_flags(sdk)
    with tempfile.TemporaryDirectory(prefix="lyn-native-build-") as directory:
        output = Path(directory)
        bundle = output / "bundle"
        build_bundle(bundle, sdk)
        module = bundle / "liblyn-gnome-native.so"
        preserved = output / "libexisting-module.so"
        common = ["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", "-fstack-protector-strong"]
        subprocess.run([*common, "-fPIC", "-shared", str(ROOT / "prototype/preserved_module.c"),
                        "-o", str(preserved), *flags], check=True)
        refusal = output / "gnome-terminal-server"
        subprocess.run([*common, str(ROOT / "prototype/native_bootstrap_test.c"),
                        "-o", str(refusal), *flags], check=True)
        subprocess.run([str(refusal), str(module)], check=True, timeout=3)
        # Shared prototype hook behavior must retain its mismatch refusal.
        sources = [str(ROOT / name) for name in ("wire.c", "native_channel.c", "native_discovery.c")]
        incompatible = output / "libincompatible.so"
        compatibility = output / "compatibility"
        subprocess.run([*common, "-DLYN_GNOME_TEST_REFUSED_MODULE", "-fPIC", "-shared",
                        str(ROOT / "prototype/compatibility_test.c"), *sources,
                        "-o", str(incompatible), *flags], check=True)
        subprocess.run([*common, str(ROOT / "prototype/compatibility_test.c"), *sources,
                        "-o", str(compatibility), *flags], check=True)
        subprocess.run([str(compatibility), str(incompatible)], check=True, timeout=3)
        compiled = subprocess.run(["cargo", "test", "--manifest-path", str(repo / "src-tauri/Cargo.toml"),
                                   "--lib", "--no-run", "--message-format=json"],
                                  cwd=repo, check=True, stdout=subprocess.PIPE, text=True)
        messages = [json.loads(line) for line in compiled.stdout.splitlines()]
        probes = [item["executable"] for item in messages if item.get("reason") == "compiler-artifact"
                  and item["target"]["name"] == "lyn_lib" and item["profile"]["test"] and item.get("executable")]
        assert len(probes) == 1, "Expected the current Cargo library test executable"
        subprocess.run([sys.executable, str(ROOT / "prototype/isolated_run.py"), str(sdk) if sdk else "-",
                        str(ROOT / "prototype/native_activation_test.py"), str(module), str(preserved),
                        probes[0], str(sdk) if sdk else "-"], check=True)


if __name__ == "__main__":
    main()
