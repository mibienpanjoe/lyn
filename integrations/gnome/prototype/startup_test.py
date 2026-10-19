"""Deterministic GTK initialization delay, separate from real GNOME admission."""

import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile

from stream_fixture import NativeStream


def run(module, executable, discovery=False):
    assert Path(os.environ["XDG_RUNTIME_DIR"]).parent.name.startswith("lyn-gnome-run-")
    with tempfile.TemporaryDirectory(prefix="lyn-gnome-startup-") as directory:
        diagnostic = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        endpoint = str(Path(directory) / "diagnostic.sock")
        diagnostic.bind(endpoint)
        runtime = Path(directory)
        if discovery:
            instance_name = "gn-" + "a" * 32
            instance = runtime / instance_name
            instance.mkdir(mode=0o700)
            native = NativeStream(instance, socket_name="lyn-gnome-v1.sock")
            Path(native.path).chmod(0o600)
            temporary = runtime / "publication.next"
            temporary.write_text(instance_name + "\n")
            temporary.chmod(0o600)
            temporary.replace(runtime / "lyn-gnome-v1.current")
        else:
            native = NativeStream(directory)
        # The prototype's basename guard enables this GTK unit fixture. Rust's
        # production executable-inode admission never accepts this executable.
        environment = dict(os.environ, GTK3_MODULES=str(module), LYN_GNOME_TEST_SOCKET=endpoint)
        environment.pop("LYN_GNOME_TEST_STREAM", None)
        environment.pop("LYN_GNOME_TEST_DISCOVERY", None)
        environment["LYN_GNOME_TEST_DISCOVERY" if discovery else "LYN_GNOME_TEST_STREAM"] = \
            str(runtime) if discovery else native.path
        server = subprocess.Popen([str(executable)], env=environment, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL)
        try:
            native.handshake(server)
            native.query({"windowId": 42}, status=(2, 5))
            assert server.poll() is None
            mode = "discovery" if discovery else "explicit"
            print(f"PASS: GTK fixture ({mode}) delayed 300ms before first dispatch; native handshake/query still bounded", flush=True)
        finally:
            native.close()
            diagnostic.close()
            if server.poll() is None:
                server.terminate()
            try:
                server.wait(timeout=3)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=2)


if __name__ == "__main__":
    for discovery in (False, True):
        run(Path(sys.argv[1]).resolve(strict=True), Path(sys.argv[2]).resolve(strict=True), discovery)
