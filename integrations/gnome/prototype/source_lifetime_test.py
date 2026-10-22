"""Allow natural VTE cache expiry, only on the guarded private display."""
from pathlib import Path
import os
import subprocess
import sys

from desktop_isolation import require_owned_display
from sanitizer_environment import sanitizer_environment


def lifetime_environment(original):
    environment = sanitizer_environment(original)
    environment.update(GTK_MODULES="", GTK3_MODULES="")
    environment.pop("LD_PRELOAD", None)
    environment.pop("LD_AUDIT", None)
    return environment


def run(executable, sdk):
    require_owned_display(sdk)
    result = subprocess.run([str(executable.resolve(strict=True))],
                            env=lifetime_environment(os.environ), check=False, timeout=45)
    return result.returncode if result.returncode >= 0 else 128 - result.returncode


if __name__ == "__main__":
    raise SystemExit(run(Path(sys.argv[1]), None if sys.argv[2] == "-" else Path(sys.argv[2]).resolve(strict=True)))
