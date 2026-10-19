"""Fixed shell/TUI recipes on private synthetic projects, never user profiles."""

import os
from pathlib import Path
import signal
import stat
import sys
import time

from cwd_fixture import destination


def recipe(shape, beta, config):
    scripts = {
        "shell": "/usr/bin/sleep 40 & wait",
        "exec-tui": 'exec /usr/bin/htop --pid "$$"',
        "child-tui": 'set -m; /usr/bin/env -C "$1" /usr/bin/htop --pid "$$"; wait',
    }
    assert shape in scripts
    environment = {"PATH": "/usr/bin:/bin", "TERM": "xterm-256color", "LANG": "C.UTF-8",
                   "XDG_CONFIG_HOME": str(config), "HTOPRC": str(config / "fixture-htoprc")}
    return ["/bin/bash", "--noprofile", "--norc", "-c", scripts[shape], "lyn-fixture", str(beta)], environment


def ready(shape, source, server, alpha, beta):
    pid = source["foregroundPgid"]
    rows = []
    def process(number):
        root = Path(f"/proc/{number}")
        row = (root / "stat").read_text().rsplit(")", 1)[1].split()
        assert root.stat().st_uid == os.getuid()
        rows.append(row)
        return root, row
    try:
        root, row = process(pid)
        tty = os.stat(f'/dev/pts/{source["ptyNumber"]}').st_rdev
        if int(row[2]) != pid or int(row[4]) != tty or int(row[5]) != pid:
            return None
        expected = "/bin/bash" if shape == "shell" else "/usr/bin/htop"
        if not (root / "exe").samefile(expected):
            return None
        directory = beta if shape == "child-tui" else alpha
        if (root / "cwd").resolve(strict=True) != directory:
            return None
        if shape == "child-tui":
            parent, shell = process(int(row[1]))
            if (not (parent / "exe").samefile("/bin/bash") or int(shell[1]) != server
                    or (parent / "cwd").resolve(strict=True) != alpha
                    or int(shell[2]) != int(row[1]) or int(shell[4]) != tty
                    or int(shell[5]) != pid or row[3] != shell[3]):
                return None
        elif int(row[1]) != server:
            return None
        if shape == "shell":
            children = (root / "task" / str(pid) / "children").read_text().split()
            if len(children) != 1:
                return None
            child, member = process(int(children[0]))
            if (not (child / "exe").samefile("/usr/bin/sleep") or int(member[2]) != pid
                    or (child / "cwd").resolve(strict=True) != alpha):
                return None
        # Fixture readiness, not admission or a warmup delay. Rust still mints
        # its own invocation and attests every live foreground-group member.
        ticks = os.sysconf("SC_CLK_TCK")
        if not all((int(item[19]) + 1) / ticks < time.clock_gettime(time.CLOCK_BOOTTIME) for item in rows):
            return None
        return {"shape": shape, "foregroundPid": pid, "expectedGroupMembers": 2 if shape == "shell" else 1,
                "parentShellHasDifferentCwd": shape == "child-tui"}
    except (FileNotFoundError, ProcessLookupError):
        return None


def run(shape, beta):
    beta = destination(beta, os.environ["XDG_RUNTIME_DIR"], Path.cwd())
    config = Path(os.environ["XDG_CONFIG_HOME"])
    metadata = config.lstat()
    assert config.parent == beta.parent and config.resolve(strict=True) == config
    assert stat.S_ISDIR(metadata.st_mode) and metadata.st_uid == os.getuid() and metadata.st_mode & 0o077 == 0
    args, environment = recipe(shape, beta, config)
    (config / "fixture-htoprc").touch(mode=0o600, exist_ok=False)
    # alarm survives exec; on the child-TUI path Bash remains the session
    # leader, so its bounded exit also hangs up its foreground terminal group.
    signal.alarm(45)
    os.execve(args[0], args, environment)


if __name__ == "__main__":
    run(sys.argv[1], Path(sys.argv[2]))
