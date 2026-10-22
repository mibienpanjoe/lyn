"""Bounded GTK tab actions concurrent with fixture metadata queries, not capture."""
import json
import os
from pathlib import Path
import re
import select
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time

from desktop_isolation import require_owned_display
from desktop_lifecycle import _pin_owned_app
from resource_test import stop_server
from stream_fixture import NativeStream

APP_ID = "org.lyn.NativeEventStorm"


def action_path(prop):
    match = re.fullmatch(r'_GTK_WINDOW_OBJECT_PATH\(UTF8_STRING\) = '
                         r'"(/org/lyn/NativeEventStorm/window/[1-9][0-9]*)"\n?', prop)
    assert match, "Action path is not the private fixture's window"
    return match[1]


def checked_reply(reply, request, sources, received_us):
    assert len(reply) == len(request) == 112
    assert reply[:8] == b"LYGN\x00\x01\x00\x05" and reply[8:44] == request[8:44]
    assert not any(reply[92:94] + reply[96:112])
    invoked = struct.unpack_from("!Q", request, 60)[0]
    observed = struct.unpack_from("!Q", reply, 60)[0]
    assert invoked <= observed <= received_us and received_us - invoked < 120000
    status = struct.unpack_from("!H", reply, 94)[0]
    assert 1 <= status <= 5
    if status == 1:
        pty, pgid, identity = struct.unpack_from("!IIQ", reply, 44)
        changed, revision, registered = struct.unpack_from("!QQQ", reply, 68)
        assert identity in sources, "Candidate substituted an unknown source"
        original = sources[identity]
        assert (pty, pgid, registered) == (original["ptyNumber"], original["foregroundPgid"],
                                           original["registeredAtUs"])
        assert 0 < registered <= original["changedAtUs"] <= changed < invoked
        assert revision >= original["revision"]
    else:
        assert not any(reply[44:60] + reply[68:92])
    return status


def selection_observed(sources, originals, expected, after_us):
    return (len(sources) == 2 and {s["sourceId"] for s in sources} == set(originals)
            and all(s["observedAtUs"] >= after_us for s in sources)
            and [s["sourceId"] for s in sources if s["active"]] == [expected])


def run(module, sdk):
    require_owned_display(sdk)
    runtime = Path(os.environ["XDG_RUNTIME_DIR"])
    assert runtime.name == "runtime" and runtime.parent.name.startswith("lyn-gnome-run-")
    with tempfile.TemporaryDirectory(prefix="lyn-gnome-storm-") as directory, tempfile.TemporaryFile() as logs:
        root = Path(directory)
        native = NativeStream(root)
        diagnostic = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        diagnostic.setsockopt(socket.SOL_SOCKET, socket.SO_PASSCRED, 1)
        diagnostic.bind(str(root / "diagnostic.sock"))
        diagnostic.setblocking(False)
        config = root / "config"
        config.mkdir(mode=0o700)
        environment = dict(os.environ, GTK3_MODULES=str(module), GTK_MODULES="",
                           LYN_GNOME_TEST_SOCKET=str(root / "diagnostic.sock"),
                           LYN_GNOME_TEST_STREAM=native.path, NO_AT_BRIDGE="1",
                           XDG_CONFIG_HOME=str(config), GSETTINGS_BACKEND="keyfile")
        environment.pop("LYN_GNOME_TEST_DISCOVERY", None)
        profile_id = subprocess.check_output(["gsettings", "get", "org.gnome.Terminal.ProfilesList", "default"],
                                             env=environment, text=True, timeout=2).strip().strip("'")
        assert re.fullmatch(r"[0-9a-f-]{36}", profile_id)
        profile = f"org.gnome.Terminal.Legacy.Profile:/org/gnome/terminal/legacy/profiles:/:{profile_id}/"
        for key, value in (("use-custom-command", "true"), ("custom-command", "/usr/bin/sleep 45")):
            subprocess.run(["gsettings", "set", profile, key, value], env=environment, check=True, timeout=2)
        server = subprocess.Popen(["/usr/libexec/gnome-terminal-server", f"--app-id={APP_ID}"],
                                  env=environment, stdout=logs, stderr=logs)
        pin = None
        worker = None
        done = threading.Event()
        state = {"sources": [], "metrics": None}
        errors, observations, progress = [], [], [0]
        try:
            subprocess.run(["gdbus", "wait", "--session", "--timeout", "5", APP_ID], check=True, timeout=6)
            pin = _pin_owned_app(server, Path("/usr/libexec/gnome-terminal-server"), runtime,
                                 expected_cwd=Path.home())
            poller = select.poll()
            poller.register(pin, select.POLLIN)
            native.handshake(server)
            client = "/usr/bin/gnome-terminal.real" if Path("/usr/bin/gnome-terminal.real").is_file() else "gnome-terminal"
            subprocess.run([client, f"--app-id={APP_ID}", "--window", "--", "/usr/bin/sleep", "45"],
                           env=environment, check=True, timeout=5,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

            def resources():
                assert server.poll() is None and not poller.poll(0), "Private GNOME exited"
                fields = Path(f"/proc/{server.pid}/stat").read_text().rsplit(")", 1)[1].split()
                result = {"atUs": time.monotonic_ns() // 1000, "ticks": int(fields[11]) + int(fields[12]),
                          "rssBytes": int(fields[21]) * os.sysconf("SC_PAGE_SIZE"),
                          "fds": len(list(Path(f"/proc/{server.pid}/fd").iterdir()))}
                assert not poller.poll(0), "Private GNOME exited during resource observation"
                return result

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

            def wait_for(predicate):
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline:
                    drain()
                    if predicate():
                        return
                    time.sleep(.005)
                raise AssertionError(f"Private event-storm fixture did not settle: {state['sources']!r}")

            wait_for(lambda: len(state["sources"]) == 1 and state["sources"][0]["active"])
            window = state["sources"][0]["windowId"]
            path = action_path(subprocess.check_output(
                ["xprop", "-id", str(window), "_GTK_WINDOW_OBJECT_PATH"], text=True, timeout=2))
            subprocess.run(["gdbus", "call", "--session", "--dest", APP_ID, "--object-path", path,
                            "--method", "org.gtk.Actions.Activate", "new-terminal", "[<('tab', 'current')>]", "{}"],
                           check=True, timeout=2, stdout=subprocess.DEVNULL)
            wait_for(lambda: len(state["sources"]) == 2 and sum(s["active"] for s in state["sources"]) == 1)
            assert {source["windowId"] for source in state["sources"]} == {window}

            def switch(index):
                subprocess.run(["gdbus", "call", "--session", "--dest", APP_ID, "--object-path", path,
                                "--method", "org.gtk.Actions.SetState", "active-tab", f"<int32 {index}>", "{}"],
                               check=True, timeout=2, stdout=subprocess.DEVNULL)

            switch(1)
            after_switch = time.monotonic_ns() // 1000
            wait_for(lambda: len(state["sources"]) == 2 and all(
                source["observedAtUs"] >= after_switch for source in state["sources"]))
            originals = {source["sourceId"]: dict(source) for source in state["sources"]}
            assert len(originals) == 2 and len({s["ptyNumber"] for s in originals.values()}) == 2
            initial_active = [s for s in originals.values() if s["active"]]
            assert len(initial_active) == 1
            tab_sources = [next(identity for identity in originals
                                if identity != initial_active[0]["sourceId"]),
                           initial_active[0]["sourceId"]]
            native.query(initial_active[0])

            def query_during_actions():
                try:
                    while not done.is_set():
                        assert len(observations) < 10000, "Metadata workload exceeded its fixture bound"
                        invoked = time.monotonic_ns() // 1000
                        phase = min(progress[0] // 100, 1)
                        native.counter += 1
                        request = bytearray(native.vectors["query"])
                        request[24:40] = native.counter.to_bytes(16, "big")
                        struct.pack_into("!I", request, 40, window)
                        struct.pack_into("!Q", request, 60, invoked)
                        native.channel.settimeout(.12)
                        native.channel.sendall(request)
                        reply = bytearray()
                        while len(reply) < 112:
                            remaining = (invoked + 120000 - time.monotonic_ns() // 1000) / 1000000
                            assert remaining > 0, "Metadata response exceeded its original 120ms deadline"
                            native.channel.settimeout(remaining)
                            part = native.channel.recv(112 - len(reply))
                            assert part, "Module disconnected during event storm"
                            reply.extend(part)
                        received = time.monotonic_ns() // 1000
                        status = checked_reply(reply, request, originals, received)
                        observations.append((phase, status, received - invoked))
                        time.sleep(.001)
                except BaseException as error:
                    errors.append(error)
                    done.set()

            before = resources()
            peak = before["rssBytes"]
            worker = threading.Thread(target=query_during_actions, daemon=True)
            worker.start()
            for index in range(200):
                assert not errors, errors
                assert time.monotonic_ns() // 1000 - before["atUs"] < 20000000
                switch(index % 2)
                action_done = time.monotonic_ns() // 1000
                wait_for(lambda: selection_observed(state["sources"], originals,
                                                     tab_sources[index % 2], action_done))
                progress[0] = index + 1
                drain()
                peak = max(peak, resources()["rssBytes"])
            done.set()
            worker.join(timeout=1)
            assert not worker.is_alive() and not errors, errors
            after = resources()
            peak = max(peak, after["rssBytes"])
            assert before["fds"] == after["fds"], "GNOME retained descriptors after the storm"
            assert all(any(phase == half for phase, _, _ in observations) for half in (0, 1))
            assert any(status == 1 for _, status, _ in observations)
            wait_for(lambda: len(state["sources"]) == 2 and all(
                s["observedAtUs"] >= after["atUs"] for s in state["sources"]))
            for source in state["sources"]:
                original = originals[source["sourceId"]]
                for key in ("windowId", "ptyNumber", "foregroundPgid", "registeredAtUs"):
                    assert source[key] == original[key]
                assert source["revision"] >= original["revision"] + 200, "Requested switches did not occur"
            active = [s for s in state["sources"] if s["active"]]
            assert len(active) == 1 and active[0]["sourceId"] == initial_active[0]["sourceId"]
            native.channel.settimeout(2)
            native.query(active[0])
            wait_for(lambda: state["metrics"] and state["metrics"]["query"]["count"] == len(observations) + 2)
            print(json.dumps({"check": "native_event_storm", "tabSwitches": 200,
                              "metadataQueries": len(observations), "candidateReplies": sum(s == 1 for _, s, _ in observations),
                              "maxQueryRoundTripUs": max(t for _, _, t in observations),
                              "resourceIntervalStartUs": before["atUs"], "resourceIntervalEndUs": after["atUs"],
                              "cpuTicks": after["ticks"] - before["ticks"], "clockTicksPerSecond": os.sysconf("SC_CLK_TCK"),
                              "rssStartBytes": before["rssBytes"], "rssEndBytes": after["rssBytes"], "sampledPeakRssBytes": peak,
                              "fdsBefore": before["fds"], "fdsAfter": after["fds"],
                              "callbackScope": "cumulative_since_module_init", "callbacks": state["metrics"],
                              "installedAcceptance": False}), flush=True)
        except BaseException:
            logs.seek(0)
            print(logs.read(262144).decode(errors="replace"), flush=True)
            raise
        finally:
            done.set()
            if worker:
                worker.join(timeout=1)
            native.close()
            diagnostic.close()
            stop_server(server, pin)


if __name__ == "__main__":
    run(Path(sys.argv[1]).resolve(strict=True), None if sys.argv[2] == "-" else Path(sys.argv[2]))
