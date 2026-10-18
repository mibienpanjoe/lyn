"""Deterministic GTK initialization delay, separate from real GNOME admission."""

import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile

from stream_fixture import NativeStream


def run(module, executable):
    assert Path(os.environ["XDG_RUNTIME_DIR"]).parent.name.startswith("lyn-gnome-run-")
    with tempfile.TemporaryDirectory(prefix="lyn-gnome-startup-") as directory:
        diagnostic = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        endpoint = str(Path(directory) / "diagnostic.sock")
        diagnostic.bind(endpoint)
        native = NativeStream(directory)
        # The prototype's basename guard enables this GTK unit fixture. Rust's
        # production executable-inode admission never accepts this executable.
        server = subprocess.Popen([str(executable)], env=dict(os.environ,
            GTK3_MODULES=str(module), LYN_GNOME_TEST_SOCKET=endpoint,
            LYN_GNOME_TEST_STREAM=native.path), stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL)
        try:
            native.handshake(server)
            native.query({"windowId": 42}, status=(2, 5))
            assert server.poll() is None
            print("PASS: GTK fixture delayed 300ms before first dispatch; native handshake/query still bounded", flush=True)
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
    run(Path(sys.argv[1]).resolve(strict=True), Path(sys.argv[2]).resolve(strict=True))
