"""Keep display, D-Bus, user directories and runtime completely test-owned."""

import os
from pathlib import Path
import subprocess
import sys
import tempfile


with tempfile.TemporaryDirectory(prefix="lyn-gnome-run-") as directory:
    root = Path(directory)
    environment = dict(os.environ, GSETTINGS_BACKEND="memory", GIO_USE_VFS="local",
                       GTK_USE_PORTAL="0", NO_AT_BRIDGE="1", GTK_A11Y="none")
    for variable, subdirectory in (("XDG_RUNTIME_DIR", "runtime"),
                                   ("XDG_CONFIG_HOME", "config"),
                                   ("XDG_DATA_HOME", "data"),
                                   ("XDG_CACHE_HOME", "cache")):
        location = root / subdirectory
        location.mkdir(mode=0o700)
        environment[variable] = str(location)
    if sys.argv[1] != "-":
        sdk = Path(sys.argv[1]).resolve(strict=True)
        environment["PATH"] = str(sdk / "root/usr/bin") + os.pathsep + environment["PATH"]
    # No service directories: never auto-start portals, keyrings or a second
    # terminal server. Only the fixture processes are allowed to exist here.
    bus_config = root / "session.conf"
    bus_config.write_text(f"""<busconfig>
      <type>session</type>
      <listen>unix:tmpdir={root / 'runtime'}</listen>
      <auth>EXTERNAL</auth>
      <policy context="default">
        <allow user="*"/>
        <allow own="*"/>
        <allow send_destination="*"/>
        <allow receive_sender="*"/>
      </policy>
    </busconfig>""")
    result = subprocess.run([
        "xvfb-run", "-a", "-s",
        "-screen 0 1280x800x24 -nolisten tcp", "dbus-run-session",
        "--config-file", str(bus_config), "--",
        sys.executable, *sys.argv[2:]], env=environment)
raise SystemExit(result.returncode)
