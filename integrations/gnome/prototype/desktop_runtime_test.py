"""Real Lyn webviews/shortcut on test-owned X11, bus, WM and XDG directories."""

import argparse
import json
import os
from pathlib import Path
import re
import select
import shlex
import socket
import sqlite3
import struct
import subprocess
import sys
import tempfile
import time

from fixtures import repositories
from desktop_isolation import require_owned_display
from desktop_pixels import region_changed, region_has_contrast
from desktop_accessibility import page_heading


def wait_for(check, message, timeout=12):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(.05)
    raise AssertionError(message)


def run(module, executable, sdk, probe=False, concurrent=False, warm=None, alternations=4, relocate=False, race=None, late_app=False, restart_app=False, stale_reconnect=False, closed_source=False):
    root = Path(os.environ["XDG_RUNTIME_DIR"]).parent
    assert root.name.startswith("lyn-gnome-run-")
    assert os.environ["DBUS_SESSION_BUS_ADDRESS"].startswith(f"unix:path={root}/runtime/")
    assert Path(os.environ["XAUTHORITY"]).parent.name.startswith("xvfb-run."), "Use the owned Xvfb display"
    require_owned_display(sdk)
    repo = Path(__file__).resolve().parents[3]
    environment = dict(os.environ, LANG="C.UTF-8", GTK_MODULES="atk-bridge")
    environment.pop("NO_AT_BRIDGE", None)
    environment.pop("GTK_A11Y", None)
    # AT-SPI must use this private session bus, never the user's desktop bus.
    os.environ.update(environment)
    os.environ.pop("NO_AT_BRIDGE", None)
    os.environ.pop("GTK_A11Y", None)
    import gi
    gi.require_version("Atspi", "2.0")
    gi.require_version("Gdk", "3.0")
    from gi.repository import Atspi, Gdk, GLib
    Atspi.set_timeout(1000, 1000)
    initialized, _ = Gdk.init_check([])
    assert initialized, "Owned Xvfb display is unavailable for screenshot"
    display_root = Gdk.get_default_root_window()
    assert display_root is not None

    def pixels():
        Gdk.Display.get_default().sync()
        result = Gdk.pixbuf_get_from_window(display_root, 0, 0,
                                            display_root.get_width(), display_root.get_height())
        assert result is not None, "Could not sample the owned display"
        return result

    def save_screenshot(pixbuf, prefix):
        artifact_root = repo / "tasks"
        assert artifact_root.is_dir() and not artifact_root.is_symlink()
        descriptor, screenshot = tempfile.mkstemp(prefix=prefix, suffix=".png", dir=artifact_root)
        os.close(descriptor)
        assert pixbuf.savev(screenshot, "png", [], [])
        return screenshot

    children = []
    diagnostic = None
    gate_listener = None
    query_gate = None
    lifecycle_listener = None
    lifecycle = None

    def start(command, env=environment, **kwargs):
        process = subprocess.Popen(command, env=env, **kwargs)
        children.append(process)
        return process

    def command(*args):
        return subprocess.check_output(args, env=environment, text=True, timeout=3).strip()

    def visible_windows(pid, class_name=None):
        args = ["xdotool", "search", "--onlyvisible", "--pid", str(pid)]
        if class_name:
            args.extend(["--class", class_name])
        result = subprocess.run(args, env=environment, text=True, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, timeout=3)
        if result.returncode == 1:
            return []
        result.check_returncode()
        return result.stdout.strip().splitlines()

    def nodes(pid):
        for _ in range(32):
            if not GLib.MainContext.default().pending():
                break
            GLib.MainContext.default().iteration(False)
        desktop = Atspi.get_desktop(0)
        found = []
        def walk(node, depth=0):
            if depth > 32 or len(found) >= 4096:
                raise AssertionError("Unbounded fixture accessibility tree")
            found.append(node)
            for index in range(node.get_child_count()):
                child = node.get_child_at_index(index)
                if child:
                    walk(child, depth + 1)
        for index in range(desktop.get_child_count()):
            application = desktop.get_child_at_index(index)
            if application and application.get_process_id() == pid:
                for child_index in range(application.get_child_count()):
                    frame = application.get_child_at_index(child_index)
                    if frame and frame.get_state_set().contains(Atspi.StateType.SHOWING):
                        walk(frame)
        return found

    def named(pid, name):
        try:
            return next((node for node in nodes(pid) if node.get_name() == name), None)
        except GLib.GError as error:
            # WebKit may replace an accessible object during initial loading.
            # Only that defunct-proxy error is a not-ready result; the existing
            # wait deadline still applies, and an exited application must fail.
            if (error.domain != "atspi_error"
                    or error.message != "The application no longer exists"
                    or app.poll() is not None):
                raise
            return None

    def contains(pid, fragment):
        for node in nodes(pid):
            if fragment in node.get_name():
                return True
            if "Text" in node.get_interfaces() and fragment in node.get_text(0, -1):
                return True
        return False

    try:
        wm = (sdk / "root/usr/bin/openbox") if sdk else Path("/usr/bin/openbox")
        wm_env = dict(environment)
        if sdk:
            wm_env["LD_LIBRARY_PATH"] = str(sdk / "root/usr/lib/x86_64-linux-gnu")
            wm_env["XDG_DATA_DIRS"] = str(sdk / "root/usr/share") + ":/usr/share"
        wm_process = start([str(wm), "--sm-disable", "--config-file",
               str(Path(__file__).with_name("desktop-openbox.xml"))], env=wm_env,
              stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        wait_for(lambda: wm_process.poll() is None and "_NET_ACTIVE_WINDOW" in
                 command("xprop", "-root", "_NET_SUPPORTED"), "Openbox EWMH activation not ready")
        # AT-SPI 2.44's libatspi honors AT_SPI_BUS_ADDRESS before X11/session
        # discovery. Avoid its distro launcher, whose broker needs systemd.
        # https://github.com/GNOME/at-spi2-core/blob/AT_SPI2_CORE_2_44_0/atspi/atspi-misc.c
        bus_address = f'unix:path={root / "runtime/desktop-a11y.sock"}'
        bus_env = dict(environment, AT_SPI_BUS_ADDRESS=bus_address)
        bus = start(["/usr/bin/dbus-daemon", "--nofork", "--print-address",
                     f'--config-file={Path(__file__).with_name("desktop-a11y.conf")}',
                     f"--address={bus_address}"], env=bus_env, stdout=subprocess.PIPE, text=True)
        assert select.select([bus.stdout], [], [], 5)[0], "Private accessibility bus not ready"
        address = bus.stdout.readline().strip()
        assert address.startswith(bus_address) and bus.poll() is None
        environment["AT_SPI_BUS_ADDRESS"] = address
        os.environ["AT_SPI_BUS_ADDRESS"] = address
        with socket.socket() as reserve:
            reserve.bind(("127.0.0.1", 0))
            port = reserve.getsockname()[1]
        server_command = ([sys.executable, "-u", "-m", "http.server", str(port),
                           "--bind", "127.0.0.1", "--directory",
                           str(Path(__file__).with_name("a11y-probe"))] if probe else
                          ["node", str(repo / "node_modules/vite/bin/vite.js"), "--host", "127.0.0.1",
                           "--port", str(port), "--strictPort"])
        vite = start(server_command, cwd=repo, stdout=subprocess.PIPE,
                     stderr=subprocess.STDOUT, text=True)
        deadline = time.monotonic() + 5
        vite_output = ""
        while True:
            assert vite.poll() is None, "Owned Vite process exited"
            assert time.monotonic() < deadline, "Owned Vite did not become ready"
            if select.select([vite.stdout], [], [], .2)[0]:
                vite_output += os.read(vite.stdout.fileno(), 8192).decode()
                if (f"port {port}" if probe else f"127.0.0.1:{port}") in vite_output:
                    break
        if race:
            gate_listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            gate_listener.bind(str(root / "runtime/native-query-gate.sock"))
            os.chmod(root / "runtime/native-query-gate.sock", 0o600)
            gate_listener.listen(1)
            gate_listener.settimeout(3)
        if restart_app or stale_reconnect:
            lifecycle_listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            lifecycle_listener.bind(str(root / "runtime/desktop-lifecycle.sock"))
            os.chmod(root / "runtime/desktop-lifecycle.sock", 0o600)
            lifecycle_listener.listen(1)
            lifecycle_listener.settimeout(15)

        def start_app():
            return start([str(executable), "--exact", "desktop_tests::real_gnome_desktop_entry",
                          "--nocapture", "--test-threads=1"],
                         env=dict(environment, LYN_GNOME_UI_FIXTURE="1", LYN_GNOME_UI_PORT=str(port),
                                  LYN_GNOME_UI_QUERY_GATE="1" if race else "0",
                                  LYN_GNOME_UI_RESTART="1" if restart_app or stale_reconnect else "0"))

        def lifecycle_connection():
            connection, _ = lifecycle_listener.accept()
            assert struct.unpack("3i", connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)) == (
                app.pid, os.getuid(), os.getgid())
            connection.settimeout(1)
            return connection

        app = None if late_app else start_app()
        if restart_app or stale_reconnect:
            lifecycle = lifecycle_connection()
        if race:
            query_gate, _ = gate_listener.accept()
            assert struct.unpack("3i", query_gate.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)) == (
                app.pid, os.getuid(), os.getgid())
            query_gate.settimeout(1)
        endpoint = Path(environment["XDG_RUNTIME_DIR"]) / "lyn-gnome-v1.sock"
        if not late_app:
            wait_for(lambda: endpoint.exists() and app.poll() is None, "Lyn native endpoint not ready", 15)
        if probe:
            action = wait_for(lambda: named(app.pid, "Lyn probe action"), "Probe button not accessible", 15)
            heading = wait_for(lambda: named(app.pid, "Lyn probe heading"), "Probe heading not accessible")
            assert page_heading([heading], "Lyn probe heading") is heading, "Probe HTML heading attributes differ"
            observations = []
            for node in (heading, action):
                observations.append({"name": node.get_name(), "role": int(node.get_role()),
                                     "roleName": node.get_role_name(), "attributes": node.get_attributes()})
            assert action.do_action(0)
            wait_for(lambda: contains(app.pid, "Lyn probe activated"), "Probe action did not update text")
            print(json.dumps({"check": "webkit_accessibility_probe", "nodes": observations,
                              "expectedHeadingRole": int(Atspi.Role.HEADING),
                              "headingRoleMatches": heading.get_role() == Atspi.Role.HEADING,
                              "expectedButtonRole": int(Atspi.Role.PUSH_BUTTON)}), flush=True)
            return
        if not late_app:
            wait_for(lambda: named(app.pid, "All captures"), "Real Library not accessible", 15)
            library_window = wait_for(lambda: visible_windows(app.pid), "Owned Library window not mapped")[0]
        projects = repositories(root, ("alpha", "beta") if concurrent or warm or closed_source else ("alpha",))
        if closed_source:
            environment["GSETTINGS_BACKEND"] = "keyfile"
            command("gsettings", "set", "org.gnome.Terminal.Legacy.Settings", "confirm-close", "false")
        if warm == "tabs":
            # Only this fixture's private XDG profile may create a tab; never
            # start an interactive shell or read the user's startup files.
            environment["GSETTINGS_BACKEND"] = "keyfile"
            profile_id = command("gsettings", "get", "org.gnome.Terminal.ProfilesList", "default").strip("'")
            profile = f"org.gnome.Terminal.Legacy.Profile:/org/gnome/terminal/legacy/profiles:/:{profile_id}/"
            fixture_command = shlex.join(["/usr/bin/env", "-C", str(projects[1]), "/usr/bin/sleep", "60"])
            for key, value in (("use-custom-command", "true"), ("custom-command", fixture_command)):
                command("gsettings", "set", profile, key, value)
        diagnostic_path = root / "runtime/desktop-native.sock"
        diagnostic = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        diagnostic.setsockopt(socket.SOL_SOCKET, socket.SO_PASSCRED, 1)
        diagnostic.bind(str(diagnostic_path))
        diagnostic.setblocking(False)
        app_id = "org.lyn.NativeDesktop"
        server = start(["/usr/libexec/gnome-terminal-server", f"--app-id={app_id}"],
                       env=dict(environment, GTK3_MODULES=str(module), LYN_GNOME_TEST_STREAM=str(endpoint),
                                LYN_GNOME_TEST_SOCKET=str(diagnostic_path)),
                       stdout=subprocess.DEVNULL)
        subprocess.run(["gdbus", "wait", "--session", "--timeout", "5", app_id],
                       env=environment, check=True, timeout=6)
        client = "/usr/bin/gnome-terminal.real" if Path("/usr/bin/gnome-terminal.real").exists() else "gnome-terminal"
        window_projects = projects[:1] if warm == "tabs" else projects
        launches = [start([client, f"--app-id={app_id}", "--window", f"--role=lyn-desktop-{project.name}",
                           f"--working-directory={project}", "--", "/usr/bin/sleep", "60" if warm else "45"])
                    for project in window_projects]
        for launch in launches:
            assert launch.wait(timeout=5) == 0, "Concurrent terminal creation failed"

        def project_windows():
            windows = visible_windows(server.pid, "gnome-terminal")
            if len(windows) != len(window_projects):
                return None
            found = {}
            for project in window_projects:
                matching = [window for window in windows
                            if command("xprop", "-id", window, "WM_WINDOW_ROLE").endswith(
                                f'"lyn-desktop-{project.name}"')]
                if len(matching) != 1:
                    return None
                found[project.name] = matching[0]
            return found

        windows = wait_for(project_windows, "GNOME windows not ready")
        if warm == "tabs":
            window = windows[projects[0].name]
            command("xdotool", "windowactivate", "--sync", window)
            window_path = command("xprop", "-id", window, "_GTK_WINDOW_OBJECT_PATH")
            object_path = re.search(r'= "([^"]+)"', window_path).group(1)
            command("gdbus", "call", "--session", "--dest", app_id,
                    "--object-path", object_path, "--method", "org.gtk.Actions.Activate",
                    "new-terminal", "[<('tab', 'current')>]", "{}")
            windows[projects[1].name] = window
        producer_state = {}
        def read_producer_observation():
            try:
                payload, ancillary, flags, _ = diagnostic.recvmsg(8192, socket.CMSG_SPACE(12))
            except BlockingIOError:
                return False
            assert not flags & (socket.MSG_TRUNC | socket.MSG_CTRUNC)
            credentials = [struct.unpack("3i", data) for level, kind, data in ancillary
                           if level == socket.SOL_SOCKET and kind == socket.SCM_CREDENTIALS]
            assert credentials == [(server.pid, os.getuid(), os.getgid())]
            frame = json.loads(payload)
            producer_state["datagramsRead"] = producer_state.get("datagramsRead", 0) + 1
            if frame["kind"] == "snapshot":
                producer_state["allSources"] = frame["sources"]
                producer_state["sources"] = [
                    {"windowMatches": source["windowId"] == int(window),
                     "active": source["active"],
                     "fresh": source["observedAtUs"] >= readiness_after_us,
                     "observationLagMs": round((readiness_after_us - source["observedAtUs"]) / 1000, 2),
                     "directoryMatches": Path(f'/proc/{source["foregroundPgid"]}/cwd').resolve() == project.resolve()}
                    for source in frame["sources"]]
            matching = [source for source in frame.get("sources", []) if source["windowId"] == int(window) and
                source["active"] and source["observedAtUs"] >= readiness_after_us and
                Path(f'/proc/{source["foregroundPgid"]}/cwd').resolve() == project.resolve()
                ] if frame["kind"] == "snapshot" else []
            if len(matching) != 1:
                return False
            producer_state["selectedSource"] = matching[0]
            return True
        def producer_ready():
            # Diagnostics arrive at 40 Hz, while wait_for polls at 20 Hz.
            # Drain a bounded batch instead of accumulating old observations
            # across captures. Every packet retains its uid/PID/size checks.
            for _ in range(32):
                if read_producer_observation():
                    return True
            return False
        if late_app:
            project = projects[0]
            window = windows[project.name]
            command("xdotool", "windowactivate", "--sync", window)
            readiness_after_us = time.monotonic_ns() // 1000
            wait_for(producer_ready, "GNOME must be mapped and reporting before Lyn starts")
            assert not endpoint.exists(), "Late-app fixture unexpectedly has a broker"
            original_server = server.pid
            original_source = producer_state["selectedSource"].copy()
            app = start_app()
            wait_for(lambda: endpoint.exists() and app.poll() is None, "Late Lyn endpoint not ready", 15)
            wait_for(lambda: named(app.pid, "All captures"), "Late Lyn Library not accessible", 15)
            library_window = wait_for(lambda: visible_windows(app.pid), "Late Library not mapped")[0]
            assert server.pid == original_server and server.poll() is None
            readiness_after_us = time.monotonic_ns() // 1000
            wait_for(producer_ready, "Original GNOME source disappeared during late app startup")
            assert producer_state["selectedSource"]["sourceId"] == original_source["sourceId"]
        database = Path(environment["XDG_DATA_HOME"]) / "com.mibienpanjoe.lyn/lyn.db"
        if race:
            window = windows[projects[0].name]
            project = projects[0]
            command("xdotool", "windowactivate", "--sync", window)
            command("gdbus", "call", "--session", "--dest", app_id, "--object-path", object_path,
                    "--method", "org.gtk.Actions.SetState", "active-tab", "<int32 0>", "{}")
            readiness_after_us = time.monotonic_ns() // 1000
            wait_for(producer_ready, "Original alpha tab not ready for race")
            original = producer_state["selectedSource"].copy()
            command("xdotool", "key", "--delay", "0", "--clearmodifiers", "ctrl+shift+space")
            def receive_gate(size):
                result = bytearray()
                while len(result) < size:
                    part = query_gate.recv(size - len(result))
                    assert part, "Fixture query gate closed"
                    result.extend(part)
                return bytes(result)
            packet = receive_gate(13)
            assert packet[0:1] == b'Q' and int.from_bytes(packet[1:5], "big") == int(window)
            invoked_us = int.from_bytes(packet[5:], "big")
            assert original["changedAtUs"] < invoked_us
            for index in ([1, 0] if race == "aba" else [1]):
                command("gdbus", "call", "--session", "--dest", app_id, "--object-path", object_path,
                        "--method", "org.gtk.Actions.SetState", "active-tab", f"<int32 {index}>", "{}")
                project = projects[index]
                readiness_after_us = invoked_us
                while not producer_ready():
                    remaining = invoked_us / 1_000_000 + .100 - time.monotonic()
                    assert remaining > 0, "Race setup consumed the native reply budget"
                    select.select([diagnostic], [], [], remaining)
            changed = producer_state["selectedSource"]
            assert changed["changedAtUs"] >= invoked_us
            assert (changed["sourceId"] == original["sourceId"]) == (race == "aba")
            query_gate.sendall(b'R')
            outcome = receive_gate(2)
            assert outcome == b'O\x01', f"Expected topology refusal, not timeout or another error: {outcome!r}"
            wait_for(lambda: named(app.pid, "Capture text"), "Race popup absent")
            wait_for(lambda: named(app.pid, "Choose context"), "Race opportunistically resolved a context")
            assert not any(node.get_name().startswith("Context alpha,") or
                           node.get_name().startswith("Context beta,") for node in nodes(app.pid))
            capture_window = command("xdotool", "getactivewindow")
            assert command("xdotool", "getwindowpid", capture_window) == str(app.pid)
            body = f"synthetic-race-{race}-preserved-draft-ENDOFNOTE"
            command("xdotool", "type", "--clearmodifiers", "--delay", "0", body)
            wait_for(lambda: contains(app.pid, body), "Race draft absent")
            assert named(app.pid, "Back to note") is None, "Chooser was already open before the save attempt"
            command("xdotool", "key", "Return")
            back = wait_for(lambda: named(app.pid, "Back to note"), "Refused save did not open chooser")
            assert back.do_action(0)
            wait_for(lambda: named(app.pid, "Capture text") and contains(app.pid, body), "Refusal lost draft")
            with sqlite3.connect(database) as db:
                assert db.execute("SELECT COUNT(*) FROM captures").fetchone()[0] == 0
            draft_paint_state = {}
            def draft_painted():
                field = next((node for node in nodes(app.pid) if "Text" in node.get_interfaces()
                              and node.get_attributes().get("id") == "capture-text"
                              and node.get_attributes().get("tag") == "textarea"
                              and node.get_text(0, -1) == body), None)
                draft_paint_state["fieldFound"] = field is not None
                if field is None:
                    return False
                # The final marker lies past the initial placeholder. Empty or
                # stale placeholder frames cannot supply ink in this rectangle.
                first = field.get_character_extents(len(body) - 6, Atspi.CoordType.SCREEN)
                last = field.get_character_extents(len(body) - 1, Atspi.CoordType.SCREEN)
                region = (first.x, first.y, last.x + last.width - first.x, max(first.height, last.height))
                draft_paint_state["region"] = region
                current = pixels()
                x, y, width, height = region
                if x < 0 or y < 0 or width <= 0 or height <= 0 or x + width > current.get_width() or y + height > current.get_height():
                    return False
                return current if region_has_contrast(current, region) else False
            try:
                painted = wait_for(draft_painted, "Preserved draft marker was not painted")
            except AssertionError:
                candidates = [{"interfaces": node.get_interfaces(), "attributes": node.get_attributes()}
                              for node in nodes(app.pid) if "Text" in node.get_interfaces()
                              and node.get_text(0, -1) == body]
                print(json.dumps({"check": "race_draft_paint_refused", **draft_paint_state,
                                  "textCandidates": candidates,
                                  "screenshot": save_screenshot(pixels(), "gnome-race-failed-")}), flush=True)
                raise
            print(json.dumps({"check": "native_invocation_race", "race": race,
                              "nativeReplyChanged": True, "savedCount": 0, "draftPreserved": True,
                              "screenshot": save_screenshot(painted, "gnome-race-")}), flush=True)
            return
        if stale_reconnect or closed_source:
            project = projects[0]
            window = windows[project.name]
            command("xdotool", "windowactivate", "--sync", window)
            readiness_after_us = time.monotonic_ns() // 1000
            wait_for(producer_ready, "Native source not ready before disconnect")
            original = producer_state["selectedSource"].copy()
            command("xdotool", "key", "--delay", "0", "--clearmodifiers", "ctrl+shift+space")
            wait_for(lambda: named(app.pid, "Capture text") and
                     named(app.pid, "Context alpha, branch gnome-alpha. Change context"), "Original native context absent")
            assert command("xdotool", "getwindowpid", command("xdotool", "getactivewindow")) == str(app.pid)
            failure = "closed" if closed_source else "reconnect"
            body = f"synthetic-{failure}-preserved-draft-ENDOFNOTE"
            command("xdotool", "type", "--clearmodifiers", "--delay", "0", body)
            wait_for(lambda: contains(app.pid, body), "Draft absent before disconnect")
            if closed_source:
                window_path = command("xprop", "-id", window, "_GTK_WINDOW_OBJECT_PATH")
                object_path = re.search(r'= "([^"]+)"', window_path).group(1)
                description = command("gdbus", "call", "--session", "--dest", app_id,
                                      "--object-path", object_path, "--method", "org.gtk.Actions.Describe", "close")
                assert "'s'" in description, f"GNOME close action must take a string: {description}"
                command("gdbus", "call", "--session", "--dest", app_id, "--object-path", object_path,
                        "--method", "org.gtk.Actions.Activate", "close", "[<'window'>]", "{}")
                wait_for(lambda: window not in visible_windows(server.pid, "gnome-terminal"),
                         "Requested GNOME window did not close")
                def closed_source_removed():
                    for _ in range(32):
                        read_producer_observation()
                        sources = producer_state.get("allSources", [])
                        if (len(sources) == 1 and sources[0]["sourceId"] != original["sourceId"]
                                and sources[0]["windowId"] == int(windows[projects[1].name])):
                            return True
                    return False
                wait_for(closed_source_removed, "Closed source retained or surviving source missing")
                assert server.poll() is None, "Closing one source killed the surviving GNOME server"
            else:
                lifecycle.sendall(b'R')
                lifecycle.settimeout(4)
                generation_reply = bytearray()
                while len(generation_reply) < 33:
                    part = lifecycle.recv(33 - len(generation_reply))
                    assert part, "Lifecycle control closed without reconnect evidence"
                    generation_reply.extend(part)
                assert generation_reply[:1] == b'G' and generation_reply[1:17] != generation_reply[17:33]
                assert server.poll() is None, "Reconnect restarted GNOME instead of its channel"
                readiness_after_us = time.monotonic_ns() // 1000
                wait_for(producer_ready, "Native source absent after reconnect")
                for field in ("sourceId", "ptyNumber", "foregroundPgid", "registeredAtUs", "revision"):
                    assert producer_state["selectedSource"][field] == original[field], field
            assert command("xdotool", "getwindowpid", command("xdotool", "getactivewindow")) == str(app.pid)
            command("xdotool", "key", "Return")
            back = wait_for(lambda: named(app.pid, "Back to note"), "Old lease save did not open chooser")
            wait_for(lambda: named(app.pid, "Context source stale. Choose another context"),
                     "Old lease was not explicitly marked stale")
            assert back.do_action(0)
            wait_for(lambda: named(app.pid, "Capture text") and contains(app.pid, body), "Reconnect lost draft")
            with sqlite3.connect(database) as db:
                assert db.execute("SELECT COUNT(*) FROM captures").fetchone()[0] == 0
            def preserved_draft_pixels():
                field = next((node for node in nodes(app.pid) if "Text" in node.get_interfaces()
                              and node.get_attributes().get("id") == "capture-text"
                              and node.get_text(0, -1) == body), None)
                if field is None:
                    return False
                first = field.get_character_extents(len(body) - 6, Atspi.CoordType.SCREEN)
                last = field.get_character_extents(len(body) - 1, Atspi.CoordType.SCREEN)
                region = (first.x, first.y, last.x + last.width - first.x, first.height)
                current = pixels()
                x, y, width, height = region
                if min(x, y) < 0 or min(width, height) <= 0 or x + width > current.get_width() or y + height > current.get_height():
                    return False
                return current if region_has_contrast(current, region) else False
            painted = wait_for(preserved_draft_pixels, "Preserved reconnect draft not painted")
            print(json.dumps({"check": "native_stale_source_draft", "failure": failure,
                              "generationChanged": stale_reconnect, "sourceClosed": closed_source,
                              "sameNativeSource": not closed_source,
                              "oldLeaseRefused": True, "savedCount": 0,
                              "draftPreserved": True,
                              "screenshot": save_screenshot(painted, f"gnome-{failure}-draft-")}), flush=True)
            assert named(app.pid, "Cancel").do_action(0)
            def cancel_focus_restored():
                # Openbox may briefly unset the active-window property while
                # the popup hides. Absence is not success or a command failure;
                # the original wait still requires the exact terminal XID.
                property_value = command("xprop", "-root", "_NET_ACTIVE_WINDOW")
                active = re.fullmatch(r"_NET_ACTIVE_WINDOW\(WINDOW\): window id # (0x[0-9a-fA-F]+)", property_value)
                return active is not None and int(active[1], 16) == int(window)
            if closed_source:
                wait_for(lambda: named(app.pid, "Capture text") is None, "Cancel did not hide popup")
            else:
                wait_for(cancel_focus_restored, "Cancel did not return focus")
        bodies = []
        detached = False
        original_source = None
        capture_projects = [projects[index % 2] for index in range(alternations + 1)] if warm else projects
        if restart_app:
            capture_projects = [projects[0], projects[0]]
        if closed_source:
            capture_projects = [projects[1]]
        for project in capture_projects:
            if restart_app and len(bodies) == 1:
                previous_app = app.pid
                previous_socket = endpoint.stat().st_ino
                original_source = producer_state["selectedSource"].copy()
                lifecycle.sendall(b'X')
                assert app.wait(timeout=5) == 0, "Lyn did not exit cleanly"
                lifecycle.close()
                lifecycle = None
                assert not endpoint.exists(), "Clean app exit left its native broker socket behind"
                assert server.poll() is None, "App exit terminated the private GNOME server"
                app = start_app()
                lifecycle = lifecycle_connection()
                assert app.pid != previous_app
                wait_for(lambda: endpoint.exists() and app.poll() is None, "Restarted Lyn endpoint not ready", 15)
                wait_for(lambda: named(app.pid, "All captures"), "Restarted Library not accessible", 15)
                library_window = wait_for(lambda: visible_windows(app.pid), "Restarted Library not mapped")[0]
                # Absence between lifetimes is mandatory; inode numbers alone
                # are not lifetime identities and may legitimately be reused.
                print(json.dumps({"check": "app_restarted", "oldSocketInode": previous_socket,
                                  "newSocketInode": endpoint.stat().st_ino}), flush=True)
            capture_started = time.monotonic()
            window = windows[project.name]
            command("xdotool", "windowactivate", "--sync", window)
            wait_for(lambda: int(command("xdotool", "getactivewindow")) == int(window), "GNOME not foreground")
            readiness_after_us = time.monotonic_ns() // 1000
            if warm == "tabs" and not detached:
                command("xdotool", "key", "--clearmodifiers", f"alt+{projects.index(project) + 1}")
            # GN05 establishes explicit readiness; concurrent-start acceptance
            # invokes as soon as the owned window is mapped and foreground.
            if not concurrent:
                try:
                    wait_for(producer_ready, "Native prototype did not register its window")
                except AssertionError:
                    print(json.dumps({"check": "native_readiness_refused", "mode": warm,
                                      "project": project.name, "completedCaptures": len(bodies),
                                      "serverExit": server.poll(),
                                      **producer_state}), flush=True)
                    raise
            if restart_app and len(bodies) == 1:
                for field in ("sourceId", "ptyNumber", "foregroundPgid", "registeredAtUs"):
                    assert producer_state["selectedSource"][field] == original_source[field], field
            if relocate and not bodies:
                original_source = producer_state["selectedSource"].copy()
            if relocate and len(bodies) == 2:
                before_move = producer_state["selectedSource"].copy()
                before_windows = set(visible_windows(server.pid, "gnome-terminal"))
                command("gdbus", "call", "--session", "--dest", app_id,
                        "--object-path", object_path, "--method", "org.gtk.Actions.Activate",
                        "tab-detach", "[]", "{}")
                def detached_window():
                    current = set(visible_windows(server.pid, "gnome-terminal"))
                    added = current - before_windows
                    return next(iter(added)) if len(added) == 1 and before_windows <= current else None
                window = wait_for(detached_window, "Detached tab did not create one owned window")
                windows[project.name] = window
                command("xdotool", "windowactivate", "--sync", window)
                wait_for(lambda: int(command("xdotool", "getactivewindow")) == int(window), "Detached GNOME not foreground")
                readiness_after_us = time.monotonic_ns() // 1000
                wait_for(producer_ready, "Detached source not observed in new window")
                moved = producer_state["selectedSource"]
                for field in ("sourceId", "ptyNumber", "foregroundPgid", "registeredAtUs"):
                    assert moved[field] == original_source[field], (field, moved, original_source)
                assert moved["windowId"] != original_source["windowId"]
                assert moved["revision"] > before_move["revision"]
                assert moved["changedAtUs"] > before_move["changedAtUs"]
                detached = True
            command("xdotool", "key", "--clearmodifiers", "ctrl+shift+space")
            wait_for(lambda: named(app.pid, "Capture text"), "Real capture popup not accessible")
            context_label = f"Context {project.name}, branch gnome-{project.name}"
            wait_for(lambda: any(node.get_name().startswith(context_label)
                                 for node in nodes(app.pid)), f"Native context not rendered: {project.name}")
            capture_window = command("xdotool", "getactivewindow")
            assert command("xdotool", "getwindowpid", capture_window) == str(app.pid), "Input target is not the owned Lyn process"
            body = (f"synthetic-{warm}-{len(bodies):02d}-{project.name}" if warm else
                    f"synthetic-restart-{len(bodies)}" if restart_app else
                    "synthetic-native-desktop-acceptance" + (f"-{project.name}" if concurrent else ""))
            bodies.append(body)
            command("xdotool", "type", "--clearmodifiers", "--delay", "0", body)
            wait_for(lambda: contains(app.pid, body), "Typed text absent from real popup")
            command("xdotool", "key", "Return")
            def saved():
                with sqlite3.connect(database) as db:
                    rows = db.execute("SELECT c.text_body,c.branch_name,p.project_path FROM captures c "
                                      "JOIN contexts p ON p.id=c.context_id WHERE c.text_body=?", (body,)).fetchall()
                    count = db.execute("SELECT COUNT(*) FROM captures").fetchone()[0]
                    return count == len(bodies) and rows == [(body, f"gnome-{project.name}", str(project.resolve()))]
            wait_for(saved, f"Real popup did not durably save once: {project.name}")
            wait_for(lambda: int(command("xdotool", "getactivewindow")) == int(window), "Focus not restored to GNOME")
            if warm:
                print(json.dumps({"check": "warm_capture", "mode": warm, "index": len(bodies) - 1,
                                  "project": project.name,
                                  "elapsedMs": round((time.monotonic() - capture_started) * 1000, 2)}), flush=True)
        assert not relocate or detached, "Requested tab relocation was not exercised"
        if restart_app:
            # Restart loads the first saved row into Recent. It may already be
            # painted identically before switching to All captures. Establish
            # an actually obscured baseline, not an unchanged old-row failure.
            command("xdotool", "windowminimize", "--sync", library_window)
            wait_for(lambda: library_window not in visible_windows(app.pid), "Library did not minimize")
            before_navigation = pixels()
            command("xdotool", "windowactivate", "--sync", library_window)
        else:
            command("xdotool", "windowactivate", "--sync", library_window)
            before_navigation = pixels()
        library = named(app.pid, "All captures")
        assert library.do_action(0)
        # Every save was checked in SQLite. Library pages contain at most 50
        # rows and scroll to the newest; inspect the last five painted rows.
        visible_bodies = bodies[-5:]
        wait_for(lambda: all(contains(app.pid, body) for body in visible_bodies), "Saved capture absent from rendered Library")
        assert named(app.pid, "Capture text") is None, "Hidden popup must not prove Library rendering"
        paint_state = {}
        def library_painted():
            visible = [node for node in nodes(app.pid)
                       if node.get_state_set().contains(Atspi.StateType.SHOWING)]
            heading = page_heading(visible, "All captures")
            rows = [next((node for node in visible if body in node.get_name()
                          and "Component" in node.get_interfaces()), None) for body in visible_bodies]
            paint_state.update(heading=bool(heading), row=all(rows),
                               empty=any(node.get_name() == "Nothing captured yet" for node in visible),
                               titleRole=(str(heading.get_role()) if heading else None))
            if not heading or not all(rows) or any(node.get_name() == "Nothing captured yet" for node in visible):
                return False
            current = pixels()
            # WebKit can publish its accessibility tree before Xvfb has painted
            # the new page. Require changed pixels at BOTH the heading and row,
            # not an arbitrary delay or a cursor/focus change elsewhere.
            for index, node in enumerate([heading, *rows]):
                rect = node.get_extents(Atspi.CoordType.SCREEN)
                key = "heading" if index == 0 else f"row{index}"
                paint_state[key + "Rect"] = (rect.x, rect.y, rect.width, rect.height)
                if (rect.x < 0 or rect.y < 0 or rect.width <= 0 or rect.height <= 0
                        or rect.x + rect.width > current.get_width()
                        or rect.y + rect.height > current.get_height()):
                    return False
                region = (rect.x, rect.y, rect.width, rect.height)
                changed = region_changed(before_navigation, current, region)
                paint_state[key + "PixelsChanged"] = changed
                if not changed:
                    return False
            return current
        try:
            painted = wait_for(library_painted, "Library accessibility changed without a painted heading and capture")
        except AssertionError:
            candidates = []
            for node in nodes(app.pid):
                if (node.get_attributes().get("tag") == "h1"
                        or node.get_name() == "All captures"):
                    rect = node.get_extents(Atspi.CoordType.SCREEN)
                    candidates.append({"name": node.get_name(), "role": str(node.get_role()),
                                       "attributes": node.get_attributes(),
                                       "showing": node.get_state_set().contains(Atspi.StateType.SHOWING),
                                       "rect": (rect.x, rect.y, rect.width, rect.height)})
            print(json.dumps({"check": "library_paint_refused", **paint_state,
                              "titleCandidates": candidates,
                              "screenshot": save_screenshot(pixels(), "gnome-desktop-failed-")}), flush=True)
            raise
        screenshot = save_screenshot(painted, "gnome-desktop-")
        print(json.dumps({"check": "real_gnome_desktop_capture", "nativeContextRendered": True,
                          "appStartedAfterGnome": late_app,
                          "appRestarted": restart_app,
                          "staleReconnect": stale_reconnect,
                          "closedSource": closed_source,
                          "savedOnceInExpectedProjectBranch": True, "libraryRendered": True,
                          "concurrentStart": concurrent, "nativeReadinessWait": not concurrent,
                          "warmMode": warm, "alternations": len(bodies) - 1 if warm else 0,
                          "tabRelocated": detached,
                          "capturedProjects": [project.name for project in capture_projects],
                          "focusRestored": True, "screenshot": screenshot}), flush=True)
    finally:
        for process in reversed(children):
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=2)
        if 'wm_process' in locals() and wm_process.stdout:
            print("Owned Openbox output:", wm_process.stdout.read()[:2048], flush=True)
        if diagnostic:
            diagnostic.close()
        if query_gate:
            query_gate.close()
        if gate_listener:
            gate_listener.close()
        if lifecycle:
            lifecycle.close()
        if lifecycle_listener:
            lifecycle_listener.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("module", type=Path)
    parser.add_argument("executable", type=Path)
    parser.add_argument("sdk")
    mode = parser.add_mutually_exclusive_group()
    for name in ("probe", "concurrent-start", "warm-windows", "warm-tabs", "relocate-tab", "race-switch", "race-aba", "late-app", "restart-app", "stale-reconnect", "closed-source"):
        mode.add_argument(f"--{name}", action="store_true")
    parser.add_argument("--alternations", type=int, default=4)
    arguments = parser.parse_args()
    if not 1 <= arguments.alternations <= 100:
        parser.error("--alternations must be from 1 to 100")
    run(arguments.module.resolve(strict=True), arguments.executable.resolve(strict=True),
        None if arguments.sdk == "-" else Path(arguments.sdk).resolve(strict=True),
        probe=arguments.probe, concurrent=arguments.concurrent_start,
        warm="windows" if arguments.warm_windows else "tabs" if arguments.warm_tabs or arguments.relocate_tab or arguments.race_switch or arguments.race_aba else None,
        alternations=arguments.alternations, relocate=arguments.relocate_tab,
        race="switch" if arguments.race_switch else "aba" if arguments.race_aba else None,
        late_app=arguments.late_app, restart_app=arguments.restart_app,
        stale_reconnect=arguments.stale_reconnect, closed_source=arguments.closed_source)
