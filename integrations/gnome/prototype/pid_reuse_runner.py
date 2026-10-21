"""Opt-in PID reuse in a throwaway user/PID/mount namespace, never the host."""
import os
from pathlib import Path
import sys
import tempfile
import uuid

from isolated_run import run_fixture


ENTRY = "context::gnome_peer::tests::pid_reuse::isolated_pid_reuse_entry"


def completed(output, nonce):
    return (output.splitlines().count(f"LYN_PID_REUSE_PROVED:{nonce}") == 1
            and "test result: ok. 1 passed; 0 failed; 0 ignored;" in output)


def run(executable):
    assert os.geteuid() > 0 and os.getegid() > 0, "Run as the ordinary developer, never host root"
    nonce = str(uuid.uuid4())
    environment = dict(os.environ, LYN_PID_REUSE_NONCE=nonce,
                       LYN_PID_REUSE_OUTSIDE_UID=str(os.geteuid()),
                       LYN_PID_REUSE_OUTSIDE_GID=str(os.getegid()))
    for kind in ("pid", "mnt", "user"):
        environment[f"LYN_PID_REUSE_PARENT_{kind.upper()}"] = os.readlink(f"/proc/self/ns/{kind}")
    # Reuse the existing private session/group timeout and reap discipline.
    # It covers pre-exec failures before the inner five-second alarm is armed.
    with tempfile.TemporaryDirectory(prefix="lyn-pid-reuse-") as owned_root, \
            tempfile.TemporaryFile(dir=owned_root) as output:
        environment["TMPDIR"] = owned_root
        result = run_fixture(["/usr/bin/unshare", "--user", "--map-root-user", "--pid", "--fork",
                              "--mount-proc", "--kill-child", str(executable.resolve(strict=True)),
                              "--exact", ENTRY, "--nocapture", "--test-threads=1"],
                             environment, timeout=8, stdout=output)
        output.seek(0)
        data = output.read(65537)
        assert len(data) <= 65536, "Oversized PID fixture output"
    text = data.decode("utf-8")
    assert result == 0 and completed(text, nonce), f"PID reuse proof missing (exit {result}): {text}"
    print("PASS: actual PID64 reuse; fresh peer admitted, original dead pidfd refused even with refreshed metadata; host counter never written", flush=True)


if __name__ == "__main__":
    run(Path(sys.argv[1]))
