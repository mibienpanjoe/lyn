"""Real Lyn webviews/shortcut on test-owned X11, bus, WM and XDG directories."""

import json
import os
from pathlib import Path
import select
import socket
import sqlite3
import struct
import subprocess
import sys
import tempfile
import time

from fixtures import repositories
from desktop_isolation import require_owned_display
from desktop_pixels import region_changed


def wait_for(check, message, timeout=12):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(.05)
    raise AssertionError(message)


def run(module, executable, sdk):
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

    children = []
    diagnostic = None

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
        return next((node for node in nodes(pid) if node.get_name() == name), None)

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
        vite = start(["node", str(repo / "node_modules/vite/bin/vite.js"), "--host", "127.0.0.1",
                      "--port", str(port), "--strictPort"], cwd=repo, stdout=subprocess.PIPE,
                     stderr=subprocess.STDOUT, text=True)
        deadline = time.monotonic() + 5
        vite_output = ""
        while True:
            assert vite.poll() is None, "Owned Vite process exited"
            assert time.monotonic() < deadline, "Owned Vite did not become ready"
            if select.select([vite.stdout], [], [], .2)[0]:
                vite_output += os.read(vite.stdout.fileno(), 8192).decode()
                if f"127.0.0.1:{port}" in vite_output:
                    break
        app = start([str(executable), "--exact", "desktop_tests::real_gnome_desktop_entry",
                     "--nocapture", "--test-threads=1"],
                    env=dict(environment, LYN_GNOME_UI_FIXTURE="1", LYN_GNOME_UI_PORT=str(port)))
        endpoint = Path(environment["XDG_RUNTIME_DIR"]) / "lyn-gnome-v1.sock"
        wait_for(lambda: endpoint.exists() and app.poll() is None, "Lyn native endpoint not ready", 15)
        wait_for(lambda: named(app.pid, "All captures"), "Real Library not accessible", 15)
        library_window = wait_for(lambda: visible_windows(app.pid), "Owned Library window not mapped")[0]
        project = repositories(root, ("alpha",))[0]
        diagnostic_path = root / "runtime/desktop-native.sock"
        diagnostic = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        diagnostic.setsockopt(socket.SOL_SOCKET, socket.SO_PASSCRED, 1)
        diagnostic.bind(str(diagnostic_path))
        diagnostic.settimeout(.25)
        app_id = "org.lyn.NativeDesktop"
        server = start(["/usr/libexec/gnome-terminal-server", f"--app-id={app_id}"],
                       env=dict(environment, GTK3_MODULES=str(module), LYN_GNOME_TEST_STREAM=str(endpoint),
                                LYN_GNOME_TEST_SOCKET=str(diagnostic_path)),
                       stdout=subprocess.DEVNULL)
        subprocess.run(["gdbus", "wait", "--session", "--timeout", "5", app_id],
                       env=environment, check=True, timeout=6)
        client = "/usr/bin/gnome-terminal.real" if Path("/usr/bin/gnome-terminal.real").exists() else "gnome-terminal"
        subprocess.run([client, f"--app-id={app_id}", "--window", "--role=lyn-desktop-alpha",
                        f"--working-directory={project}", "--", "/usr/bin/sleep", "45"],
                       env=environment, check=True, timeout=5)
        window = wait_for(lambda: visible_windows(server.pid, "gnome-terminal"), "GNOME window not ready")[0]
        command("xdotool", "windowactivate", "--sync", window)
        wait_for(lambda: int(command("xdotool", "getactivewindow")) == int(window), "GNOME not foreground")
        def producer_ready():
            try:
                payload, ancillary, flags, _ = diagnostic.recvmsg(8192, socket.CMSG_SPACE(12))
            except socket.timeout:
                return False
            assert not flags & (socket.MSG_TRUNC | socket.MSG_CTRUNC)
            credentials = [struct.unpack("3i", data) for level, kind, data in ancillary
                           if level == socket.SOL_SOCKET and kind == socket.SCM_CREDENTIALS]
            assert credentials == [(server.pid, os.getuid(), os.getgid())]
            frame = json.loads(payload)
            return frame["kind"] == "snapshot" and any(source["windowId"] == int(window) and
                source["active"] for source in frame["sources"])
        # Fixture readiness only; diagnostics never authorize the real capture.
        # Cold first-invoke acceptance without this readiness is separate GN06.
        wait_for(producer_ready, "Native prototype did not register its window")
        command("xdotool", "key", "--clearmodifiers", "ctrl+shift+space")
        wait_for(lambda: named(app.pid, "Capture text"), "Real capture popup not accessible")
        wait_for(lambda: any(node.get_name().startswith("Context alpha, branch gnome-alpha")
                             for node in nodes(app.pid)), "Native context not rendered")
        capture_window = command("xdotool", "getactivewindow")
        assert command("xdotool", "getwindowpid", capture_window) == str(app.pid), "Input target is not the owned Lyn process"
        body = "synthetic-native-desktop-acceptance"
        command("xdotool", "type", "--clearmodifiers", "--delay", "0", body)
        wait_for(lambda: contains(app.pid, body), "Typed text absent from real popup")
        command("xdotool", "key", "Return")
        database = Path(environment["XDG_DATA_HOME"]) / "com.mibienpanjoe.lyn/lyn.db"
        def saved():
            with sqlite3.connect(database) as db:
                rows = db.execute("SELECT c.text_body,c.branch_name,p.project_path FROM captures c "
                                  "JOIN contexts p ON p.id=c.context_id WHERE c.text_body=?", (body,)).fetchall()
                count = db.execute("SELECT COUNT(*) FROM captures").fetchone()[0]
                return count == 1 and rows == [(body, "gnome-alpha", str(project.resolve()))]
        wait_for(saved, "Real popup did not durably save once")
        wait_for(lambda: int(command("xdotool", "getactivewindow")) == int(window), "Focus not restored to GNOME")
        command("xdotool", "windowactivate", "--sync", library_window)
        before_navigation = pixels()
        library = named(app.pid, "All captures")
        assert library.do_action(0)
        wait_for(lambda: contains(app.pid, body), "Saved capture absent from rendered Library")
        assert named(app.pid, "Capture text") is None, "Hidden popup must not prove Library rendering"
        paint_state = {}
        def library_painted():
            visible = [node for node in nodes(app.pid)
                       if node.get_state_set().contains(Atspi.StateType.SHOWING)]
            heading = next((node for node in visible if node.get_name() == "All captures"
                            and node.get_role() == Atspi.Role.HEADING), None)
            row = next((node for node in visible if body in node.get_name()
                        and "Component" in node.get_interfaces()), None)
            paint_state.update(heading=bool(heading), row=bool(row),
                               empty=any(node.get_name() == "Nothing captured yet" for node in visible))
            if not heading or not row or any(node.get_name() == "Nothing captured yet" for node in visible):
                return False
            current = pixels()
            # WebKit can publish its accessibility tree before Xvfb has painted
            # the new page. Require changed pixels at BOTH the heading and row,
            # not an arbitrary delay or a cursor/focus change elsewhere.
            for node in (heading, row):
                rect = node.get_extents(Atspi.CoordType.SCREEN)
                key = "heading" if node == heading else "row"
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
            print(json.dumps({"check": "library_paint_refused", **paint_state}), flush=True)
            raise
        artifact_root = repo / "tasks"
        assert artifact_root.is_dir() and not artifact_root.is_symlink()
        descriptor, screenshot = tempfile.mkstemp(prefix="gnome-desktop-", suffix=".png", dir=artifact_root)
        os.close(descriptor)
        assert painted.savev(screenshot, "png", [], [])
        print(json.dumps({"check": "real_gnome_desktop_capture", "nativeContextRendered": True,
                          "savedOnceInExpectedProjectBranch": True, "libraryRendered": True,
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


if __name__ == "__main__":
    run(Path(sys.argv[1]).resolve(strict=True), Path(sys.argv[2]).resolve(strict=True),
        None if sys.argv[3] == "-" else Path(sys.argv[3]).resolve(strict=True))
