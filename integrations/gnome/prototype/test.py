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
from sanitizer_environment import sanitizer_environment
from source_lifetime_test import lifetime_environment


root = Path(__file__).resolve().parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--desktop", action="store_true")
parser.add_argument("--pid-reuse", action="store_true")
parser.add_argument("--startup-pairs", type=int, default=1)
parser.add_argument("--warm-alternations", type=int, default=0)
parser.add_argument("--tab-relocation", action="store_true")
parser.add_argument("--invocation-races", action="store_true")
parser.add_argument("--late-app", action="store_true")
parser.add_argument("--restart-app", action="store_true")
parser.add_argument("--crash-app", action="store_true")
parser.add_argument("--stale-reconnect", action="store_true")
parser.add_argument("--closed-source", action="store_true")
parser.add_argument("--closed-tab", action="store_true")
parser.add_argument("--shell-exit", action="store_true")
parser.add_argument("--stale-selection", action="store_true")
parser.add_argument("--cwd-change", action="store_true")
parser.add_argument("--pty-reuse", action="store_true")
parser.add_argument("--xid-reuse", action="store_true")
parser.add_argument("--foreground-shapes", action="store_true")
parser.add_argument("--image-closed-source", action="store_true")
parser.add_argument("--audio-closed-source", action="store_true")
parser.add_argument("--module-absent", action="store_true")
parser.add_argument("--module-slow", action="store_true")
parser.add_argument("--channel-cut", action="store_true")
parser.add_argument("--native-safety", action="store_true",
                    help="add ASan/UBSan transport fixtures, not an instrumented GNOME server")
parser.add_argument("--native-metrics", action="store_true",
                    help="measure private real GNOME CPU/RSS and test-build callback bodies")
parser.add_argument("--reconnect-cycles", type=int,
                    help="repeat real GNOME/Rust broker reconnects (1 to 20, native mode only)")
parser.add_argument("--reconnect-discovery", action="store_true",
                    help="use fresh published broker endpoints for the reconnect series")
parser.add_argument("--event-storm", action="store_true",
                    help="exercise 200 private GNOME tab switches concurrent with metadata queries")
parser.add_argument("--source-lifetimes", action="store_true",
                    help="diagnose real VTE source lifetimes under sanitizers, including a no-Lyn baseline")
arguments = parser.parse_args()
if arguments.source_lifetimes and arguments.desktop:
    parser.error("--source-lifetimes requires native mode")
if arguments.event_storm and arguments.desktop:
    parser.error("--event-storm requires native mode")
if arguments.reconnect_discovery and arguments.desktop:
    parser.error("--reconnect-discovery requires native mode")
if arguments.reconnect_cycles is not None and (arguments.desktop or not 1 <= arguments.reconnect_cycles <= 20):
    parser.error("--reconnect-cycles requires native mode and a value from 1 to 20")
if arguments.reconnect_cycles is None:
    arguments.reconnect_cycles = 1
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
if arguments.crash_app and not arguments.desktop:
    parser.error("--crash-app requires --desktop")
if arguments.stale_reconnect and not arguments.desktop:
    parser.error("--stale-reconnect requires --desktop")
if arguments.closed_source and not arguments.desktop:
    parser.error("--closed-source requires --desktop")
if arguments.closed_tab and not arguments.desktop:
    parser.error("--closed-tab requires --desktop")
if arguments.shell_exit and not arguments.desktop:
    parser.error("--shell-exit requires --desktop")
if arguments.stale_selection and not arguments.desktop:
    parser.error("--stale-selection requires --desktop")
if arguments.cwd_change and not arguments.desktop:
    parser.error("--cwd-change requires --desktop")
if arguments.pty_reuse and not arguments.desktop:
    parser.error("--pty-reuse requires --desktop")
if arguments.xid_reuse and not arguments.desktop:
    parser.error("--xid-reuse requires --desktop")
if arguments.foreground_shapes and not arguments.desktop:
    parser.error("--foreground-shapes requires --desktop")
if arguments.image_closed_source and not arguments.desktop:
    parser.error("--image-closed-source requires --desktop")
if arguments.audio_closed_source and not arguments.desktop:
    parser.error("--audio-closed-source requires --desktop")
if arguments.module_absent and not arguments.desktop:
    parser.error("--module-absent requires --desktop")
if arguments.module_slow and not arguments.desktop:
    parser.error("--module-slow requires --desktop")
if arguments.channel_cut and not arguments.desktop:
    parser.error("--channel-cut requires --desktop")
desktop = arguments.desktop
subprocess.run([sys.executable, str(root / "reconnect_controller_test.py")], check=True)
if desktop:
    subprocess.run([sys.executable, str(root / "desktop_latency_test.py")], check=True)
    subprocess.run([sys.executable, str(root / "desktop_audio_test.py")], check=True)
    subprocess.run([sys.executable, str(root / "desktop_media_test.py")], check=True)
    subprocess.run([sys.executable, str(root / "desktop_shell_exit_test.py")], check=True)
    subprocess.run([sys.executable, str(root / "desktop_lifecycle_test.py")], check=True)
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

if arguments.pid_reuse:
    subprocess.run([sys.executable, str(root / "pid_reuse_runner_test.py")], check=True)
    subprocess.run([sys.executable, str(root / "pid_reuse_runner.py"), str(probe)], check=True)

with tempfile.TemporaryDirectory(prefix="lyn-gnome-build-") as directory:
    destination = Path(directory)
    module = destination / "liblyn-gnome-prototype.so"
    preserved = destination / "libexisting-gtk-module.so"
    wire_test = destination / "wire-test"
    startup_fixture = destination / "gnome-terminal-server"
    traversal_fixture = destination / "traversal-test"
    fault_fixture = destination / "channel-fault-test"
    lifecycle_test = destination / "channel-lifecycle-test"
    discovery_library = destination / "libnative-discovery-test.so"
    discovery_channel_test = destination / "discovery-channel-test"
    compatibility_test = destination / "compatibility-test"
    refused_module = destination / "librefused-compatibility-test.so"
    subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                    "-DLYN_GNOME_TEST_REFUSED_MODULE", "-fPIC", "-shared",
                    "-fstack-protector-strong", str(root / "compatibility_test.c"),
                    *[str(root.parent / file) for file in ("wire.c", "native_channel.c", "native_discovery.c")],
                    "-o", str(refused_module), *flags], check=True)
    subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                    "-fstack-protector-strong", str(root / "compatibility_test.c"),
                    *[str(root.parent / file) for file in ("wire.c", "native_channel.c", "native_discovery.c")],
                    "-o", str(compatibility_test), *flags], check=True)
    subprocess.run([str(compatibility_test), str(refused_module)], check=True, timeout=3)
    subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                    "-fPIC", "-fstack-protector-strong", "-shared",
                    str(root.parent / "native_discovery.c"),
                    "-o", str(discovery_library)], check=True)
    subprocess.run([sys.executable, str(root / "native_discovery_test.py"),
                    str(discovery_library)], check=True, timeout=5)
    subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                    "-fstack-protector-strong", str(root / "discovery_channel_test.c"),
                    str(root.parent / "native_channel.c"), str(root.parent / "wire.c"),
                    str(root.parent / "native_discovery.c"),
                    "-o", str(discovery_channel_test), *flags], check=True)
    subprocess.run([str(discovery_channel_test), str(destination)], check=True, timeout=5)
    subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                    "-fstack-protector-strong", str(root / "channel_lifecycle_test.c"),
                    str(root.parent / "native_channel.c"), str(root.parent / "wire.c"),
                    str(root.parent / "native_discovery.c"), "-o", str(lifecycle_test), *flags], check=True)
    subprocess.run([str(lifecycle_test), str(destination)], check=True, timeout=3)
    subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                    "-fstack-protector-strong", str(root / "channel_fault_test.c"),
                    str(root.parent / "wire.c"), str(root.parent / "native_discovery.c"),
                    "-o", str(fault_fixture), *flags], check=True)
    subprocess.run([str(fault_fixture), str(destination)], check=True, timeout=3)
    if arguments.native_safety:
        subprocess.run([sys.executable, str(root / "sanitizer_environment_test.py")], check=True)
        safety_environment = sanitizer_environment(os.environ)
        for source in ("channel_fault_test.c", "channel_lifecycle_test.c", "discovery_channel_test.c"):
            instrumented = destination / (Path(source).stem + "-sanitized")
            sources = [str(root / source), str(root.parent / "wire.c"),
                       str(root.parent / "native_discovery.c")]
            if source != "channel_fault_test.c":
                sources.append(str(root.parent / "native_channel.c"))
            subprocess.run(["gcc", "-std=c11", "-O1", "-g", "-Wall", "-Wextra", "-Werror",
                            "-fsanitize=address,undefined", "-fno-omit-frame-pointer",
                            *sources, "-o", str(instrumented), *flags], check=True)
            subprocess.run([str(instrumented), str(destination)], env=safety_environment,
                           check=True, timeout=5)
    subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                    "-fstack-protector-strong", str(root / "startup_fixture.c"),
                    "-o", str(startup_fixture), *flags], check=True)
    subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                    "-fstack-protector-strong", str(root / "traversal_test.c"),
                    *[str(root.parent / file) for file in ("wire.c", "native_channel.c", "native_discovery.c")],
                    "-o", str(traversal_fixture), *flags], check=True)
    subprocess.run([sys.executable, str(root / "isolated_run.py"), str(sdk) if sdk else "-",
                    str(root / "traversal_test.py"), str(traversal_fixture), str(sdk) if sdk else "-"], check=True)
    if arguments.native_safety:
        instrumented = destination / "traversal-sanitized"
        subprocess.run(["gcc", "-std=c11", "-O1", "-g", "-Wall", "-Wextra", "-Werror",
                        "-fsanitize=address,undefined", "-fno-omit-frame-pointer",
                        "-fstack-protector-strong", str(root / "traversal_test.c"),
                        *[str(root.parent / file) for file in ("wire.c", "native_channel.c", "native_discovery.c")],
                        "-o", str(instrumented), *flags], check=True)
        subprocess.run([sys.executable, str(root / "isolated_run.py"), str(sdk) if sdk else "-",
                        str(root / "traversal_test.py"), str(instrumented), str(sdk) if sdk else "-"],
                       env=sanitizer_environment(os.environ), check=True)
    subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                    "-fstack-protector-strong", str(root / "wire_test.c"),
                    str(root.parent / "wire.c"), "-o", str(wire_test)], check=True)
    subprocess.run([str(wire_test), str(root.parent / "wire_vectors.txt"),
                    str(root.parent / "wire_rejects.txt")], check=True)
    for source, output in (("module.c", module), ("preserved_module.c", preserved)):
        extra_sources = [str(root.parent / file) for file in ("wire.c", "native_channel.c", "native_discovery.c")] \
            if source == "module.c" else []
        subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                        "-fPIC", "-fstack-protector-strong", "-shared", str(root / source),
                        *extra_sources, "-o", str(output), *flags], check=True)
    print("Native prototype SHA256:", hashlib.sha256(module.read_bytes()).hexdigest(), flush=True)
    if arguments.source_lifetimes:
        subprocess.run([sys.executable, str(root / "source_lifetime_controller_test.py")], check=True)
        reachability = destination / "fontconfig-reachability-sanitized"
        fontconfig_flags = shlex.split(subprocess.check_output(
            ["pkg-config", "--cflags", "--libs", "fontconfig"], text=True))
        subprocess.run(["gcc", "-std=c11", "-O1", "-g", "-Wall", "-Wextra", "-Werror",
                        "-fsanitize=address,undefined", "-fno-omit-frame-pointer",
                        str(root / "fontconfig_reachability_test.c"), "-o", str(reachability),
                        *fontconfig_flags], check=True)
        subprocess.run([str(reachability)], env=lifetime_environment(os.environ), check=True, timeout=3)
        lifetime_results = []
        for source in ("source_lifetime_baseline.c", "source_lifetime_test.c"):
            instrumented = destination / (Path(source).stem + "-sanitized")
            sources = [str(root / source)]
            if source == "source_lifetime_test.c":
                sources.extend(str(root.parent / file) for file in ("wire.c", "native_channel.c", "native_discovery.c"))
            subprocess.run(["gcc", "-std=c11", "-O1", "-g", "-Wall", "-Wextra", "-Werror",
                            "-fsanitize=address,undefined", "-fno-omit-frame-pointer",
                            "-fstack-protector-strong", *sources, "-o", str(instrumented), *flags], check=True)
            # Run each independent comparison once even if the baseline fails;
            # retain every failure. Equal reports are never a sanitizer PASS.
            result = subprocess.run([sys.executable, str(root / "isolated_run.py"), str(sdk) if sdk else "-",
                                     str(root / "source_lifetime_test.py"), str(instrumented), str(sdk) if sdk else "-"],
                                    env=lifetime_environment(os.environ), check=False)
            lifetime_results.append({"source": source, "exitCode": result.returncode})
        print(json.dumps({"check": "vte_source_lifetimes", "results": lifetime_results}), flush=True)
        if any(result["exitCode"] != 0 for result in lifetime_results):
            raise SystemExit("VTE source lifetime sanitizer acceptance incomplete; comparison failures retained")
    if arguments.native_metrics or arguments.event_storm:
        subprocess.run([sys.executable, str(root / "resource_controller_test.py")], check=True)
        extra_sources = [str(root.parent / file) for file in ("wire.c", "native_channel.c", "native_discovery.c")]
        metrics_test = destination / "module-metrics-test"
        metrics_module = destination / "liblyn-gnome-metrics.so"
        subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                        str(root / "module_metrics_test.c"), *extra_sources,
                        "-o", str(metrics_test), *flags], check=True)
        subprocess.run([str(metrics_test)], check=True, timeout=3)
        subprocess.run(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                        "-DLYN_GNOME_TEST_METRICS", "-fPIC", "-fstack-protector-strong", "-shared",
                        str(root / "module.c"), *extra_sources, "-o", str(metrics_module), *flags], check=True)
        print("Metrics prototype SHA256:", hashlib.sha256(metrics_module.read_bytes()).hexdigest(), flush=True)
        if arguments.native_metrics:
            subprocess.run([sys.executable, str(root / "isolated_run.py"), str(sdk) if sdk else "-",
                            str(root / "resource_test.py"), str(metrics_module), str(sdk) if sdk else "-"], check=True)
        if arguments.event_storm:
            subprocess.run([sys.executable, str(root / "event_storm_controller_test.py")], check=True)
            subprocess.run([sys.executable, str(root / "isolated_run.py"), str(sdk) if sdk else "-",
                            str(root / "event_storm_test.py"), str(metrics_module), str(sdk) if sdk else "-"], check=True)
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
        if arguments.crash_app:
            modes.append(["--crash-app"])
        if arguments.stale_reconnect:
            modes.append(["--stale-reconnect"])
        if arguments.closed_source:
            modes.append(["--closed-source"])
        if arguments.closed_tab:
            modes.append(["--closed-tab"])
        if arguments.shell_exit:
            modes.append(["--shell-exit"])
        if arguments.stale_selection:
            modes.append(["--stale-selection"])
        if arguments.cwd_change:
            modes.append(["--cwd-change"])
        if arguments.pty_reuse:
            modes.append(["--pty-reuse"])
        if arguments.xid_reuse:
            modes.append(["--xid-reuse"])
        if arguments.foreground_shapes:
            modes.extend([[f"--process-shape={shape}"] for shape in ("shell", "exec-tui", "child-tui")])
        if arguments.image_closed_source:
            modes.append(["--image-closed-source"])
        if arguments.audio_closed_source:
            modes.append(["--audio-closed-source"])
        if arguments.module_absent:
            modes.append(["--module-absent"])
        if arguments.module_slow:
            modes.append(["--module-slow"])
        if arguments.channel_cut:
            modes.append(["--channel-cut"])
        cases = [("desktop_runtime_test.py", [*fixture_args, *mode]) for mode in modes]
    else:
        cases = (("startup_test.py", [str(module), str(startup_fixture)]),
                 ("reconnect_test.py", [str(module), str(probe), str(arguments.reconnect_cycles),
                                        str(sdk) if sdk else "-",
                                        "published" if arguments.reconnect_discovery else "fixed"]),
                 ("smoke_test.py", [str(module)]),
                 ("identity_test.py", [str(module), str(preserved)]),
                 ("rust_broker_test.py", [str(module), str(probe)]))
    for test, arguments in cases:
        subprocess.run([sys.executable, str(root / "isolated_run.py"),
                        str(sdk) if sdk else "-", str(root / test), *arguments], check=True)
    print("PASS: requested native fixture checks passed; nothing installed")
