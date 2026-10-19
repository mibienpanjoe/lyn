"""Build and exercise the opt-in native prototype; install nothing."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile


root = Path(__file__).resolve().parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--desktop", action="store_true")
parser.add_argument("--startup-pairs", type=int, default=1)
parser.add_argument("--warm-alternations", type=int, default=0)
parser.add_argument("--tab-relocation", action="store_true")
parser.add_argument("--invocation-races", action="store_true")
parser.add_argument("--late-app", action="store_true")
parser.add_argument("--restart-app", action="store_true")
parser.add_argument("--stale-reconnect", action="store_true")
parser.add_argument("--closed-source", action="store_true")
parser.add_argument("--stale-selection", action="store_true")
parser.add_argument("--cwd-change", action="store_true")
parser.add_argument("--pty-reuse", action="store_true")
parser.add_argument("--foreground-shapes", action="store_true")
arguments = parser.parse_args()
if not 1 <= arguments.startup_pairs <= 20 or (not arguments.desktop and arguments.startup_pairs != 1):
    parser.error("--startup-pairs requires --desktop and a value from 1 to 20")
if not 0 <= arguments.warm_alternations <= 100 or (not arguments.desktop and arguments.warm_alternations):
    parser.error("--warm-alternations requires --desktop and a value from 0 to 100")
if arguments.tab_relocation and not arguments.desktop:
    parser.error("--tab-relocation requires --desktop")
if arguments.invocation_races and not arguments.desktop:
    parser.error("--invocation-races requires --desktop")
if arguments.late_app and not arguments.desktop:
    parser.error("--late-app requires --desktop")
if arguments.restart_app and not arguments.desktop:
    parser.error("--restart-app requires --desktop")
if arguments.stale_reconnect and not arguments.desktop:
    parser.error("--stale-reconnect requires --desktop")
if arguments.closed_source and not arguments.desktop:
    parser.error("--closed-source requires --desktop")
if arguments.stale_selection and not arguments.desktop:
    parser.error("--stale-selection requires --desktop")
if arguments.cwd_change and not arguments.desktop:
    parser.error("--cwd-change requires --desktop")
if arguments.pty_reuse and not arguments.desktop:
    parser.error("--pty-reuse requires --desktop")
if arguments.foreground_shapes and not arguments.desktop:
    parser.error("--foreground-shapes requires --desktop")
desktop = arguments.desktop
if desktop:
    subprocess.run([sys.executable, str(root / "foreground_fixture_test.py")], check=True)
    subprocess.run([sys.executable, str(root / "cwd_fixture_test.py")], check=True)
    subprocess.run([sys.executable, str(root / "desktop_accessibility_test.py")], check=True)
    subprocess.run([sys.executable, str(root / "desktop_pixels_test.py")], check=True)
subprocess.run([sys.executable, str(root / "isolation_test.py")], check=True)
subprocess.run([sys.executable, str(root / "desktop_isolation_test.py")], check=True)
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

# Build the current cfg(test) entrypoint, never expose its control socket in the
# application. Cargo's artifact message is authoritative even with TARGET_DIR.
repo = root.parents[2]
build = subprocess.run(["cargo", "test", "--manifest-path", str(repo / "src-tauri/Cargo.toml"),
                        "--lib", "--no-run", "--message-format=json"],
                       cwd=repo, check=True, stdout=subprocess.PIPE, text=True)
artifacts = [json.loads(line) for line in build.stdout.splitlines()]
probes = [item["executable"] for item in artifacts
          if item.get("reason") == "compiler-artifact" and item["target"]["name"] == "lyn_lib"
          and item["profile"]["test"] and item.get("executable")]
assert len(probes) == 1, "Expected one current Rust library test executable"
probe = Path(probes[0]).resolve(strict=True)

with tempfile.TemporaryDirectory(prefix="lyn-gnome-build-") as directory:
    destination = Path(directory)
    module = destination / "liblyn-gnome-prototype.so"
    preserved = destination / "libexisting-gtk-module.so"
    wire_test = destination / "wire-test"
    startup_fixture = destination / "gnome-terminal-server"
    lifecycle_test = destination / "channel-lifecycle-test"
    subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                    "-fstack-protector-strong", str(root / "channel_lifecycle_test.c"),
                    str(root.parent / "native_channel.c"), str(root.parent / "wire.c"),
                    "-o", str(lifecycle_test), *flags], check=True)
    subprocess.run([str(lifecycle_test), str(destination)], check=True, timeout=3)
    subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                    "-fstack-protector-strong", str(root / "startup_fixture.c"),
                    "-o", str(startup_fixture), *flags], check=True)
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
    if desktop:
        fixture_args = [str(module), str(probe), str(sdk) if sdk else "-"]
        modes = [["--probe"], []] + [["--concurrent-start"] for _ in range(arguments.startup_pairs)]
        if arguments.warm_alternations:
            modes.extend([f"--warm-{mode}", f"--alternations={arguments.warm_alternations}"]
                         for mode in ("windows", "tabs"))
        if arguments.tab_relocation:
            modes.append(["--relocate-tab"])
        if arguments.invocation_races:
            modes.extend([["--race-switch"], ["--race-aba"]])
        if arguments.late_app:
            modes.append(["--late-app"])
        if arguments.restart_app:
            modes.append(["--restart-app"])
        if arguments.stale_reconnect:
            modes.append(["--stale-reconnect"])
        if arguments.closed_source:
            modes.append(["--closed-source"])
        if arguments.stale_selection:
            modes.append(["--stale-selection"])
        if arguments.cwd_change:
            modes.append(["--cwd-change"])
        if arguments.pty_reuse:
            modes.append(["--pty-reuse"])
        if arguments.foreground_shapes:
            modes.extend([[f"--process-shape={shape}"] for shape in ("shell", "exec-tui", "child-tui")])
        cases = [("desktop_runtime_test.py", [*fixture_args, *mode]) for mode in modes]
    else:
        cases = (("startup_test.py", [str(module), str(startup_fixture)]),
                 ("reconnect_test.py", [str(module), str(probe)]),
                 ("smoke_test.py", [str(module)]),
                 ("identity_test.py", [str(module), str(preserved)]),
                 ("rust_broker_test.py", [str(module), str(probe)]))
    for test, arguments in cases:
        subprocess.run([sys.executable, str(root / "isolated_run.py"),
                        str(sdk) if sdk else "-", str(root / test), *arguments], check=True)
    print("PASS: requested native fixture checks passed; nothing installed")
