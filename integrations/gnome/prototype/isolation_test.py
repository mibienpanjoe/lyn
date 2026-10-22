"""Prove timeout kills only the new fixture group, including a nested process."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from unittest.mock import patch
import xml.etree.ElementTree as ET

import isolated_run
from isolated_run import run_fixture


def run():
    def inspect_bus(command, environment):
        config = Path(command[command.index("--config-file") + 1])
        listeners = ET.fromstring(config.read_text()).findall("listen")
        expected = "unix:path=" + str(Path(environment["XDG_RUNTIME_DIR"]) / "session-bus")
        assert [node.text for node in listeners] == [expected], "Fixture must explicitly request its private filesystem bus"
        return 0
    with patch.object(isolated_run, "run_fixture", side_effect=inspect_bus):
        assert isolated_run.main(["-", "-c", "pass"]) == 0
    code = """import json,os,subprocess,time
child=subprocess.Popen(['/usr/bin/sleep','30'])
print(json.dumps({'pids':[os.getpid(),child.pid],'group':os.getpgrp()}),flush=True)
time.sleep(30)
"""
    with tempfile.TemporaryFile(mode="w+") as output:
        started = time.monotonic()
        try:
            run_fixture([sys.executable, "-c", code], dict(os.environ), timeout=.5, stdout=output)
        except subprocess.TimeoutExpired:
            pass
        else:
            raise AssertionError("Stalled fixture was not stopped at its deadline")
        assert time.monotonic() - started < 3
        output.seek(0)
        frame = json.loads(output.read())
        assert frame["group"] != os.getpgrp(), "Fixture must not share our process group"
        assert frame["group"] == frame["pids"][0]
        for pid in frame["pids"]:
            try:
                fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
            except FileNotFoundError:
                continue
            # Orphan zombies may await PID1; no fixture may remain running.
            # A recycled numeric PID is not ours and is never signalled here.
            assert int(fields[3]) != frame["group"] or fields[0] == "Z"
    assert run_fixture([sys.executable, "-c", "pass"], dict(os.environ), timeout=2) == 0
    print("PASS: isolated deadline kills its private group and nested child, parent remains alive", flush=True)


if __name__ == "__main__":
    run()
