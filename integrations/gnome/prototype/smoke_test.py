"""Load a GTK module in a private GNOME Terminal server, not the user's server."""

import argparse
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import tempfile

from fixtures import repositories


def run(module):
    with tempfile.TemporaryDirectory(prefix="lyn-gnome-smoke-") as directory:
        projects = repositories(directory)
        assert len(projects) == 2
        endpoint = str(Path(directory) / "module.sock")
        with socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM) as channel:
            channel.setsockopt(socket.SOL_SOCKET, socket.SO_PASSCRED, 1)
            channel.bind(endpoint)
            channel.settimeout(8)
            environment = dict(os.environ, GTK3_MODULES=str(module),
                               LYN_GNOME_TEST_SOCKET=endpoint,
                               GSETTINGS_BACKEND="memory", NO_AT_BRIDGE="1")
            with tempfile.TemporaryFile() as logs:
                server = subprocess.Popen(
                    ["/usr/libexec/gnome-terminal-server", "--app-id=org.lyn.NativeSmoke"],
                    env=environment, stdout=logs, stderr=logs)
                try:
                    payload, ancillary, flags, _ = channel.recvmsg(512, socket.CMSG_SPACE(12))
                    assert not flags & socket.MSG_TRUNC, "Oversized identity frame"
                    credentials = [struct.unpack("3i", data) for level, kind, data in ancillary
                                   if level == socket.SOL_SOCKET and kind == socket.SCM_CREDENTIALS]
                    assert credentials == [(server.pid, os.getuid(), os.getgid())], credentials
                    assert json.loads(payload) == {
                        "version": 0, "kind": "loaded", "processId": server.pid}, payload
                    assert server.poll() is None, "Terminal server exited after module initialization"
                    subprocess.run(["gdbus", "wait", "--session", "--timeout", "5",
                                    "org.lyn.NativeSmoke"], check=True)
                    owner = subprocess.check_output([
                        "gdbus", "call", "--session", "--dest", "org.freedesktop.DBus",
                        "--object-path", "/org/freedesktop/DBus", "--method",
                        "org.freedesktop.DBus.GetConnectionUnixProcessID", "org.lyn.NativeSmoke"],
                        text=True)
                    assert f"uint32 {server.pid}" in owner, owner
                    executable = os.readlink(f"/proc/{server.pid}/exe")
                    assert executable == "/usr/libexec/gnome-terminal-server", executable
                    print(json.dumps({"check": "gtk_module_loaded_in_real_gnome_server",
                                      "peerPid": server.pid, "pass": True}))
                except Exception:
                    logs.seek(0)
                    print(logs.read().decode(errors="replace"))
                    raise
                finally:
                    if server.poll() is None:
                        server.terminate()
                    try:
                        server.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        server.kill()
                        server.wait(timeout=2)
                assert server.poll() is not None, "Fixture server not cleaned up"
                print("PASS: isolated module handshake authenticated; fixture server stopped")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("module", type=Path)
    arguments = parser.parse_args()
    module = arguments.module.resolve(strict=True)
    assert "DBUS_SESSION_BUS_ADDRESS" in os.environ, "Run with dbus-run-session"
    run(module)
