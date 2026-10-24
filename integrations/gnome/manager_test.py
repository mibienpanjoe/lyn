"""Build and verify GNOME activation definitions with a private real systemd."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from build import ROOT, build_bundle

sys.path.insert(0, str(ROOT / "prototype"))
from isolated_run import run_fixture


def main():
    if len(sys.argv) != 1:
        raise SystemExit("This private manager suite accepts no arguments")
    repo = ROOT.parents[1]
    subprocess.run([sys.executable, str(ROOT / "prototype/real_manager_guard_test.py")], check=True, timeout=5)
    sdk_value = os.environ.get("LYN_GNOME_SDK_ROOT")
    sdk = Path(sdk_value).resolve(strict=True) if sdk_value else None
    with tempfile.TemporaryDirectory(prefix="lyn-manager-build-") as directory:
        bundle = Path(directory) / "bundle"
        build_bundle(bundle, sdk)
        compiled = subprocess.run(["cargo", "test", "--manifest-path", str(repo / "src-tauri/Cargo.toml"),
                                   "--lib", "--no-run", "--message-format=json"],
                                  cwd=repo, check=True, stdout=subprocess.PIPE, text=True)
        messages = [json.loads(line) for line in compiled.stdout.splitlines()]
        probes = [item["executable"] for item in messages if item.get("reason") == "compiler-artifact"
                  and item["target"]["name"] == "lyn_lib" and item["profile"]["test"] and item.get("executable")]
        assert len(probes) == 1, "Expected the current Cargo library test executable"
        result = run_fixture([sys.executable, str(ROOT / "prototype/real_manager_runtime_test.py"),
                              probes[0], str(bundle / "liblyn-gnome-native.so")], dict(os.environ))
        if result:
            raise SystemExit(result)


if __name__ == "__main__":
    main()
