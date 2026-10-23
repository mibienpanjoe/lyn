"""Stored native bundle -> generated wrapper -> admitted broker -> backend save."""

import json
from contextlib import contextmanager
import os
from pathlib import Path
import re
import shlex
import socket
import struct
import subprocess
import sys
import tempfile
import time

from desktop_isolation import require_owned_display
from desktop_lifecycle import _pin_owned_app
from fixtures import repositories
from resource_test import stop_server
from rust_probe import RustProbe


@contextmanager
def simulated_manager(root, mode, sdk, environment):
    ready = root / f"manager-{mode}.sock"
    with socket.socket(socket.AF_UNIX) as listener:
        listener.bind(str(ready))
        ready.chmod(0o600)
        listener.listen(1)
        listener.settimeout(3)
        process = subprocess.Popen(["/usr/bin/python3", str(Path(__file__).with_name("managed_service_fixture.py")),
                                    str(root), mode, str(ready), str(sdk) if sdk else "-"], env=environment)
        try:
            channel, _ = listener.accept()
            with channel:
                channel.settimeout(1)
                assert channel.recv(1) == b"R"
            yield
        finally:
            if process.poll() is None:
                process.terminate()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=3)


def check_managed_service(root, executable, sdk, environment):
    for mode in ("pending", "loaded", "changing"):
        with simulated_manager(root, mode, sdk, environment):
            subprocess.run([str(executable), "--exact", "integrations::gnome::tests::native_managed_service_fixture_entry",
                            "--test-threads=1"], env=dict(environment, LYN_PRIVATE_MANAGED_DEFINITION=mode),
                           check=True, timeout=4)
    print("PASS: private simulated manager API distinguishes pending/loaded definitions and refuses changed properties; no service action", flush=True)


def run(module, preserved, executable, sdk):
    require_owned_display(sdk)
    private_runtime = Path(os.environ["XDG_RUNTIME_DIR"])
    assert private_runtime.name == "runtime" and private_runtime.parent.name.startswith("lyn-gnome-run-")
    assert os.environ["DBUS_SESSION_BUS_ADDRESS"].startswith(f"unix:path={private_runtime}/")
    with tempfile.TemporaryDirectory(prefix="lyn-na-") as directory:
        root = Path(directory)
        project = repositories(root, ("alpha",))[0]
        fixture_environment = dict(os.environ, LYN_PRIVATE_NATIVE_ACTIVATION_ROOT=str(root),
                                   LYN_PRIVATE_NATIVE_ACTIVATION_MODULE=str(module))
        with simulated_manager(root, "stock", sdk, fixture_environment):
            subprocess.run([str(executable), "--exact", "integrations::gnome::tests::native_activation_fixture_entry",
                            "--test-threads=1"], env=fixture_environment, check=True, timeout=8)
        import hashlib
        stored_module = root / "state/modules" / hashlib.sha256(module.read_bytes()).hexdigest() / module.name
        assert stored_module.read_bytes() == module.read_bytes()
        assert stored_module.stat().st_mode & 0o7777 == 0o400
        check_managed_service(root, executable, sdk, fixture_environment)
        broker = RustProbe(root, executable, published=True)
        server = None
        pin = None
        app_id = "org.lyn.NativeActivation"
        child = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        child.setsockopt(socket.SOL_SOCKET, socket.SO_PASSCRED, 1)
        child.bind(str(root / "child.sock"))
        child.settimeout(5)
        environment = {name: value for name, value in os.environ.items() if not name.startswith("LYN_GNOME_TEST_")}
        environment.update(XDG_RUNTIME_DIR=str(broker.runtime), GTK3_MODULES=str(preserved), GSETTINGS_BACKEND="keyfile")
        try:
            profile_id = subprocess.check_output(["gsettings", "get", "org.gnome.Terminal.ProfilesList", "default"],
                                                 env=environment, text=True, timeout=3).strip().strip("'")
            profile = f"org.gnome.Terminal.Legacy.Profile:/org/gnome/terminal/legacy/profiles:/:{profile_id}/"
            command = shlex.join(["/usr/bin/python3", str(Path(__file__).with_name("native_child_test.py")),
                                  str(stored_module), str(preserved), str(root / "child.sock")])
            for key, value in (("use-custom-command", "true"), ("custom-command", command)):
                subprocess.run(["gsettings", "set", profile, key, value], env=environment, check=True, timeout=3)
            server = subprocess.Popen([str(root / "state/launcher-v1"), f"--app-id={app_id}"], env=environment,
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            subprocess.run(["gdbus", "wait", "--session", "--timeout", "5", app_id], check=True, timeout=6)
            pin = _pin_owned_app(server, Path("/usr/libexec/gnome-terminal-server"), private_runtime,
                                 expected_cwd=Path.home())
            broker.handshake(server)
            maps = Path(f"/proc/{server.pid}/maps").read_text()
            assert str(stored_module) in maps and str(module) not in maps and str(preserved) in maps
            client = "/usr/bin/gnome-terminal.real" if Path("/usr/bin/gnome-terminal.real").is_file() else "gnome-terminal"
            subprocess.run([client, f"--app-id={app_id}", "--window", "--title=Lyn Native Activation",
                            f"--working-directory={project}", "--", "/usr/bin/sleep", "40"], check=True, timeout=5)
            deadline = time.monotonic() + 5
            window = None
            while time.monotonic() < deadline:
                result = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", "^Lyn Native Activation$"],
                                        capture_output=True, text=True, timeout=1)
                candidates = result.stdout.split()
                if len(candidates) == 1:
                    window = int(candidates[0])
                    break
                assert server.poll() is None
                time.sleep(.01)
            assert window is not None, "Private native window did not map"
            # Explicit settled-widget fixture, not a cold-first-invoke latency claim.
            time.sleep(.1)
            metadata = broker.window_query(window)
            assert metadata["status"] == "candidate" and metadata["serverPid"] == server.pid, metadata
            result = broker.capture_probe(window, project)
            assert result == {"status": "saved", "bodyMatches": True, "projectMatches": True,
                              "branchMatches": True, "duplicateRefused": True, "captureCount": 1,
                              "leaseReleased": True}, result
            print("PASS: generated wrapper, native discovery and production-policy admission -> exact backend save", flush=True)
            subprocess.run(["xdotool", "windowfocus", "--sync", str(window)], check=True, timeout=2)
            time.sleep(.1)
            prop = subprocess.check_output(["xprop", "-id", str(window), "_GTK_WINDOW_OBJECT_PATH"], text=True, timeout=2)
            path = re.fullmatch(r'_GTK_WINDOW_OBJECT_PATH\(UTF8_STRING\) = "([^"]+)"\n?', prop)
            assert path, prop
            subprocess.run(["gdbus", "call", "--session", "--dest", app_id, "--object-path", path[1],
                            "--method", "org.gtk.Actions.Activate", "new-terminal", "[<('tab', 'current')>]", "{}"],
                           check=True, timeout=3, stdout=subprocess.DEVNULL)
            payload, ancillary, flags, _ = child.recvmsg(512, socket.CMSG_SPACE(12))
            assert not flags & (socket.MSG_TRUNC | socket.MSG_CTRUNC)
            credentials = [struct.unpack("3i", data) for level, kind, data in ancillary
                           if level == socket.SOL_SOCKET and kind == socket.SCM_CREDENTIALS]
            assert len(credentials) == 1 and credentials[0][0] > 0 and credentials[0][1] == os.getuid()
            assert json.loads(payload) == {"childModuleAbsent": True, "existingModulePreserved": True,
                                           "moduleEnvironmentAbsent": True}, payload
            print("PASS: actual GTK new-tab child excludes Lyn and preserves the existing module", flush=True)
        finally:
            child.close()
            if server is not None:
                stop_server(server, pin)
            broker.close()


if __name__ == "__main__":
    run(*(Path(value).resolve(strict=True) for value in sys.argv[1:4]),
        Path(sys.argv[4]).resolve(strict=True) if sys.argv[4] != "-" else None)
