"""Exercise native window/PTY identity in real GNOME Terminal on a private bus."""

import json
import os
from pathlib import Path
import re
import shlex
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time

from fixtures import repositories


def run(module, preserved_module):
    with tempfile.TemporaryDirectory(prefix="lyn-gnome-identity-") as directory:
        projects = repositories(directory, ("alpha", "beta", "gamma"))
        endpoint = str(Path(directory) / "module.sock")
        child_endpoint = str(Path(directory) / "child.sock")
        child_channel = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        child_channel.bind(child_endpoint)
        child_channel.settimeout(5)
        channel = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        channel.setsockopt(socket.SOL_SOCKET, socket.SO_PASSCRED, 1)
        channel.bind(endpoint)
        channel.settimeout(.25)
        app_id = "org.lyn.NativeIdentity"
        # Never let a GTK "new tab" action run the user's shell startup files.
        config = Path(os.environ["XDG_CONFIG_HOME"])
        assert config.parent.name.startswith("lyn-gnome-run-"), "Use isolated_run.py"
        environment = dict(os.environ, GTK3_MODULES=f"{module}:{preserved_module}",
                           LYN_GNOME_TEST_SOCKET=endpoint,
                           GSETTINGS_BACKEND="keyfile", NO_AT_BRIDGE="1")
        profile_id = subprocess.check_output(
            ["gsettings", "get", "org.gnome.Terminal.ProfilesList", "default"],
            env=environment, text=True).strip().strip("'")
        profile = f"org.gnome.Terminal.Legacy.Profile:/org/gnome/terminal/legacy/profiles:/:{profile_id}/"
        child_fixture = Path(__file__).with_name("child_fixture.py").resolve()
        child_command = shlex.join(["/usr/bin/env", "-C", str(projects[2]), "/usr/bin/python3",
                                    str(child_fixture), str(module), child_endpoint, str(preserved_module)])
        for key, value in (("use-custom-command", "true"), ("custom-command", child_command)):
            subprocess.run(["gsettings", "set", profile, key, value], env=environment, check=True)
        server = subprocess.Popen(["/usr/libexec/gnome-terminal-server", f"--app-id={app_id}"],
                                  env=environment, stdout=subprocess.DEVNULL,
                                  stderr=subprocess.DEVNULL)
        fixture_processes = set()

        def snapshot(predicate, description):
            until = time.monotonic()+5
            last_sources = []
            while time.monotonic() < until:
                try:
                    payload, ancillary, flags, _ = channel.recvmsg(8192, socket.CMSG_SPACE(12))
                except socket.timeout:
                    continue
                assert not flags & socket.MSG_TRUNC
                credentials = [struct.unpack("3i", data) for level, kind, data in ancillary
                               if level == socket.SOL_SOCKET and kind == socket.SCM_CREDENTIALS]
                assert credentials == [(server.pid, os.getuid(), os.getgid())]
                frame = json.loads(payload)
                if frame["kind"] == "snapshot":
                    last_sources = frame["sources"]
                    if predicate(last_sources):
                        return last_sources
            raise AssertionError(f"Native snapshot missing: {description}; identities={last_sources}")

        try:
            subprocess.run(["gdbus", "wait", "--session", "--timeout", "5", app_id], check=True)
            # Pop!_OS wraps gnome-terminal with a per-launch server spawner.
            # Target the already authenticated fixture server with the real client.
            client_binary = "/usr/bin/gnome-terminal.real"
            if not Path(client_binary).is_file():
                client_binary = "gnome-terminal"
            clients = [subprocess.Popen([client_binary, f"--app-id={app_id}", "--window",
                                         f"--role=lyn-fixture-{project.name}",
                                         f"--working-directory={project}", "--",
                                         "/usr/bin/sleep", "60"],
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                       for project in projects[:2]]
            for client in clients:
                assert client.wait(timeout=5) == 0
            sources = snapshot(lambda items: len(items) == 2, "two concurrent windows")
            assert len({item["windowId"] for item in sources}) == 2, sources
            assert len({item["ptyNumber"] for item in sources}) == 2, sources
            assert all(item["active"] for item in sources), sources
            observed_projects = set()
            for item in sources:
                pid = item["foregroundPgid"]
                fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
                assert int(fields[1]) == server.pid, "Foreground fixture not owned by test server"
                assert int(fields[4]) == os.stat(f'/dev/pts/{item["ptyNumber"]}').st_rdev
                fixture_processes.add(pid)
                project = Path(f"/proc/{pid}/cwd").resolve()
                observed_projects.add(project)
                role = subprocess.check_output(["xprop", "-id", str(item["windowId"]),
                                                "WM_WINDOW_ROLE"], text=True)
                assert f'"lyn-fixture-{project.name}"' in role, "Wrong native window for process"
            assert observed_projects == set(projects[:2])
            print("PASS: two concurrent GNOME windows have distinct, correct native PTYs/processes")

            invocation_us = time.monotonic_ns() // 1000
            sources = snapshot(lambda items: len(items) == 2 and
                               all(item["observedAtUs"] >= invocation_us for item in items),
                               "native monotonic observation after invocation")
            for item in sources:
                assert 0 < item["registeredAtUs"] <= item["changedAtUs"] <= invocation_us
                assert item["observedAtUs"] <= time.monotonic_ns() // 1000
                assert item["revision"] >= 1
            print("PASS: native GLib and invocation monotonic clocks are comparable")

            previous = {item["sourceId"] for item in sources}
            target_window = sources[0]["windowId"]
            subprocess.run(["xdotool", "windowfocus", "--sync", str(target_window)], check=True)
            snapshot(lambda items: any(item["windowId"] == target_window and
                                      item["windowFocused"] for item in items),
                     "GTK focus established before requesting a tab")
            prop = subprocess.check_output(["xprop", "-id", str(target_window),
                                            "_GTK_WINDOW_OBJECT_PATH"], text=True)
            object_path = re.search(r'= "([^"]+)"', prop).group(1)
            subprocess.run(["gdbus", "call", "--session", "--dest", app_id,
                            "--object-path", object_path, "--method", "org.gtk.Actions.Activate",
                            "new-terminal", "[<('tab', 'current')>]", "{}"], check=True,
                           stdout=subprocess.DEVNULL)
            assert json.loads(child_channel.recv(512)) == {
                "childModuleAbsent": True, "existingModulePreserved": True}, \
                "Our GTK module propagated to the terminal's GTK child"
            sources = snapshot(lambda items: len(items) == 3 and
                               sum(item["active"] for item in items) == 2 and
                               any(item["sourceId"] not in previous and
                                   Path(f'/proc/{item["foregroundPgid"]}/exe').resolve() ==
                                   Path("/usr/bin/htop") for item in items), "third tab with real TUI")
            third = next(item for item in sources if item["sourceId"] not in previous)
            assert third["active"]
            assert third["windowId"] == target_window
            assert len({item["windowId"] for item in sources}) == 2
            pid = third["foregroundPgid"]
            fixture_processes.add(pid)
            assert Path(f"/proc/{pid}/cwd").resolve() == projects[2]
            assert Path(f"/proc/{pid}/exe").resolve() == Path("/usr/bin/htop")
            same_window = [item for item in sources if item["windowId"] == third["windowId"]]
            assert len(same_window) == 2
            other = next(item for item in same_window if item["sourceId"] != third["sourceId"])
            subprocess.run(["xdotool", "windowfocus", "--sync", str(third["windowId"])], check=True)
            for index in range(8):
                desired = other if index % 2 == 0 else third
                invocation_us = time.monotonic_ns() // 1000
                subprocess.run(["xdotool", "key", "--clearmodifiers",
                                "alt+1" if index % 2 == 0 else "alt+2"], check=True)
                sources = snapshot(lambda items: any(item["sourceId"] == desired["sourceId"]
                                                     and item["active"]
                                                     and item["changedAtUs"] > invocation_us
                                                     for item in items),
                                   "selected native tab")
                selected = [item for item in sources if item["windowId"] == third["windowId"]
                            and item["active"]]
                assert len(selected) == 1 and selected[0]["sourceId"] == desired["sourceId"]
                assert selected[0]["changedAtUs"] > invocation_us, \
                    "A post-invocation tab must never be accepted as the earlier source"
            print("PASS: native active tab changes correctly across eight switches; TUI identity preserved")
            print("PASS: module is not loaded into a GTK application launched from the terminal")

            before = next(item for item in sources if item["sourceId"] == third["sourceId"])
            invocation_us = time.monotonic_ns() // 1000
            subprocess.run(["xdotool", "key", "--delay", "0", "--clearmodifiers",
                            "alt+1", "alt+2"], check=True)
            sources = snapshot(lambda items: any(item["sourceId"] == third["sourceId"]
                                                 and item["active"]
                                                 and item["revision"] >= before["revision"] + 2
                                                 for item in items), "A-B-A topology transition")
            returned = next(item for item in sources if item["sourceId"] == third["sourceId"])
            assert returned["changedAtUs"] > invocation_us
            print("PASS: A-B-A tab transitions invalidate an earlier invocation even with the same final source")

            invocation_us = time.monotonic_ns() // 1000
            background_window = next(item["windowId"] for item in sources
                                     if item["windowId"] != target_window)
            subprocess.run(["xdotool", "windowfocus", "--sync", str(background_window)], check=True)
            sources = snapshot(lambda items: any(item["windowId"] == background_window and
                                                 item["windowFocused"] for item in items) and
                               all(item["observedAtUs"] >= invocation_us for item in items),
                               "loss of foreground after frozen invocation")
            retained = next(item for item in sources if item["sourceId"] == third["sourceId"])
            assert not retained["windowFocused"] and retained["active"]
            assert retained["changedAtUs"] <= invocation_us
            assert retained["revision"] == returned["revision"]
            print("PASS: later OS focus loss does not rewrite the bound terminal topology")

            prop = subprocess.check_output(["xprop", "-id", str(third["windowId"]),
                                            "_GTK_WINDOW_OBJECT_PATH"], text=True)
            object_path = re.search(r'= "([^"]+)"', prop).group(1)
            detach_us = time.monotonic_ns() // 1000
            subprocess.run(["gdbus", "call", "--session", "--dest", app_id,
                            "--object-path", object_path, "--method", "org.gtk.Actions.Activate",
                            "tab-detach", "[]", "{}"], check=True, stdout=subprocess.DEVNULL)
            sources = snapshot(lambda items: len({item["windowId"] for item in items}) == 3,
                               "TUI tab detached to its own window")
            moved = next(item for item in sources if item["sourceId"] == third["sourceId"])
            assert moved["windowId"] != third["windowId"]
            assert moved["ptyNumber"] == third["ptyNumber"]
            assert moved["foregroundPgid"] == pid
            assert moved["changedAtUs"] > detach_us and moved["revision"] > retained["revision"]
            print("PASS: detached tab retains its source/PTY/process while native window changes")

            os.kill(pid, signal.SIGTERM)
            fixture_processes.remove(pid)
            sources = snapshot(lambda items: len(items) == 2 and
                               all(item["sourceId"] != third["sourceId"] for item in items),
                               "closed TUI tab removed")
            assert {item["sourceId"] for item in sources} == previous
            print("PASS: closed native tab disappears without replacing another source")
        finally:
            for pid in fixture_processes:
                try:
                    os.kill(pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
            if server.poll() is None:
                server.terminate()
            try:
                server.wait(timeout=5)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=2)
            channel.close()
            child_channel.close()


if __name__ == "__main__":
    run(Path(sys.argv[1]).resolve(strict=True), Path(sys.argv[2]).resolve(strict=True))
