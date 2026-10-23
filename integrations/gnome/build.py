"""Build the native module without installing or activating it."""

import argparse
import os
from pathlib import Path
import shlex
import subprocess


ROOT = Path(__file__).resolve().parent


def module_flags(sdk=None):
    packages = ["gtk+-3.0", "gmodule-2.0"]
    if not sdk:
        packages.append("vte-2.91")
    flags = shlex.split(subprocess.check_output(["pkg-config", "--cflags", "--libs", *packages], text=True))
    if sdk:
        headers = sdk / "root/usr/include/vte-2.91"
        assert headers.is_dir(), "SDK has no extracted VTE headers"
        flags.extend(["-I", str(headers), "-l:libvte-2.91.so.0"])
    return flags


def build(output, sdk=None):
    subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                    "-fPIC", "-fstack-protector-strong", "-shared", str(ROOT / "module.c"),
                    *[str(ROOT / name) for name in ("wire.c", "native_channel.c", "native_discovery.c")],
                    "-o", str(output), *module_flags(sdk)], check=True)
    assert b"LYN_GNOME_TEST_" not in output.read_bytes(), "Native module retains diagnostic opt-ins"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    sdk = os.environ.get("LYN_GNOME_SDK_ROOT")
    build(arguments.output, Path(sdk).resolve(strict=True) if sdk else None)
