"""Bounded real-GNOME resource observations; not installed/performance acceptance."""

import json
import os
from pathlib import Path
import select
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time

from desktop_isolation import require_owned_display
from desktop_lifecycle import _pin_owned_app
from stream_fixture import NativeStream


def stop_server(server, pin):
    """Only our unreaped direct child, with a kernel pin after admission."""
    def send(kind):
        try:
            if pin is not None:
                signal.pidfd_send_signal(pin, kind)
            elif kind == signal.SIGTERM:
                server.terminate()
            else:
                server.kill()
        except ProcessLookupError:
            pass  # Exit raced the signal; still reap our direct child below.
    try:
        if server.poll() is None:
            send(signal.SIGTERM)
        try:
            server.wait(timeout=3)
        except subprocess.TimeoutExpired:
            send(signal.SIGKILL)
            server.wait(timeout=2)
    finally:
        if pin is not None:
            os.close(pin)


def run(module, sdk):
    require_owned_display(sdk)
    runtime = Path(os.environ["XDG_RUNTIME_DIR"])
    assert runtime.name == "runtime" and runtime.parent.name.startswith("lyn-gnome-run-")
    ticks_per_second = os.sysconf("SC_CLK_TCK")
    page_size = os.sysconf("SC_PAGE_SIZE")
    client = "/usr/bin/gnome-terminal.real" if Path("/usr/bin/gnome-terminal.real").is_file() else "gnome-terminal"
    for enabled in (False, True):
        with tempfile.TemporaryDirectory(prefix="lyn-gnome-resource-") as directory:
            native = NativeStream(directory)
            diagnostic = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
            diagnostic.setsockopt(socket.SOL_SOCKET, socket.SO_PASSCRED, 1)
            endpoint = str(Path(directory) / "metrics.sock")
            diagnostic.bind(endpoint)
            diagnostic.setblocking(False)
            environment = dict(os.environ, GTK3_MODULES=str(module) if enabled else "", GTK_MODULES="",
                               LYN_GNOME_TEST_SOCKET=endpoint, LYN_GNOME_TEST_STREAM=native.path,
                               GSETTINGS_BACKEND="memory", NO_AT_BRIDGE="1")
            environment.pop("LYN_GNOME_TEST_DISCOVERY", None)
            app_id = "org.lyn.NativeResource" + ("Measured" if enabled else "Baseline")
            server = subprocess.Popen(["/usr/libexec/gnome-terminal-server", f"--app-id={app_id}"],
                                      env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            pin = None
            state = {"sources": [], "metrics": None}
            try:
                subprocess.run(["gdbus", "wait", "--session", "--timeout", "5", app_id], check=True, timeout=6)
                pin = _pin_owned_app(server, Path("/usr/libexec/gnome-terminal-server"), runtime,
                                     expected_cwd=Path.home())
                poller = select.poll()
                poller.register(pin, select.POLLIN)
                if enabled:
                    native.handshake(server)
                for _ in range(2):
                    subprocess.run([client, f"--app-id={app_id}", "--window", "--", "/usr/bin/sleep", "45"],
                                   env=environment, check=True, timeout=5,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

                def resources():
                    assert server.poll() is None and not poller.poll(0), "Owned GNOME process exited"
                    fields = Path(f"/proc/{server.pid}/stat").read_text().rsplit(")", 1)[1].split()
                    assert not poller.poll(0)
                    return {"clock": time.monotonic(), "ticks": int(fields[11]) + int(fields[12]),
                            "rssBytes": int(fields[21]) * page_size}

                def drain():
                    assert server.poll() is None and not poller.poll(0)
                    for _ in range(32):
                        try:
                            payload, ancillary, flags, _ = diagnostic.recvmsg(8192, socket.CMSG_SPACE(12))
                        except BlockingIOError:
                            break
                        assert not flags & (socket.MSG_TRUNC | socket.MSG_CTRUNC)
                        credentials = [struct.unpack("3i", data) for level, kind, data in ancillary
                                       if level == socket.SOL_SOCKET and kind == socket.SCM_CREDENTIALS]
                        assert credentials == [(server.pid, os.getuid(), os.getgid())]
                        frame = json.loads(payload)
                        assert frame["version"] == 0
                        if frame["kind"] == "snapshot":
                            state["sources"] = frame["sources"]
                        elif frame["kind"] == "metrics":
                            state["metrics"] = frame

                def wait_for(check):
                    deadline = time.monotonic() + 5
                    while time.monotonic() < deadline:
                        drain()
                        if check():
                            return
                        time.sleep(.01)
                    raise AssertionError("Private resource fixture did not settle")

                def mapped():
                    result = subprocess.run(["xdotool", "search", "--onlyvisible", "--pid", str(server.pid)],
                                            check=False, capture_output=True, text=True, timeout=1)
                    assert result.returncode in (0, 1)
                    return result.returncode == 0 and len(result.stdout.splitlines()) == 2

                wait_for(mapped)
                if enabled:
                    wait_for(lambda: len(state["sources"]) == 2)
                else:
                    assert str(module) not in Path(f"/proc/{server.pid}/maps").read_text()

                def report(phase, before, after, peak):
                    duration = after["clock"] - before["clock"]
                    cpu = (after["ticks"] - before["ticks"]) / ticks_per_second
                    print(json.dumps({"check": "native_resource_observation", "moduleEnabled": enabled,
                                      "phase": phase, "windows": 2, "wallSeconds": duration,
                                      "intervalStartMonotonicUs": int(before["clock"] * 1000000),
                                      "intervalEndMonotonicUs": int(after["clock"] * 1000000),
                                      "clockTicksPerSecond": ticks_per_second,
                                      "cpuTicks": after["ticks"] - before["ticks"],
                                      "cpuSeconds": cpu, "oneCoreCpuPercent": 100 * cpu / duration,
                                      "rssStartBytes": before["rssBytes"], "rssEndBytes": after["rssBytes"],
                                      "sampledPeakRssBytes": peak,
                                      "installedAcceptance": False}), flush=True)

                def report_callbacks():
                    metrics = state["metrics"]
                    assert metrics is not None
                    print(json.dumps({"check": "native_callback_cumulative_observation",
                                      "scope": "since_module_init_until_periodic_report",
                                      "bodyOnlyExcludesRecorderAndReport": True,
                                      "reportedAtMonotonicUs": metrics["reportedAtUs"],
                                      "observedAtMonotonicUs": time.monotonic_ns() // 1000,
                                      "snapshot": metrics["snapshot"], "query": metrics["query"]}), flush=True)

                before = resources()
                peak = before["rssBytes"]
                while time.monotonic() - before["clock"] < 3:
                    drain()
                    peak = max(peak, resources()["rssBytes"])
                    time.sleep(.01)
                report("idle", before, resources(), peak)
                if enabled:
                    wait_for(lambda: state["metrics"] is not None)
                    report_callbacks()
                    sources = state["sources"]
                    assert len({source["windowId"] for source in sources}) == 2
                    before = resources()
                    peak = before["rssBytes"]
                    for index in range(100):
                        native.query(sources[index % 2])
                        drain()
                        peak = max(peak, resources()["rssBytes"])
                    after = resources()
                    wait_for(lambda: state["metrics"] and state["metrics"]["query"]["count"] == 100)
                    metrics = state["metrics"]
                    assert metrics["snapshot"]["count"] >= 40
                    assert metrics["query"]["totalUs"] >= metrics["query"]["maxUs"]
                    report("100_queries", before, after, peak)
                    report_callbacks()
            finally:
                native.close()
                diagnostic.close()
                stop_server(server, pin)


if __name__ == "__main__":
    run(Path(sys.argv[1]).resolve(strict=True), None if sys.argv[2] == "-" else Path(sys.argv[2]))
