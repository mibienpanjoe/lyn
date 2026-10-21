"""Run the GTK traversal fixture only on the private guarded display."""

from pathlib import Path
import subprocess
import sys

from desktop_isolation import require_owned_display


require_owned_display(None if sys.argv[2] == "-" else Path(sys.argv[2]).resolve(strict=True))
subprocess.run([str(Path(sys.argv[1]).resolve(strict=True))], check=True, timeout=10)
