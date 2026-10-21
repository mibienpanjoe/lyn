"""Real GNOME metadata must reconnect to the production-policy Rust broker."""

import ctypes
import json
import os
from pathlib import Path
import select
import socket
import struct
import subprocess
import sys
import tempfile
import time

from fixtures import repositories
from rust_probe import RustProbe
from desktop_isolation import require_owned_display
from desktop_lifecycle import _pin_owned_app
from resource_test import stop_server


def run(module, executable, cycles=1, sdk=None, published=False):
    assert 1 <= cycles <= 20
    require_owned_display(sdk)
    assert Path(os.environ["XDG_RUNTIME_DIR"]).parent.name.startswith("lyn-gnome-run-")
    with tempfile.TemporaryDirectory(prefix="lyn-gnome-reconnect-") as directory:
        root = Path(directory)
        project = repositories(root, ("alpha",))[0]
        diagnostics = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        diagnostics.setsockopt(socket.SOL_SOCKET, socket.SO_PASSCRED, 1)
        diagnostics.bind(str(root / "diagnostic.sock"))
        diagnostics.settimeout(.25)
        native_runtime = root / "rust-runtime"
        native_path = native_runtime / "lyn-gnome-v1.sock"
        environment = dict(os.environ, GTK3_MODULES=str(module),
                           LYN_GNOME_TEST_SOCKET=str(root / "diagnostic.sock"))
        environment.pop("LYN_GNOME_TEST_STREAM", None)
        environment.pop("LYN_GNOME_TEST_DISCOVERY", None)
        environment["LYN_GNOME_TEST_DISCOVERY" if published else "LYN_GNOME_TEST_STREAM"] = str(
            native_runtime if published else native_path)
        library = ctypes.CDLL(str(module)) if published else None
        if library:
            discover = library.lyn_native_discover
            discover.argtypes = [ctypes.c_char_p, ctypes.c_void_p]
            discover.restype = ctypes.c_bool

        def endpoint():
            if not published:
                return native_path
            address = ctypes.create_string_buffer(108)
            assert discover(os.fsencode(native_runtime), address), "Published endpoint is not discoverable"
            return Path(os.fsdecode(address.value))
        server = subprocess.Popen(["/usr/libexec/gnome-terminal-server", "--app-id=org.lyn.NativeReconnect"],
                                  env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        broker = None
        pin = None
        try:
            subprocess.run(["gdbus", "wait", "--session", "--timeout", "5", "org.lyn.NativeReconnect"],
                           check=True, timeout=6)
            pin = _pin_owned_app(server, Path("/usr/libexec/gnome-terminal-server"),
                                 Path(os.environ["XDG_RUNTIME_DIR"]), expected_cwd=Path.home())
            poller = select.poll()
            poller.register(pin, select.POLLIN)

            def descriptors():
                assert server.poll() is None and not poller.poll(0), "Owned GNOME server exited"
                count = len(list(Path(f"/proc/{server.pid}/fd").iterdir()))
                assert not poller.poll(0), "Owned GNOME exited during FD observation"
                return count
            client = "/usr/bin/gnome-terminal.real" if Path("/usr/bin/gnome-terminal.real").is_file() else "gnome-terminal"
            subprocess.run([client, "--app-id=org.lyn.NativeReconnect", "--window",
                            f"--working-directory={project}", "--", "/usr/bin/sleep", "60"],
                           check=True, timeout=5)
            source = None
            snapshots = 0
            deadline = time.monotonic() + 5
            # Establish actual GTK dispatch with a mapped native terminal before
            # the broker exists. No connection or capture retry by this test.
            while snapshots < 3:
                assert time.monotonic() < deadline, "Native terminal did not dispatch"
                try:
                    payload, ancillary, flags, _ = diagnostics.recvmsg(8192, socket.CMSG_SPACE(12))
                except socket.timeout:
                    continue
                assert not flags & (socket.MSG_TRUNC | socket.MSG_CTRUNC)
                credentials = [struct.unpack("3i", data) for level, kind, data in ancillary
                               if level == socket.SOL_SOCKET and kind == socket.SCM_CREDENTIALS]
                assert credentials == [(server.pid, os.getuid(), os.getgid())]
                frame = json.loads(payload)
                if frame["kind"] == "snapshot" and len(frame["sources"]) == 1 and frame["sources"][0]["active"]:
                    source = frame["sources"][0]
                    snapshots += 1
            assert not native_path.exists()
            broker = RustProbe(root, executable, published=published)
            broker.handshake(server)
            endpoints = {endpoint()}
            initial = broker.window_query(source["windowId"])
            assert initial["status"] == "candidate" and initial["sourceId"] == source["sourceId"], initial
            assert initial["serverPid"] == server.pid
            original_metadata = {key: value for key, value in initial.items() if key != "generation"}
            expected_save = {"status": "saved", "bodyMatches": True, "projectMatches": True,
                             "branchMatches": True, "duplicateRefused": True, "captureCount": 1,
                             "leaseReleased": True}
            assert broker.capture_probe(source["windowId"], project) == expected_save
            generations = {initial["generation"]}
            baseline_fds = descriptors()
            started = time.monotonic()
            for _ in range(cycles):
                previous_endpoint = endpoint()
                broker.close()
                broker = None
                assert not previous_endpoint.exists(), "Clean shutdown retained its owned socket"
                if not published:
                    native_runtime.rmdir()  # Empty fixture-owned directory only.
                broker = RustProbe(root, executable, published=published)
                broker.handshake(server)
                current_endpoint = endpoint()
                if published:
                    assert current_endpoint not in endpoints, "Publication reused an earlier instance"
                    assert not previous_endpoint.parent.exists(), "Clean shutdown retained its owned instance"
                endpoints.add(current_endpoint)
                current = broker.window_query(source["windowId"])
                assert {key: value for key, value in current.items() if key != "generation"} == original_metadata, current
                assert current["generation"] not in generations, "Reconnect reused a retired generation"
                generations.add(current["generation"])
                saved = broker.capture_probe(source["windowId"], project)
                assert saved == expected_save, saved
            final_fds = descriptors()
            assert final_fds == baseline_fds, ("Native GNOME FD count changed", baseline_fds, final_fds)
            print(json.dumps({"check": "native_broker_reconnect", "lateBrokerAdmitted": True,
                              "sameServerAndSource": True, "generationChanged": True,
                              "lateCaptureSavedOnce": True, "captureSavedOnce": True,
                              "reconnectCycles": cycles, "distinctGenerations": len(generations),
                              "endpointMode": "published" if published else "fixed",
                              "distinctEndpoints": len(endpoints),
                              "baselineFds": baseline_fds, "finalFds": final_fds,
                              "interactionSeconds": round(time.monotonic() - started, 3)}), flush=True)
        finally:
            try:
                if broker:
                    broker.close()
            finally:
                try:
                    stop_server(server, pin)
                finally:
                    diagnostics.close()


if __name__ == "__main__":
    run(Path(sys.argv[1]).resolve(strict=True), Path(sys.argv[2]).resolve(strict=True),
        int(sys.argv[3]) if len(sys.argv) > 3 else 1,
        Path(sys.argv[4]) if len(sys.argv) > 4 and sys.argv[4] != "-" else None,
        len(sys.argv) > 5 and sys.argv[5] == "published")
