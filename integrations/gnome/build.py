"""Build the native module without installing or activating it."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile


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
    if b"LYN_GNOME_TEST_" in output.read_bytes():
        raise ValueError("Native module retains diagnostic opt-ins")


def reference_versions(sdk=None):
    """Check headers and linked runtime without GTK initialization or a display."""
    source = r'''
#include <gtk/gtk.h>
#include <vte/vte.h>
#include <stdio.h>
#if !defined(__x86_64__) || !defined(__linux__) || !defined(__GLIBC__)
#error Unsupported native bundle build target
#endif
int main(void) {
    printf("[%u,%u,%u,%u,%u,%u,%u,%u,%u,%u,%u,%u]",
           GTK_MAJOR_VERSION, GTK_MINOR_VERSION, GTK_MICRO_VERSION,
           VTE_MAJOR_VERSION, VTE_MINOR_VERSION, VTE_MICRO_VERSION,
           gtk_get_major_version(), gtk_get_minor_version(), gtk_get_micro_version(),
           vte_get_major_version(), vte_get_minor_version(), vte_get_micro_version());
    return 0;
}
'''
    with tempfile.TemporaryDirectory(prefix="lyn-native-version-") as directory:
        executable = Path(directory) / "versions"
        subprocess.run(["gcc", "-std=c11", "-Wall", "-Wextra", "-Werror", "-x", "c", "-",
                        "-o", str(executable), *module_flags(sdk)], input=source, text=True, check=True)
        versions = json.loads(subprocess.check_output([str(executable)], text=True, timeout=5))
    if versions != [3, 24, 33, 0, 68, 0] * 2:
        raise ValueError(f"Unsupported native header/runtime stack: {versions}")


def build_bundle(directory, sdk=None):
    reference_versions(sdk)
    directory.mkdir()  # Refuse an occupied output; never replace another bundle.
    module = directory / "liblyn-gnome-native.so"
    build(module, sdk)
    content = module.read_bytes()
    if len(content) > 1024 * 1024:
        raise ValueError("Native module exceeds bundle size limit")
    manifest = {"version": 1, "protocol": 1, "target": "x86_64-unknown-linux-gnu",
                "gtk": [3, 24, 33], "vte": [0, 68, 0], "length": len(content),
                "sha256": hashlib.sha256(content).hexdigest()}
    # Manifest is last: an interrupted local build is never an admitted bundle.
    (directory / "manifest.json").write_text(json.dumps(manifest, sort_keys=True) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    output = parser.add_mutually_exclusive_group(required=True)
    output.add_argument("--output", type=Path)
    output.add_argument("--bundle-dir", type=Path)
    arguments = parser.parse_args()
    sdk = os.environ.get("LYN_GNOME_SDK_ROOT")
    sdk = Path(sdk).resolve(strict=True) if sdk else None
    if arguments.bundle_dir:
        build_bundle(arguments.bundle_dir, sdk)
    else:
        build(arguments.output, sdk)
