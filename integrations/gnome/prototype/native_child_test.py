"""One synthetic GTK child launched by the private terminal's new-tab action."""
import json
import os
from pathlib import Path
import socket
import sys

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk

Gtk.init([])
module, preserved, endpoint = sys.argv[1:]
maps = Path("/proc/self/maps").read_text()
with socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM) as channel:
    channel.sendto(json.dumps({
        "childModuleAbsent": str(Path(module).resolve()) not in maps,
        "existingModulePreserved": str(Path(preserved).resolve()) in maps,
        "moduleEnvironmentAbsent": module not in os.environ.get("GTK3_MODULES", "").split(":"),
    }).encode(), endpoint)
