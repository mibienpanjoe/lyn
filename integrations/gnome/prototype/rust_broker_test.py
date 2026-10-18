"""Prove actual GTK/VTE producer → authenticated Rust broker, on private Xvfb/D-Bus."""

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
from rust_probe import RustProbe


def run(module, executable):
    assert Path(os.environ["XDG_RUNTIME_DIR"]).parent.name.startswith("lyn-gnome-run-")
    with tempfile.TemporaryDirectory(prefix="lyn-gnome-rust-") as directory:
        projects = repositories(directory, ("alpha", "beta", "gamma"))
        endpoint = str(Path(directory) / "diagnostic.sock")
        channel = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        channel.setsockopt(socket.SOL_SOCKET, socket.SO_PASSCRED, 1)
        channel.bind(endpoint)
        channel.settimeout(.25)
        probe = RustProbe(directory, executable)
        app_id = "org.lyn.NativeRust"
        server = None
        children = {}

        def snapshot(predicate):
            after_us = time.monotonic_ns() // 1000
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                try:
                    payload, ancillary, flags, _ = channel.recvmsg(8192, socket.CMSG_SPACE(12))
                except socket.timeout:
                    continue
                assert not flags & (socket.MSG_TRUNC | socket.MSG_CTRUNC)
                credentials = [struct.unpack("3i", data) for level, kind, data in ancillary
                               if level == socket.SOL_SOCKET and kind == socket.SCM_CREDENTIALS]
                assert credentials == [(server.pid, os.getuid(), os.getgid())]
                frame = json.loads(payload)
                if frame["kind"] == "snapshot":
                    sources = frame["sources"]
                    if sources and all(item["observedAtUs"] >= after_us for item in sources) and predicate(sources):
                        return sources
            raise AssertionError("Native diagnostic sources did not settle")

        try:
            environment = dict(os.environ, GTK3_MODULES=str(module),
                               LYN_GNOME_TEST_SOCKET=endpoint, LYN_GNOME_TEST_STREAM=probe.path,
                               GSETTINGS_BACKEND="keyfile", NO_AT_BRIDGE="1")
            # GTK new-tab actions must never start the user's shell/profile.
            profile_id = subprocess.check_output(["gsettings", "get", "org.gnome.Terminal.ProfilesList", "default"],
                                                 env=environment, text=True, timeout=3).strip().strip("'")
            profile = f"org.gnome.Terminal.Legacy.Profile:/org/gnome/terminal/legacy/profiles:/:{profile_id}/"
            fixture_command = shlex.join(["/usr/bin/env", "-C", str(projects[0]), "/usr/bin/sleep", "60"])
            for key, value in (("use-custom-command", "true"), ("custom-command", fixture_command)):
                subprocess.run(["gsettings", "set", profile, key, value], env=environment, check=True, timeout=3)
            server = subprocess.Popen(["/usr/libexec/gnome-terminal-server", f"--app-id={app_id}"],
                                      env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            probe.handshake(server)
            print("PASS: actual GNOME server admitted by production Rust peer/stream/manager policy", flush=True)
            subprocess.run(["gdbus", "wait", "--session", "--timeout", "5", app_id], check=True, timeout=6)
            client = "/usr/bin/gnome-terminal.real" if Path("/usr/bin/gnome-terminal.real").is_file() else "gnome-terminal"
            for index, project in enumerate(projects):
                command = ["/usr/bin/sleep", "60"] if index < 2 else ["/usr/bin/htop"]
                subprocess.run([client, f"--app-id={app_id}", "--window",
                                f"--role=lyn-rust-{project.name}", f"--working-directory={project}",
                                "--", *command], check=True, timeout=5,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            sources = snapshot(lambda items: len(items) == 3 and all(item["active"] for item in items))
            assert len({item["windowId"] for item in sources}) == 3
            generations = set()
            for source in sources:
                result = probe.query(server.pid, source["windowId"])
                assert result["status"] == "candidate", result
                assert result["serverPid"] == server.pid
                verified = probe.window_query(source["windowId"])
                assert verified["status"] == "candidate" and verified["serverPid"] == server.pid, verified
                assert verified["sourceId"] == result["sourceId"]
                pty_verified = probe.pty_query(source["windowId"])
                assert pty_verified["status"] == "candidate", pty_verified
                assert pty_verified["sourceId"] == result["sourceId"]
                for field in ("sourceId", "ptyNumber", "foregroundPgid", "revision", "registeredAtUs", "changedAtUs"):
                    assert result[field] == source[field], (field, result, source)
                generations.add(result["generation"])
                pid = result["foregroundPgid"]
                pidfd = os.pidfd_open(pid)
                try:
                    fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
                    assert int(fields[1]) == server.pid
                    assert int(fields[4]) == os.stat(f'/dev/pts/{result["ptyNumber"]}').st_rdev
                except BaseException:
                    os.close(pidfd)
                    raise
                children[pid] = pidfd
                project = Path(f"/proc/{pid}/cwd").resolve()
                process_verified = probe.process_query(source["windowId"], project)
                assert process_verified == {"status": "candidate", "sourceId": source["sourceId"],
                                            "directoryMatches": True, "stillCurrent": True}, process_verified
                saved = probe.capture_probe(source["windowId"], project)
                assert saved == {"status": "saved", "bodyMatches": True, "projectMatches": True,
                                 "branchMatches": True, "duplicateRefused": True, "captureCount": 1,
                                 "leaseReleased": True}, saved
                role = subprocess.check_output(["xprop", "-id", str(source["windowId"]), "WM_WINDOW_ROLE"],
                                               text=True, timeout=3)
                assert f'"lyn-rust-{project.name}"' in role, "Native window does not match its process/project"
            assert len(generations) == 1
            assert {Path(f'/proc/{source["foregroundPgid"]}/cwd').resolve() for source in sources} == set(projects)
            print("PASS: each native window/TUI saves once to its own project/branch, Library reads it and its source is released", flush=True)
            assert any(Path(f'/proc/{source["foregroundPgid"]}/exe').resolve() == Path("/usr/bin/htop") for source in sources)
            print("PASS: Rust receives exact window/PTy/process metadata for two projects and a real TUI", flush=True)
            print("PASS: Rust attests owned PTY, foreground group ancestry and unique cwd for all three windows", flush=True)
            window = sources[0]["windowId"]
            try:
                subprocess.run(["xprop", "-id", str(window), "-f", "_NET_WM_PID", "32c",
                                "-set", "_NET_WM_PID", str(os.getpid())], check=True, timeout=3,
                               stdout=subprocess.DEVNULL)
                assert probe.window_query(window)["serverPid"] == server.pid
            finally:
                subprocess.run(["xprop", "-id", str(window), "-f", "_NET_WM_PID", "32c",
                                "-set", "_NET_WM_PID", str(server.pid)], check=True, timeout=3,
                               stdout=subprocess.DEVNULL)
            import gi
            gi.require_version("Gtk", "3.0")
            from gi.repository import Gtk
            foreign = Gtk.Window()
            try:
                foreign.show_all()
                while Gtk.events_pending():
                    Gtk.main_iteration_do(False)
                foreign_id = foreign.get_window().get_xid()
                subprocess.run(["xprop", "-id", str(foreign_id), "-f", "_NET_WM_PID", "32c",
                                "-set", "_NET_WM_PID", str(server.pid)], check=True, timeout=3,
                               stdout=subprocess.DEVNULL)
                assert probe.window_query(foreign_id)["status"] == "unavailable"
            finally:
                foreign.destroy()
            assert probe.window_query(foreign_id)["status"] == "unavailable"
            assert probe.window_query(0xfffffffe)["status"] == "unavailable"
            print("PASS: XRes proves real window ownership; spoofed _NET_WM_PID and foreign windows cannot route to GNOME", flush=True)
            xvfb_fds = []
            try:
                for entry in Path("/proc").iterdir():
                    if not entry.name.isdecimal():
                        continue
                    try:
                        text = (entry / "stat").read_text()
                        fields = text.rsplit(")", 1)[1].split()
                        if text.split("(", 1)[1].rsplit(")", 1)[0] != "Xvfb" or int(fields[2]) != os.getpgrp():
                            continue
                        fd = os.pidfd_open(int(entry.name))
                        try:
                            current = (entry / "stat").read_text().rsplit(")", 1)[1].split()
                            assert int(current[2]) == os.getpgrp() and int(current[3]) == os.getsid(0)
                        except BaseException:
                            os.close(fd)
                            raise
                        xvfb_fds.append(fd)
                    except FileNotFoundError:
                        continue
                assert len(xvfb_fds) == 1, "Expected exactly one Xvfb in our private fixture session"
                signal.pidfd_send_signal(xvfb_fds[0], signal.SIGSTOP)
                started = time.monotonic()
                assert probe.window_query(window)["status"] == "unavailable"
                assert time.monotonic() - started < .5, "Stopped X server blocked the ownership worker"
            finally:
                for fd in xvfb_fds:
                    try:
                        signal.pidfd_send_signal(fd, signal.SIGCONT)
                    finally:
                        os.close(fd)
            assert probe.window_query(window)["status"] == "candidate"
            print("PASS: stopped private X server times out safely; recovery keeps the native channel usable", flush=True)
            assert probe.query(os.getpid(), sources[0]["windowId"])["status"] == "unavailable"
            assert probe.query(server.pid, 0xfffffffe)["status"] == "missing"
            assert probe.query(server.pid, sources[0]["windowId"])["status"] == "candidate"
            print("PASS: unknown PID never falls back; unknown window stays unresolved without poisoning channel", flush=True)
            previous = sources[0]
            old_ids = {item["sourceId"] for item in sources}
            window_path = subprocess.check_output(["xprop", "-id", str(previous["windowId"]),
                                                   "_GTK_WINDOW_OBJECT_PATH"], text=True, timeout=3)
            object_path = re.search(r'= "([^"]+)"', window_path).group(1)
            subprocess.run(["gdbus", "call", "--session", "--dest", app_id,
                            "--object-path", object_path, "--method", "org.gtk.Actions.Activate",
                            "new-terminal", "[<('tab', 'current')>]", "{}"], check=True, timeout=3,
                           stdout=subprocess.DEVNULL)
            sources = snapshot(lambda items: len(items) == 4 and any(item["sourceId"] not in old_ids and
                                                                     item["active"] for item in items))
            source = next(item for item in sources if item["sourceId"] not in old_ids)
            assert source["windowId"] == previous["windowId"]
            # Fresh topology timestamp: unrelated checks above must not consume
            # this invocation's 120ms budget before equality is exercised.
            assert probe.query(server.pid, source["windowId"], source["changedAtUs"])["status"] == "changed"
            assert probe.query(server.pid, source["windowId"])["status"] == "candidate"
            pidfd = os.pidfd_open(source["foregroundPgid"])
            try:
                fields = Path(f'/proc/{source["foregroundPgid"]}/stat').read_text().rsplit(")", 1)[1].split()
                assert int(fields[1]) == server.pid
                assert int(fields[4]) == os.stat(f'/dev/pts/{source["ptyNumber"]}').st_rdev
            except BaseException:
                os.close(pidfd)
                raise
            children[source["foregroundPgid"]] = pidfd
            frozen = source
            destination = next(item["windowId"] for item in sources if item["windowId"] != frozen["windowId"])
            subprocess.run(["xdotool", "windowfocus", "--sync", str(frozen["windowId"])], check=True, timeout=3)
            snapshot(lambda items: any(item["windowId"] == frozen["windowId"] and item["windowFocused"] for item in items))
            invoked_us = time.monotonic_ns() // 1000
            subprocess.run(["xdotool", "windowfocus", "--sync", str(destination)], check=True, timeout=3)
            snapshot(lambda items: any(item["windowId"] == destination and item["windowFocused"] for item in items)
                     and any(item["windowId"] == frozen["windowId"] and not item["windowFocused"] for item in items))
            retained = probe.query(server.pid, frozen["windowId"], invoked_us)
            assert retained["status"] == "candidate" and retained["sourceId"] == frozen["sourceId"], retained
            print("PASS: actual Rust query keeps the frozen window after OS focus moves elsewhere", flush=True)
            closed = next(item for item in sources if item["windowId"] != frozen["windowId"])
            assert probe.window_query(closed["windowId"])["status"] == "candidate"
            signal.pidfd_send_signal(children[closed["foregroundPgid"]], signal.SIGTERM)
            snapshot(lambda items: all(item["sourceId"] != closed["sourceId"] for item in items))
            # Widget destruction can precede the asynchronous XDestroyWindow.
            # Establish the actual X11 precondition for the owner-refusal test.
            deadline = time.monotonic() + 3
            while subprocess.run(["xdotool", "getwindowgeometry", str(closed["windowId"])],
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                 timeout=1).returncode == 0:
                assert time.monotonic() < deadline, "Fixture X11 window did not close"
                time.sleep(.005)
            assert server.poll() is None
            closed_result = probe.window_query(closed["windowId"])
            assert closed_result["status"] == "unavailable", closed_result
            assert probe.process_query(closed["windowId"], projects[0])["status"] == "unavailable"
            assert probe.capture_probe(closed["windowId"], projects[0])["status"] == "unavailable"
            assert probe.window_query(frozen["windowId"])["status"] == "candidate"
            print("PASS: a previously admitted closed window is refused while its GNOME client stays live", flush=True)
            server.terminate()
            server.wait(timeout=5)
            assert probe.query(server.pid, source["windowId"])["status"] in ("unavailable", "disconnected")
            assert probe.request({"kind": "status"}) == {"peers": []}
            print("PASS: real server exit revokes Rust query/source evidence before heartbeat expiry", flush=True)
        finally:
            if server and server.poll() is None:
                server.terminate()
                try:
                    server.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait(timeout=2)
            # Pin fixture children before cleanup; never signal a recycled PID.
            for pidfd in children.values():
                try:
                    signal.pidfd_send_signal(pidfd, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                finally:
                    os.close(pidfd)
            channel.close()
            probe.close()


if __name__ == "__main__":
    run(Path(sys.argv[1]).resolve(strict=True), Path(sys.argv[2]).resolve(strict=True))
