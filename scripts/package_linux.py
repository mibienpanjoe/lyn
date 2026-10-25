"""Build Linux packages with a fresh reference native bundle; never install it."""
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile

from verify_linux_native import ROOT, verify_packages

TARGET = "x86_64-unknown-linux-gnu"
TARGET_DIR = ROOT / "src-tauri/target"
RELEASE = TARGET_DIR / TARGET / "release"


def build_environment(bundle):
    return {**os.environ, "LYN_GNOME_NATIVE_BUNDLE": str(bundle),
            "CARGO_TARGET_DIR": str(TARGET_DIR), "CARGO_BUILD_TARGET": TARGET}


def main():
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        raise SystemExit("Native Linux packaging requires an x86_64 Linux build host")
    with tempfile.TemporaryDirectory(prefix="lyn-linux-package-") as staging:
        bundle = Path(staging) / "bundle"
        subprocess.run([sys.executable, str(ROOT / "integrations/gnome/build.py"),
                        "--bundle-dir", str(bundle)], cwd=ROOT, check=True)
        # This is build-time input only; it never selects an installed artifact.
        build_env = build_environment(bundle)
        subprocess.run(["pnpm", "tauri", "build", "--bundles", "deb", "--target", TARGET],
                       cwd=ROOT, env=build_env, check=True)
        subprocess.run(["bash", str(ROOT / "scripts/package-linux-tarball.sh"), str(RELEASE / "lyn")],
                       cwd=ROOT, check=True)
        verify_packages(bundle, RELEASE)


if __name__ == "__main__":
    main()
