"""Build and exercise the opt-in native prototype; install nothing."""

import hashlib
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile


root = Path(__file__).resolve().parent
sdk_value = os.environ.get("LYN_GNOME_SDK_ROOT")
sdk = Path(sdk_value).resolve(strict=True) if sdk_value else None
packages = ["gtk+-3.0", "gmodule-2.0"]
if not sdk:
    packages.append("vte-2.91")
try:
    flags = shlex.split(subprocess.check_output(
        ["pkg-config", "--cflags", "--libs", *packages], text=True))
except subprocess.CalledProcessError:
    raise SystemExit("GTK3/VTE development headers are required; no system packages were installed.")
if sdk:
    headers = sdk / "root/usr/include/vte-2.91"
    if not headers.is_dir():
        raise SystemExit("LYN_GNOME_SDK_ROOT must contain an extracted VTE development package.")
    flags.extend(["-I", str(headers), "-l:libvte-2.91.so.0"])

with tempfile.TemporaryDirectory(prefix="lyn-gnome-build-") as directory:
    destination = Path(directory)
    module = destination / "liblyn-gnome-prototype.so"
    preserved = destination / "libexisting-gtk-module.so"
    wire_test = destination / "wire-test"
    subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                    "-fstack-protector-strong", str(root / "wire_test.c"),
                    str(root.parent / "wire.c"), "-o", str(wire_test)], check=True)
    subprocess.run([str(wire_test), str(root.parent / "wire_vectors.txt"),
                    str(root.parent / "wire_rejects.txt")], check=True)
    for source, output in (("module.c", module), ("preserved_module.c", preserved)):
        extra_sources = [str(root.parent / file) for file in ("wire.c", "native_channel.c")] \
            if source == "module.c" else []
        subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                        "-fPIC", "-fstack-protector-strong", "-shared", str(root / source),
                        *extra_sources, "-o", str(output), *flags], check=True)
    print("Native prototype SHA256:", hashlib.sha256(module.read_bytes()).hexdigest(), flush=True)
    for test, arguments in (("smoke_test.py", [str(module)]),
                            ("identity_test.py", [str(module), str(preserved)])):
        subprocess.run([sys.executable, str(root / "isolated_run.py"),
                        str(sdk) if sdk else "-", str(root / test), *arguments], check=True)
    print("PASS: GNOME native prototype compiled and passed isolated runtime checks; nothing installed")
