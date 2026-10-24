"""Stored native bundle -> generated wrapper -> admitted broker -> backend save."""

import json
from contextlib import contextmanager
import os
import select
from pathlib import Path
import re
import shlex
import socket
import struct
import subprocess
import sys
import tempfile
import time
import uuid

from desktop_isolation import require_owned_display
from desktop_lifecycle import _pin_owned_app
from fixtures import repositories
from resource_test import stop_server
from rust_probe import RustProbe


@contextmanager
def simulated_manager(root, mode, sdk, environment):
    ready = root / f"manager-{mode}-{uuid.uuid4().hex}.sock"
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


def check_synchronization(root, executable, sdk, environment, mode, expected, expected_calls):
    from gi.repository import Gio, GLib
    with simulated_manager(root, mode, sdk, environment):
        connection = Gio.DBusConnection.new_for_address_sync(environment["DBUS_SESSION_BUS_ADDRESS"],
            Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, None, None)
        connection.set_exit_on_close(False)
        try:
            reply = connection.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetNameOwner",
                GLib.Variant("(s)", ("org.freedesktop.systemd1",)), None, Gio.DBusCallFlags.NO_AUTO_START, 500, None)
            original_owner = reply.unpack()[0]
            action = "acknowledge-remove" if mode.startswith("ack-") else "synchronize-with-change" if mode == "sync-during-validation" else "synchronize"
            sync_environment = dict(environment, LYN_PRIVATE_NATIVE_FILE_ACTION=action, LYN_PRIVATE_SYNC_EXPECTED=expected)
            started = time.monotonic()
            for _ in range(1 if expected == "refused" else 2):
                subprocess.run([str(executable), "--exact", "integrations::gnome::tests::native_lifecycle_fixture_entry",
                                "--test-threads=1"], env=sync_environment, check=True, timeout=8)
            if mode == "sync-hang":
                assert 1.5 <= time.monotonic() - started <= 4, "Expected the separate two-second Reload cap"
            if mode == "ack-after-unlink":
                assert not (root / "state/uninstall-retired-v1.json").exists()
                subprocess.run([str(executable), "--exact", "integrations::gnome::tests::native_lifecycle_fixture_entry",
                                "--test-threads=1"], env=dict(sync_environment, LYN_PRIVATE_SYNC_EXPECTED="stock"), check=True, timeout=8)
            def property_value(name):
                reply = connection.call_sync(original_owner, "/org/freedesktop/systemd1", "org.freedesktop.DBus.Properties", "Get",
                    GLib.Variant("(ss)", ("org.freedesktop.systemd1.Manager", name)), None, Gio.DBusCallFlags.NO_AUTO_START, 500, None)
                return reply.get_child_value(0).get_variant().unpack()
            observed_calls = property_value("ReloadCount")
            assert observed_calls == expected_calls, (mode, observed_calls, expected_calls)
            if expected != "refused" and expected_calls:
                assert property_value("PostReloadSameClient"), mode
            if mode.startswith("ack-") and (expected != "refused" or mode == "ack-after-unlink"):
                assert property_value("PostAckSameClient"), mode
        finally:
            connection.close_sync(None)


def run(module, preserved, executable, sdk, updated_module):
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
        for mode, calls in (("sync-during-validation", 0), ("sync-changed", 0), ("sync-owner", 0), ("sync-stuck", 1), ("sync-error", 1), ("sync-hang", 1)):
            check_synchronization(root, executable, sdk, fixture_environment, mode, "refused", calls)
        check_synchronization(root, executable, sdk, fixture_environment, "sync-install", "managed", 1)
        print("PASS: guarded private manager synchronization refuses changed/owner-swapped/stuck/error/silent cases; one Reload and same-client postflight, settled retry skips Reload", flush=True)
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
            def launch_owned(app_id, title, expected, excluded):
                process = subprocess.Popen([str(root / "state/launcher-v1"), f"--app-id={app_id}"], env=environment,
                                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                owned_pin = None
                try:
                    subprocess.run(["gdbus", "wait", "--session", "--timeout", "5", app_id], check=True, timeout=6)
                    owned_pin = _pin_owned_app(process, Path("/usr/libexec/gnome-terminal-server"), private_runtime,
                                               expected_cwd=Path.home())
                    broker.handshake(process)
                    maps = Path(f"/proc/{process.pid}/maps").read_text()
                    assert str(expected) in maps and str(excluded) not in maps and str(preserved) in maps
                    subprocess.run([client, f"--app-id={app_id}", "--window", f"--title={title}",
                                    f"--working-directory={project}", "--", "/usr/bin/sleep", "40"], check=True, timeout=5)
                    deadline = time.monotonic() + 5
                    while time.monotonic() < deadline:
                        windows = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", f"^{re.escape(title)}$"],
                                                 capture_output=True, text=True, timeout=1).stdout.split()
                        if len(windows) == 1:
                            time.sleep(.1)  # Existing settled-widget contract.
                            return process, owned_pin, int(windows[0])
                        assert process.poll() is None
                        time.sleep(.01)
                    raise AssertionError("Private replacement window did not map")
                except BaseException:
                    stop_server(process, owned_pin)
                    raise
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
            # The coordinator changes files; it must not restart our server.
            original_launcher = (root / "state/launcher-v1").read_bytes()
            preparation_environment = dict(fixture_environment, LYN_PRIVATE_NATIVE_ACTIVATION_MODULE=str(updated_module),
                                           LYN_PRIVATE_NATIVE_FILE_ACTION="interrupt-update-precommit")
            with simulated_manager(root, "loaded", sdk, preparation_environment):
                interrupted = subprocess.run([str(executable), "--exact", "integrations::gnome::tests::native_lifecycle_fixture_entry",
                                "--test-threads=1"], env=preparation_environment, timeout=8)
                assert interrupted.returncode == 77, "Expected actual precommit process interruption"
                assert (root / "state/update-precommit-pending-v1.json").is_file()
                restored_environment = dict(preparation_environment, LYN_PRIVATE_NATIVE_FILE_ACTION="recover-update-precommit",
                                            LYN_PRIVATE_NATIVE_ACTIVATION_MODULE=str(root / "absent-bundle.so"))
                subprocess.run([str(executable), "--exact", "integrations::gnome::tests::native_lifecycle_fixture_entry",
                                "--test-threads=1"], env=restored_environment, check=True, timeout=8)
            assert (root / "state/launcher-v1").read_bytes() == original_launcher
            assert not (root / "state/update-precommit-pending-v1.json").exists()
            alive = select.poll()
            alive.register(pin, select.POLLIN)
            assert not alive.poll(0) and server.poll() is None
            print("PASS: uncommitted writer exit recovered in a fresh Rust process to the original factory; existing private server unchanged", flush=True)
            action_environment = dict(fixture_environment, LYN_PRIVATE_NATIVE_ACTIVATION_MODULE=str(updated_module),
                                      LYN_PRIVATE_NATIVE_FILE_ACTION="interrupt-update-commit")
            with simulated_manager(root, "loaded", sdk, action_environment):
                interrupted = subprocess.run([str(executable), "--exact", "integrations::gnome::tests::native_lifecycle_fixture_entry",
                                "--test-threads=1"], env=action_environment, timeout=8)
                assert interrupted.returncode == 77, "Expected actual committed-update process interruption"
                assert (root / "state/update-cleanup-pending-v1.json").is_file()
                recovered_environment = dict(action_environment, LYN_PRIVATE_NATIVE_FILE_ACTION="recover-update-commit",
                                             LYN_PRIVATE_NATIVE_ACTIVATION_MODULE=str(root / "absent-bundle.so"))
                subprocess.run([str(executable), "--exact", "integrations::gnome::tests::native_lifecycle_fixture_entry",
                                "--test-threads=1"], env=recovered_environment, check=True, timeout=8)
                assert not (root / "state/update-cleanup-pending-v1.json").exists()
                print("PASS: committed update process exit recovered in a fresh Rust process without an input bundle", flush=True)
            import hashlib
            updated_stored = root / "state/modules" / hashlib.sha256(updated_module.read_bytes()).hexdigest() / module.name
            assert updated_stored != stored_module and updated_stored.read_bytes() == updated_module.read_bytes()
            assert stored_module.read_bytes() == module.read_bytes()
            poller = select.poll()
            poller.register(pin, select.POLLIN)
            assert not poller.poll(0) and server.poll() is None
            maps = Path(f"/proc/{server.pid}/maps").read_text()
            assert str(stored_module) in maps and str(updated_stored) not in maps
            # Restart only this explicitly owned private fixture to prove the
            # new file is loaded on a subsequent server start. No user service.
            stop_server(server, pin)
            server, pin = None, None
            server, pin, updated_window = launch_owned("org.lyn.NativeUpdated", "Lyn Native Updated", updated_stored, stored_module)
            result = broker.capture_probe(updated_window, project)
            assert result == {"status": "saved", "bodyMatches": True, "projectMatches": True,
                              "branchMatches": True, "duplicateRefused": True, "captureCount": 1,
                              "leaseReleased": True}, result
            print("PASS: distinct real ELF update preserves the live old server; owned private relaunch loads the new digest and saves exactly", flush=True)
            action_environment["LYN_PRIVATE_NATIVE_FILE_ACTION"] = "rollback"
            # Rollback must use persisted ownership/history, never this input.
            action_environment["LYN_PRIVATE_NATIVE_ACTIVATION_MODULE"] = str(root / "absent-bundle.so")
            with simulated_manager(root, "loaded", sdk, action_environment):
                subprocess.run([str(executable), "--exact", "integrations::gnome::tests::native_lifecycle_fixture_entry",
                                "--test-threads=1"], env=action_environment, check=True, timeout=8)
            poller = select.poll()
            poller.register(pin, select.POLLIN)
            assert not poller.poll(0) and server.poll() is None
            assert str(updated_stored) in Path(f"/proc/{server.pid}/maps").read_text()
            stop_server(server, pin)
            server, pin = None, None
            server, pin, restored_window = launch_owned("org.lyn.NativeRestored", "Lyn Native Restored", stored_module, updated_stored)
            result = broker.capture_probe(restored_window, project)
            assert result == {"status": "saved", "bodyMatches": True, "projectMatches": True,
                              "branchMatches": True, "duplicateRefused": True, "captureCount": 1,
                              "leaseReleased": True}, result
            print("PASS: persisted-history rollback needs no artifact input, preserves the live server and loads/restores exact capture on owned relaunch", flush=True)
            action_environment["LYN_PRIVATE_NATIVE_FILE_ACTION"] = "interrupt-remove"
            with simulated_manager(root, "loaded", sdk, action_environment):
                interrupted = subprocess.run([str(executable), "--exact", "integrations::gnome::tests::native_lifecycle_fixture_entry",
                                              "--test-threads=1"], env=action_environment, timeout=8)
                assert interrupted.returncode == 77, interrupted.returncode
                assert (root / "state/uninstall-pending-v1.json").is_file()
                assert not (root / "units/gnome-terminal-server.service.d/90-lyn-native.conf").exists()
                assert (root / "state/launcher-v1").is_file()
                action_environment["LYN_PRIVATE_NATIVE_FILE_ACTION"] = "recover-remove"
                subprocess.run([str(executable), "--exact", "integrations::gnome::tests::native_lifecycle_fixture_entry",
                                "--test-threads=1"], env=action_environment, check=True, timeout=8)
                action_environment["LYN_PRIVATE_NATIVE_FILE_ACTION"] = "inspect-retired"
                subprocess.run([str(executable), "--exact", "integrations::gnome::tests::native_lifecycle_fixture_entry",
                                "--test-threads=1"], env=action_environment, check=True, timeout=8)
            assert not (root / "state/launcher-v1").exists()
            assert not (root / "state/installation-v1.json").exists()
            assert not (root / "units/gnome-terminal-server.service.d/90-lyn-native.conf").exists()
            assert stored_module.read_bytes() == module.read_bytes() and updated_stored.read_bytes() == updated_module.read_bytes()
            poller = select.poll()
            poller.register(pin, select.POLLIN)
            assert not poller.poll(0) and server.poll() is None
            assert (root / "state/uninstall-retired-v1.json").is_file()
            check_synchronization(root, executable, sdk, action_environment, "sync-remove", "stock", 1)
            assert (root / "state/uninstall-retired-v1.json").is_file()
            assert not poller.poll(0) and server.poll() is None
            print("PASS: actual removal-process exit recovered by a fresh Rust process; both modules and exact cached-manager authority retained; live private server unchanged", flush=True)
            for mode, calls in (("ack-owner", 0), ("ack-stuck", 1)):
                check_synchronization(root, executable, sdk, action_environment, mode, "refused", calls)
                assert (root / "state/uninstall-retired-v1.json").is_file()
            check_synchronization(root, executable, sdk, action_environment, "ack-remove", "stock", 1)
            assert not (root / "state/uninstall-retired-v1.json").exists()
            for mode, expected, calls in (("ack-stock", "stock", 0), ("ack-after-unlink", "refused", 1)):
                reset_environment = dict(fixture_environment, LYN_PRIVATE_NATIVE_FILE_ACTION="prepare-again")
                with simulated_manager(root, "stock", sdk, reset_environment):
                    subprocess.run([str(executable), "--exact", "integrations::gnome::tests::native_lifecycle_fixture_entry",
                                    "--test-threads=1"], env=reset_environment, check=True, timeout=8)
                reset_environment["LYN_PRIVATE_NATIVE_FILE_ACTION"] = "remove"
                with simulated_manager(root, "loaded", sdk, reset_environment):
                    subprocess.run([str(executable), "--exact", "integrations::gnome::tests::native_lifecycle_fixture_entry",
                                    "--test-threads=1"], env=reset_environment, check=True, timeout=8)
                check_synchronization(root, executable, sdk, action_environment, mode, expected, calls)
                assert not (root / "state/uninstall-retired-v1.json").exists()
            assert stored_module.read_bytes() == module.read_bytes() and updated_stored.read_bytes() == updated_module.read_bytes()
            assert not poller.poll(0) and server.poll() is None
            print("PASS: stock-verified retirement acknowledgement, same-client post-unlink check, no-RPC fresh retry and private reinstall preserve modules/live server", flush=True)
        finally:
            child.close()
            if server is not None:
                stop_server(server, pin)
            broker.close()


if __name__ == "__main__":
    run(*(Path(value).resolve(strict=True) for value in sys.argv[1:4]),
        Path(sys.argv[4]).resolve(strict=True) if sys.argv[4] != "-" else None,
        Path(sys.argv[5]).resolve(strict=True))
