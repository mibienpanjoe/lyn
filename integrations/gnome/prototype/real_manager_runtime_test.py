"""Real manager definition proof, confined to a fresh delegated cgroup/namespace."""
import json
import hashlib
import os
from pathlib import Path
import re
import resource
import signal
import stat
import subprocess
import sys
import tempfile
import time
import uuid

KINDS = ("user", "pid", "mnt", "net", "cgroup")
CHILDREN = frozenset(("init.scope", "dbus.service", "app.slice"))


def require(value, message):
    if not value:
        raise RuntimeError(message)


def current_group():
    return parse_group(Path("/proc/self/cgroup").read_text())


def parse_group(text):
    require(len(text) < 4096 and text.count("\n") == 1 and text.startswith("0::/"), "unified cgroup required")
    path = text.strip()[3:]
    require(path == "/" or all(part not in ("", ".", "..") for part in path.split("/")[1:]), "canonical cgroup required")
    return path


def remove_children(leaf_fd, pins):
    """Never recurse, adopt an unrecorded child, or remove unknown entries."""
    entries = os.listdir(leaf_fd)
    require(len(entries) <= 128, "too many cgroup entries: preserve")
    directories = [entry for entry in entries if stat.S_ISDIR(os.stat(entry, dir_fd=leaf_fd, follow_symlinks=False).st_mode)]
    require(set(directories) <= CHILDREN, "unknown cgroup children: preserve")
    def check(entry):
        require(pins.get(entry) is not None, "unrecorded child cgroup: preserve")
        current = os.stat(entry, dir_fd=leaf_fd, follow_symlinks=False)
        pinned = os.fstat(pins[entry])
        require((current.st_dev, current.st_ino) == (pinned.st_dev, pinned.st_ino), "child cgroup replaced: preserve")
    # Validate the whole inventory before the first removal, then recheck each
    # member immediately before rmdir. Kernel cgroup virtual files stay intact.
    for entry in directories:
        check(entry)
    for entry in directories:
        check(entry)
        os.rmdir(entry, dir_fd=leaf_fd)


def private(path):
    metadata = path.lstat()
    require(stat.S_ISDIR(metadata.st_mode) and metadata.st_uid == os.geteuid()
            and metadata.st_mode & 0o7777 == 0o700 and path.resolve(strict=True) == path, "private directory required")


def load(root):
    private(root)
    require(root.parent == Path("/tmp") and root.name.startswith("lyn-na-"), "private root required")
    path = root / "control.json"
    meta = path.lstat()
    require(stat.S_ISREG(meta.st_mode) and meta.st_uid == os.geteuid() and meta.st_mode & 0o7777 == 0o600
            and meta.st_nlink == 1 and meta.st_size < 4096, "private control required")
    control = json.loads(path.read_text())
    runtime = Path(control["runtime"])
    require(runtime.name == "runtime" and runtime.parent.parent == Path("/tmp")
            and runtime.parent.name.startswith("lyn-gnome-run-"), "wrong private runtime family")
    private(runtime.parent)
    private(runtime)
    require(control["address"] == f"unix:path={runtime}/bootstrap-bus", "wrong private bus address")
    socket = (runtime / "bootstrap-bus").lstat()
    require(stat.S_ISSOCK(socket.st_mode) and socket.st_uid == os.geteuid() and socket.st_nlink == 1, "wrong private bus socket")
    return control


def enter(root, leaf_fd):
    control = load(root)
    require(os.geteuid() == control["uid"] > 0 and os.getegid() == control["gid"] > 0, "ordinary user required")
    require(current_group() == control["parent_group"], "unexpected parent cgroup")
    metadata = os.fstat(leaf_fd)
    require([metadata.st_dev, metadata.st_ino] == control["leaf_identity"], "wrong cgroup descriptor")
    require(os.readlink(f"/proc/self/fd/{leaf_fd}") == control["leaf_path"], "wrong cgroup path")
    require(re.fullmatch(r"lyn-systemd-[0-9a-f]{32}", Path(control["leaf_path"]).name), "wrong cgroup name")
    fd = os.open("cgroup.procs", os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=leaf_fd)
    try:
        require(os.read(fd, 4096).strip() == b"", "private cgroup is occupied")
    finally:
        os.close(fd)
    fd = os.open("cgroup.procs", os.O_WRONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=leaf_fd)
    try:
        require(os.write(fd, b"0\n") == 2, "self migration failed")
    finally:
        os.close(fd)
    require(current_group() == control["parent_group"] + "/" + Path(control["leaf_path"]).name, "self migration mismatch")
    runtime_root = Path(control["runtime"]).parent
    args = ["/usr/bin/bwrap", "--die-with-parent", "--new-session", "--unshare-user", "--unshare-pid",
            "--unshare-net", "--unshare-ipc", "--unshare-uts", "--unshare-cgroup", "--uid", str(control["uid"]),
            "--gid", str(control["gid"]), "--ro-bind", "/", "/", "--ro-bind", "/sys/fs/cgroup", "/sys/fs/cgroup",
            "--bind-fd", str(leaf_fd), "/sys/fs/cgroup", "--tmpfs", "/run", "--tmpfs", "/tmp", "--tmpfs", "/home",
            "--bind", str(root), str(root), "--bind", str(runtime_root), str(runtime_root),
            "--proc", "/proc", "--dev", "/dev", "--clearenv"]
    environment = {"PATH": "/usr/bin:/bin", "HOME": str(root / "home"), "XDG_RUNTIME_DIR": control["runtime"],
                   "XDG_CONFIG_HOME": str(root / "config"), "XDG_DATA_HOME": str(root / "data"),
                   "XDG_CACHE_HOME": str(root / "cache"), "DBUS_SESSION_BUS_ADDRESS": control["address"],
                   "SYSTEMD_UNIT_PATH": str(root / "units") + ":/usr/lib/systemd/user",
                   "SYSTEMD_GENERATOR_PATH": str(root / "empty"), "SYSTEMD_ENVIRONMENT_GENERATOR_PATH": str(root / "empty")}
    for key, value in environment.items():
        args += ["--setenv", key, value]
    args += ["--chdir", str(root / "home"), "/usr/bin/python3", str(root / "probe.py"), "--inner", str(root)]
    os.set_inheritable(leaf_fd, True)
    os.execv(args[0], args)


def inner(root):
    control = load(root)
    require(os.getpid() == 2 and os.getppid() == 1 and os.geteuid() == control["uid"] > 0
            and os.getegid() == control["gid"] > 0, "private PID2 ordinary user required")
    # Bubblewrap may insert a parent namespace mapping the outer user to0.
    # Require one entry and cross-check the SAME pinned kernel cgroup's owner
    # below; map syntax alone cannot establish the composed outside identity.
    for kind, identifier in (("uid", control["uid"]), ("gid", control["gid"])):
        mapping = Path(f"/proc/self/{kind}_map").read_text().split()
        require(mapping in ([str(identifier), str(identifier), "1"], [str(identifier), "0", "1"]), f"unexpected {kind} mapping")
        require(int(Path(f"/proc/sys/kernel/overflow{kind}").read_text()) != identifier, "ambiguous overflow identity")
    for kind in KINDS:
        require(os.readlink(f"/proc/self/ns/{kind}") != control["namespaces"][kind], "namespace not isolated")
    require(current_group() == "/", "private cgroup namespace root required")
    cg = Path("/sys/fs/cgroup").stat()
    require([cg.st_dev, cg.st_ino] == control["leaf_identity"], "private cgroup mount mismatch")
    require(cg.st_uid == control["uid"] and cg.st_gid == control["gid"], "outside cgroup owner does not map to the inner user")
    require(os.statvfs("/").f_flag & os.ST_RDONLY, "root must be readonly")
    require(not list(Path("/run").iterdir()) and not list(Path("/home").iterdir()), "host run/home exposed")
    runtime = Path(control["runtime"])
    private(runtime)
    require(os.environ.get("DBUS_SESSION_BUS_ADDRESS") == f"unix:path={runtime}/bootstrap-bus", "wrong bus")
    require(stat.S_ISSOCK((runtime / "bootstrap-bus").lstat().st_mode), "private filesystem bus missing")
    # Keep the external bootstrap bus separate. The real manager owns the
    # socket/service pair for its own API bus; no host activation is enabled.
    os.environ["DBUS_SESSION_BUS_ADDRESS"] = f"unix:path={runtime}/session-bus"
    for fd in list(Path("/proc/self/fd").iterdir()):
        if int(fd.name) > 2:
            try:
                os.close(int(fd.name))
            except OSError:
                pass
    Path("/run/systemd/system").mkdir(parents=True, mode=0o700)
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    resource.setrlimit(resource.RLIMIT_FSIZE, (1024 * 1024, 1024 * 1024))
    with (root / "manager.log").open("wb") as output:
        process = subprocess.Popen(["/usr/lib/systemd/systemd", "--user", "--unit=lyn-private.target",
                                    "--log-target=console", "--log-level=info"], stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.STDOUT)
        pin = os.pidfd_open(process.pid)
        try:
            deadline = time.monotonic() + 45
            while process.poll() is None and not (root / "stop").exists():
                require(time.monotonic() < deadline, "private manager lifetime exceeded")
                time.sleep(.01)
            if process.poll() is not None:
                print("manager_exit", process.returncode, flush=True)
        finally:
            if process.poll() is None:
                signal.pidfd_send_signal(pin, signal.SIGTERM)
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                signal.pidfd_send_signal(pin, signal.SIGKILL)
                process.wait(timeout=2)
            os.close(pin)


def main(executable, module):
    require(executable.is_absolute() and executable.is_file(), "current Cargo test executable required")
    require(module.is_absolute() and module.is_file(), "built native bundle required")
    require(os.geteuid() > 0 and os.getegid() > 0, "never host root")
    original = current_group()
    require(original.startswith(f"/user.slice/user-{os.geteuid()}.slice/user@{os.geteuid()}.service/"), "delegated user hierarchy required")
    parent = Path("/sys/fs/cgroup") / original.lstrip("/")
    parent_fd = os.open(parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    meta = os.fstat(parent_fd)
    require(meta.st_uid == os.geteuid() and not meta.st_mode & 0o022, "private delegated parent required")
    require(meta.st_dev == Path("/sys/fs/cgroup").stat().st_dev, "wrong cgroup filesystem")
    name = "lyn-systemd-" + uuid.uuid4().hex
    leaf_fd = None
    scope_fd = None
    service_fd = None
    app_fd = None
    child = bus = None
    try:
        with tempfile.TemporaryDirectory(prefix="lyn-na-") as raw_root, tempfile.TemporaryDirectory(prefix="lyn-gnome-run-") as raw_runtime:
            root = Path(raw_root)
            runtime_root = Path(raw_runtime)
            runtime = runtime_root / "runtime"
            runtime.mkdir(mode=0o700)
            for name_dir in ("home", "config", "data", "cache", "units", "empty"):
                (root / name_dir).mkdir(mode=0o700)
            (root / "units/lyn-private.target").write_text("[Unit]\nDescription=Lyn private manager fixture\nDefaultDependencies=no\nRequires=dbus.socket dbus.service\nAfter=dbus.socket dbus.service\n")
            (root / "units/dbus.socket").write_text(f"[Unit]\nDefaultDependencies=no\n[Socket]\nListenStream={runtime}/session-bus\nSocketMode=0600\n")
            (root / "units/dbus.service").write_text(f"[Unit]\nDefaultDependencies=no\nRequires=dbus.socket\nAfter=dbus.socket\n[Service]\nSlice=-.slice\nExecStart=/usr/bin/dbus-daemon --config-file={runtime_root}/session.conf --address=systemd: --nofork --nopidfile\n")
            address = f"unix:path={runtime}/bootstrap-bus"
            config = runtime_root / "session.conf"
            config.write_text(f'<busconfig><type>session</type><listen>{address}</listen><auth>EXTERNAL</auth><policy context="default"><allow user="*"/><allow own="*"/><allow send_destination="*"/><allow receive_sender="*"/></policy></busconfig>')
            bus = subprocess.Popen(["/usr/bin/dbus-daemon", "--nofork", "--config-file", str(config)], stdin=subprocess.DEVNULL,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env={"PATH": "/usr/bin:/bin"})
            deadline = time.monotonic() + 2
            while not (runtime / "bootstrap-bus").is_socket():
                require(bus.poll() is None and time.monotonic() < deadline, "private bus did not start")
                time.sleep(.01)
            os.mkdir(name, dir_fd=parent_fd)
            leaf_fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent_fd)
            identity = os.fstat(leaf_fd)
            require(identity.st_uid == os.geteuid() and identity.st_gid == os.getegid(), "wrong new cgroup owner")
            control = {"uid": os.geteuid(), "gid": os.getegid(), "parent_group": original, "leaf_path": str(parent / name),
                       "leaf_identity": [identity.st_dev, identity.st_ino], "runtime": str(runtime), "address": address,
                       "namespaces": {kind: os.readlink(f"/proc/self/ns/{kind}") for kind in KINDS}}
            (root / "control.json").write_text(json.dumps(control))
            (root / "control.json").chmod(0o600)
            (root / "probe.py").write_bytes(Path(__file__).read_bytes())
            with (root / "bootstrap.log").open("wb") as bootstrap:
                child = subprocess.Popen(["/usr/bin/python3", str(root / "probe.py"), "--enter", str(root), str(leaf_fd)],
                                         pass_fds=(leaf_fd,), start_new_session=True, stdin=subprocess.DEVNULL, stdout=bootstrap, stderr=subprocess.STDOUT)
                try:
                    deadline = time.monotonic() + 5
                    available = False
                    while child.poll() is None and time.monotonic() < deadline:
                        if scope_fd is None:
                            try:
                                scope_fd = os.open("init.scope", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=leaf_fd)
                            except FileNotFoundError:
                                pass
                        if service_fd is None:
                            try:
                                service_fd = os.open("dbus.service", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=leaf_fd)
                            except FileNotFoundError:
                                pass
                        result = subprocess.run(["/usr/bin/gdbus", "call", "--address", f"unix:path={runtime}/session-bus", "--dest", "org.freedesktop.DBus",
                                                 "--object-path", "/org/freedesktop/DBus", "--method", "org.freedesktop.DBus.GetNameOwner",
                                                 "org.freedesktop.systemd1"], capture_output=True, text=True, timeout=1)
                        if result.returncode == 0:
                            print("real_manager_owner", result.stdout.strip(), flush=True)
                            available = True
                            break
                        time.sleep(.01)
                    print("real_manager_available", available, flush=True)
                    require(available, "real manager API unavailable")
                    def call(destination, object_path, method, *arguments):
                        result = subprocess.run(["/usr/bin/gdbus", "call", "--address", f"unix:path={runtime}/session-bus",
                                                 "--dest", destination, "--object-path", object_path, "--method", method,
                                                 *arguments], capture_output=True, text=True, timeout=2)
                        require(result.returncode == 0, result.stderr[:2048])
                        require(len(result.stdout) < 16384, "oversized manager reply")
                        return result.stdout.strip()
                    manager = "org.freedesktop.systemd1"
                    manager_path = "/org/freedesktop/systemd1"
                    unit = "gnome-terminal-server.service"
                    from gi.repository import Gio, GLib
                    reference = Gio.DBusConnection.new_for_address_sync(f"unix:path={runtime}/session-bus",
                        Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, None, None)
                    reference.call_sync(manager, manager_path, manager + ".Manager", "RefUnit", GLib.Variant("(s)", (unit,)),
                                        None, Gio.DBusCallFlags.NO_AUTO_START, 500, None)
                    loaded = call(manager, manager_path, manager + ".Manager.LoadUnit", unit)
                    app_fd = os.open("app.slice", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=leaf_fd)
                    require(re.fullmatch(r"\(objectpath '(/org/freedesktop/systemd1/unit/[A-Za-z0-9_]+)',\)", loaded), "unexpected unit object")
                    unit_path = re.fullmatch(r"\(objectpath '([^']+)',\)", loaded).group(1)
                    def property_value(interface, name):
                        return call(manager, unit_path, "org.freedesktop.DBus.Properties.Get", interface, name)
                    print("real_manager_unit_fragment", property_value(manager + ".Unit", "FragmentPath"), flush=True)
                    print("real_manager_unit_exec", property_value(manager + ".Service", "ExecStart"), flush=True)
                    require(property_value(manager + ".Unit", "LoadState") == "(<'loaded'>,)", "GNOME definition not loaded")
                    print("real_manager_definition_reload", call(manager, manager_path, manager + ".Manager.Reload"), flush=True)
                    after_reload = property_value(manager + ".Unit", "LoadState")
                    print("real_manager_unit_loaded_after_reload", after_reload, flush=True)
                    require(after_reload == "(<'loaded'>,)", "GNOME definition lost after reload")
                    environment = {"PATH": "/usr/bin:/bin", "HOME": str(root / "home"),
                                   "XDG_RUNTIME_DIR": str(runtime), "DBUS_SESSION_BUS_ADDRESS": f"unix:path={runtime}/session-bus",
                                   "LYN_PRIVATE_NATIVE_ACTIVATION_ROOT": str(root),
                                   "LYN_PRIVATE_NATIVE_ACTIVATION_MODULE": str(module)}
                    digest = hashlib.sha256(module.read_bytes()).hexdigest()
                    stored = root / "state/modules" / digest / module.name
                    for action, expected in (("prepare-live-manager", "managed"), ("synchronize", "managed"),
                                             ("synchronize", "managed"), ("remove", "stock"),
                                             ("synchronize", "stock"), ("synchronize", "stock"),
                                             ("acknowledge-remove", "stock"), ("acknowledge-remove", "stock")):
                        test_environment = dict(environment, LYN_PRIVATE_NATIVE_FILE_ACTION=action, LYN_PRIVATE_SYNC_EXPECTED=expected)
                        subprocess.run([str(executable), "--exact", "integrations::gnome::tests::native_lifecycle_fixture_entry", "--test-threads=1", "--nocapture"],
                                       env=test_environment, check=True, timeout=8)
                        require(property_value(manager + ".Unit", "ActiveState") == "(<'inactive'>,)", "GNOME service unexpectedly activated")
                        definition = property_value(manager + ".Service", "ExecStart")
                        managed = str(root / "state") in definition
                        require(managed == (action == "remove" or (action == "synchronize" and expected == "managed")), "unexpected settled manager definition")
                        if action == "prepare-live-manager":
                            print("real_manager_pending_reload_flag", property_value(manager + ".Unit", "NeedDaemonReload"), flush=True)
                        require(stored.read_bytes() == module.read_bytes() and stored.stat().st_mode & 0o7777 == 0o400,
                                "retained module changed during lifecycle")
                        if action == "acknowledge-remove":
                            require(not (root / "state/uninstall-retired-v1.json").exists(), "retirement authority still present")
                        print("real_manager_lifecycle", action, flush=True)
                    reference.close_sync(None)
                    (root / "stop").touch(exist_ok=False)
                    child.wait(timeout=5)
                finally:
                    if child.poll() is None:
                        os.killpg(child.pid, signal.SIGKILL)
                        child.wait(timeout=3)
            print((root / "bootstrap.log").read_bytes()[:8192].decode(errors="replace"))
            if (root / "manager.log").exists():
                log = (root / "manager.log").read_bytes()[:8192]
                print(log.decode(errors="replace"))
                require(log.count(b"Reloading.\n") == 3, "expected initial diagnostic and exactly two coordinator reloads")
    finally:
        if child is not None and child.poll() is None:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=3)
        if bus is not None:
            bus.terminate()
            try:
                bus.wait(timeout=2)
            except subprocess.TimeoutExpired:
                bus.kill()
                bus.wait(timeout=2)
        if leaf_fd is not None:
            deadline = time.monotonic() + 2
            while True:
                event_fd = os.open("cgroup.events", os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=leaf_fd)
                try:
                    empty = b"populated 0\n" in os.read(event_fd, 4096)
                finally:
                    os.close(event_fd)
                if empty:
                    break
                require(time.monotonic() < deadline, f"private cgroup occupied: preserve {parent / name}")
                time.sleep(.01)
            current = os.stat(name, dir_fd=parent_fd, follow_symlinks=False)
            pinned = os.fstat(leaf_fd)
            require((current.st_dev, current.st_ino) == (pinned.st_dev, pinned.st_ino), "cgroup path replaced")
            remove_children(leaf_fd, {"init.scope": scope_fd, "dbus.service": service_fd, "app.slice": app_fd})
            if scope_fd is not None:
                os.close(scope_fd)
            if service_fd is not None:
                os.close(service_fd)
            if app_fd is not None:
                os.close(app_fd)
            current = os.stat(name, dir_fd=parent_fd, follow_symlinks=False)
            pinned = os.fstat(leaf_fd)
            require((current.st_dev, current.st_ino) == (pinned.st_dev, pinned.st_ino), "cgroup path replaced")
            os.close(leaf_fd)
            os.rmdir(name, dir_fd=parent_fd)
        os.close(parent_fd)


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "--enter":
        enter(Path(sys.argv[2]), int(sys.argv[3]))
    elif len(sys.argv) == 3 and sys.argv[1] == "--inner":
        inner(Path(sys.argv[2]))
    elif len(sys.argv) == 3:
        main(Path(sys.argv[1]).resolve(strict=True), Path(sys.argv[2]).resolve(strict=True))
    else:
        raise RuntimeError("Unknown probe entry")

