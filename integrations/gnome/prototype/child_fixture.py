"""Check module containment in a GTK child, then exec the synthetic TUI."""

import json
import os
from pathlib import Path
import socket
import sys

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk

Gtk.init([])
module = Path(sys.argv[1]).resolve()
maps = Path("/proc/self/maps").read_text()
mapped = str(module) in maps
preserved = str(Path(sys.argv[3]).resolve()) in maps
with socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM) as channel:
    channel.sendto(json.dumps({"childModuleAbsent": not mapped,
                               "existingModulePreserved": preserved}).encode(), sys.argv[2])
os.execl("/usr/bin/htop", "htop")
