"""Simulated systemd API, including guarded test-only definition reload."""

import os
from pathlib import Path
import socket
import stat
import sys
import time

import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib
from desktop_isolation import require_owned_display


def private_bus_address(address, runtime):
    path = str(runtime / "session-bus")
    if not path.isascii() or any(character not in "/-_.0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
                                 for character in path):
        return False
    fields = address.split(",")
    if fields[0] != f"unix:path={path}":
        return False
    if len(fields) == 1:
        return True
    if len(fields) != 2 or not fields[1].startswith("guid="):
        return False
    guid = fields[1][5:]
    return len(guid) == 32 and all(character in "0123456789abcdefABCDEF" for character in guid)


def run(root, mode, ready, sdk):
    require_owned_display(sdk)
    runtime = Path(os.environ["XDG_RUNTIME_DIR"])
    if runtime.name != "runtime" or runtime.parent.parent != Path("/tmp") or not runtime.parent.name.startswith("lyn-gnome-run-"):
        raise RuntimeError("Expected the isolated runtime")
    for path in (root, runtime.parent, runtime):
        metadata = path.lstat()
        if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.geteuid() \
                or metadata.st_mode & 0o7777 != 0o700 or path.resolve(strict=True) != path:
            raise RuntimeError("Expected private owned directories without symlinks")
    metadata = (runtime / "session-bus").lstat()
    if not stat.S_ISSOCK(metadata.st_mode) or metadata.st_uid != os.geteuid():
        raise RuntimeError("Expected the private bus socket")
    address = os.environ["DBUS_SESSION_BUS_ADDRESS"]
    if not private_bus_address(address, runtime):
        raise RuntimeError("Only the exact private fixture bus is permitted")
    if root.parent != Path("/tmp") or not root.name.startswith("lyn-na-") or ready.parent != root:
        raise RuntimeError("Expected the isolated activation root")
    sync_modes = ("sync-install", "sync-remove", "sync-stuck", "sync-error", "sync-hang", "sync-changed", "sync-owner", "sync-during-validation",
                  "ack-remove", "ack-stock", "ack-owner", "ack-stuck", "ack-after-unlink")
    if mode not in ("stock", "loaded", "pending", "changing", *sync_modes):
        raise RuntimeError("Unknown private manager mode")
    unit = "gnome-terminal-server.service"
    unit_path = "/org/freedesktop/systemd1/unit/gnome_2dterminal_2dserver_2eservice"
    manager = "org.freedesktop.systemd1.Manager"
    unit_interface = "org.freedesktop.systemd1.Unit"
    service_interface = "org.freedesktop.systemd1.Service"
    server = "/usr/libexec/gnome-terminal-server"
    launcher = str(root / "state/launcher-v1")
    drop_in = str(root / "units" / f"{unit}.d/90-lyn-native.conf")
    stock = mode in ("stock", "pending") or mode in sync_modes and mode not in ("sync-remove", "ack-remove", "ack-stuck", "ack-after-unlink")
    executable = server if stock else launcher
    properties = {
        "LoadState": GLib.Variant("s", "loaded"),
        "FragmentPath": GLib.Variant("s", f"/usr/lib/systemd/user/{unit}"),
        "DropInPaths": GLib.Variant("as", [] if stock else [drop_in]),
        "NeedDaemonReload": GLib.Variant("b", mode == "pending"),
        "Transient": GLib.Variant("b", False),
        "Names": GLib.Variant("as", [unit]),
        "Type": GLib.Variant("s", "dbus"),
        "BusName": GLib.Variant("s", "org.gnome.Terminal"),
        "ExecStart": GLib.Variant("a(sasbttttuii)", [(executable, [executable], False, 0, 0, 0, 0, 0, 0, 0)]),
    }
    for name in ("ExecStartPre", "ExecStartPost", "ExecCondition"):
        properties[name] = GLib.Variant("a(sasbttttuii)", [])
    connection = Gio.DBusConnection.new_for_address_sync(
        address, Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
        None, None)
    connection.set_exit_on_close(False)
    deadline = time.monotonic() + 2
    while True:
        reply = connection.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
                                     "RequestName", GLib.Variant("(su)", ("org.freedesktop.systemd1", 4)),
                                     GLib.VariantType.new("(u)"), Gio.DBusCallFlags.NO_AUTO_START, 500, None)
        if reply.unpack() == (1,):
            break
        assert time.monotonic() < deadline, "Private fixture manager name is occupied"
        time.sleep(.005)

    reload_count = 0
    reload_sender = None
    post_same_client = False
    held_invocations = []
    takeover = []
    first_inspection_sender = None
    post_ack_same_client = False
    failed_after_unlink = False

    def load_unit(_connection, _sender, _path, _interface, method, arguments, invocation):
        nonlocal reload_count, reload_sender, first_inspection_sender
        if method == "MutateDefinition":
            if mode != "sync-during-validation" or arguments.unpack() != ():
                invocation.return_dbus_error("org.lyn.Private.Refused", "Mutation is not enabled in this fixture mode")
                return
            properties["ExecStart"] = GLib.Variant("a(sasbttttuii)", [("/synthetic/unowned", ["/synthetic/unowned"], False, 0, 0, 0, 0, 0, 0, 0)])
            invocation.return_value(GLib.Variant("()", ()))
            return
        if method == "Reload":
            if mode not in sync_modes or arguments.unpack() != ():
                invocation.return_dbus_error("org.lyn.Private.Refused", "Reload is not enabled in this fixture mode")
                return
            reload_count += 1
            reload_sender = _sender
            if mode == "sync-error":
                invocation.return_dbus_error("org.lyn.Private.Refused", "Synthetic reload error")
                return
            if mode == "sync-hang":
                held_invocations.append(invocation)
                return
            if mode in ("sync-install", "sync-remove", "ack-remove", "ack-after-unlink"):
                next_executable = launcher if mode == "sync-install" else server
                properties["ExecStart"] = GLib.Variant("a(sasbttttuii)", [(next_executable, [next_executable], False, 0, 0, 0, 0, 0, 0, 0)])
                properties["DropInPaths"] = GLib.Variant("as", [drop_in] if mode == "sync-install" else [])
                properties["NeedDaemonReload"] = GLib.Variant("b", False)
            invocation.return_value(GLib.Variant("()", ()))
            return
        assert method == "LoadUnit" and arguments.unpack() == (unit,)
        if first_inspection_sender is None:
            first_inspection_sender = _sender
        invocation.return_value(GLib.Variant("(o)", (unit_path,)))

    start_reads = 0

    def get_property(_connection, _sender, _path, interface, name):
        nonlocal start_reads, post_same_client, post_ack_same_client, failed_after_unlink
        if interface == manager:
            if name == "ReloadCount":
                return GLib.Variant("u", reload_count)
            if name == "PostReloadSameClient":
                return GLib.Variant("b", post_same_client)
            if name == "PostAckSameClient":
                return GLib.Variant("b", post_ack_same_client)
            assert name == "UnitPath"
            return GLib.Variant("as", [str(root / "units")])
        if name == "ExecStart":
            start_reads += 1
            if reload_sender is not None and _sender == reload_sender:
                post_same_client = True
            if mode.startswith("ack-") and not (root / "state/uninstall-retired-v1.json").exists():
                if _sender == first_inspection_sender:
                    post_ack_same_client = True
                if mode == "ack-after-unlink" and not failed_after_unlink:
                    failed_after_unlink = True
                    return GLib.Variant("a(sasbttttuii)", [("/synthetic/unowned", ["/synthetic/unowned"], False, 0, 0, 0, 0, 0, 0, 0)])
            if mode in ("changing", "sync-changed") and start_reads > 1:
                return GLib.Variant("a(sasbttttuii)", [(executable, [executable], False, 0, 0, 0, 0, 1, 0, 0)])
            if mode in ("sync-owner", "ack-owner") and start_reads == 3:
                connection.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "ReleaseName",
                                     GLib.Variant("(s)", ("org.freedesktop.systemd1",)), None, Gio.DBusCallFlags.NO_AUTO_START, 500, None)
                replacement = Gio.DBusConnection.new_for_address_sync(address,
                    Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, None, None)
                replacement.set_exit_on_close(False)
                reply = replacement.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "RequestName",
                    GLib.Variant("(su)", ("org.freedesktop.systemd1", 4)), None, Gio.DBusCallFlags.NO_AUTO_START, 500, None)
                assert reply.unpack() == (1,)
                takeover.append(replacement)
        return properties[name]

    manager_xml = f'''<node><interface name="{manager}">
      <method name="LoadUnit"><arg type="s" direction="in"/><arg type="o" direction="out"/></method>
      <method name="Reload"/>
      <method name="MutateDefinition"/>
      <property name="UnitPath" type="as" access="read"/>
      <property name="ReloadCount" type="u" access="read"/>
      <property name="PostReloadSameClient" type="b" access="read"/>
      <property name="PostAckSameClient" type="b" access="read"/></interface></node>'''
    connection.register_object("/org/freedesktop/systemd1", Gio.DBusNodeInfo.new_for_xml(manager_xml).interfaces[0],
                               load_unit, get_property, None)
    for interface, names in (
            (unit_interface, ("LoadState", "FragmentPath", "DropInPaths", "NeedDaemonReload", "Transient", "Names")),
            (service_interface, ("Type", "BusName", "ExecStart", "ExecStartPre", "ExecStartPost", "ExecCondition"))):
        fields = "".join(f'<property name="{name}" type="{properties[name].get_type_string()}" access="read"/>'
                         for name in names)
        xml = f'<node><interface name="{interface}">{fields}</interface></node>'
        connection.register_object(unit_path, Gio.DBusNodeInfo.new_for_xml(xml).interfaces[0], None, get_property, None)
    with socket.socket(socket.AF_UNIX) as channel:
        channel.settimeout(2)
        channel.connect(str(ready))
        channel.sendall(b"R")
    GLib.MainLoop().run()


if __name__ == "__main__":
    run(Path(sys.argv[1]).resolve(strict=True), sys.argv[2], Path(sys.argv[3]),
        Path(sys.argv[4]).resolve(strict=True) if sys.argv[4] != "-" else None)
