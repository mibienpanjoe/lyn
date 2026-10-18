"""Test-only control of the production Rust broker; no provider authentication shim."""

import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import time


class RustProbe:
    def __init__(self, directory, executable):
        self.runtime = Path(directory) / "rust-runtime"
        self.runtime.mkdir(mode=0o700)
        self.path = str(self.runtime / "lyn-gnome-v1.sock")
        self.control = None
        self.process = subprocess.Popen([
            str(executable), "--exact", "context::gnome_broker::probe_tests::real_gnome_probe_entry",
            "--nocapture", "--test-threads=1"],
            env=dict(os.environ, LYN_GNOME_PROBE_RUNTIME=str(self.runtime)),
            stdout=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 5
            while not (self.runtime / "probe.sock").exists():
                assert self.process.poll() is None, "Rust broker probe exited before readiness"
                assert time.monotonic() < deadline, "Rust broker probe did not become ready"
                time.sleep(.01)
            self.control = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.control.settimeout(3)
            self.control.connect(str(self.runtime / "probe.sock"))
            assert self.request({"kind": "status"}) == {"peers": []}
        except BaseException:
            self.close()
            raise

    def receive(self, size):
        result = bytearray()
        while len(result) < size:
            part = self.control.recv(size-len(result))
            assert part, "Rust probe disconnected during control reply"
            result.extend(part)
        return bytes(result)

    def request(self, value):
        payload = json.dumps(value).encode()
        assert len(payload) <= 2048
        self.control.sendall(struct.pack("!I", len(payload)) + payload)
        size = struct.unpack("!I", self.receive(4))[0]
        assert 0 < size <= 2048
        return json.loads(self.receive(size))

    def handshake(self, server):
        deadline = time.monotonic() + 3
        while self.request({"kind": "status"}) != {"peers": [server.pid]}:
            assert server.poll() is None, "Native GNOME server exited during Rust admission"
            assert time.monotonic() < deadline, "Real GNOME peer was not admitted by Rust"
            time.sleep(.01)

    def query(self, server, window, invoked_us=None):
        return self.request({"kind": "query", "serverPid": server,
                             "windowId": window, "invokedUs": invoked_us if invoked_us is not None
                             else time.monotonic_ns() // 1000})

    def close(self):
        if self.control:
            self.control.close()
            self.control = None
        try:
            code = self.process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=2)
            raise AssertionError("Rust probe did not stop after control EOF")
        assert code == 0, f"Rust broker probe exited with status {code}"

    def window_query(self, window):
        return self.request({"kind": "windowQuery", "windowId": window,
                             "invokedUs": time.monotonic_ns() // 1000})

    def pty_query(self, window):
        return self.request({"kind": "ptyQuery", "windowId": window,
                             "invokedUs": time.monotonic_ns() // 1000})

    def process_query(self, window, expected_directory):
        return self.request({"kind": "processQuery", "windowId": window,
                             "expectedDirectory": str(expected_directory)})
