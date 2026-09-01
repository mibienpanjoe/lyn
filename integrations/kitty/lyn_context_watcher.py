"""Privacy-bounded Kitty focus provider for Lyn.

Kitty loads this module as a global watcher. It reports only the focused Kitty
pane ID and its child process ID; Lyn derives and validates the working
directory inside its Rust-owned boundary.
"""

from __future__ import annotations

import json
import os
import socket
import stat
import threading
import time
from typing import Any


SOCKET_NAME = "lyn-shell-v1.sock"
HEARTBEAT_SECONDS = 2
SOCKET_TIMEOUT_SECONDS = 0.75
MAX_MESSAGE_BYTES = 4 * 1024

_lock = threading.Lock()
_focused: dict[int, int] = {}
_heartbeat_started = False


def create_message(
    terminal_session_id: int, process_id: int, state: str
) -> dict[str, int | str]:
    return {
        "version": 1,
        "terminalSessionId": int(terminal_session_id),
        "processId": int(process_id),
        "state": state,
    }


def _socket_path() -> str | None:
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    if not runtime:
        return None
    try:
        metadata = os.stat(runtime, follow_symlinks=False)
    except OSError:
        return None
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) & 0o077
    ):
        return None
    return os.path.join(runtime, SOCKET_NAME)


def send_message(message: dict[str, int | str]) -> None:
    path = _socket_path()
    if not path:
        return
    payload = json.dumps(message, separators=(",", ":")).encode("utf-8")
    if len(payload) > MAX_MESSAGE_BYTES:
        return
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
            client.settimeout(SOCKET_TIMEOUT_SECONDS)
            client.connect(path)
            client.sendall(payload)
    except OSError:
        # Provider availability never controls Lyn capture availability.
        return


def _window_identity(window: Any) -> tuple[int, int] | None:
    try:
        terminal_session_id = int(window.id)
        process_id = int(window.child.pid)
    except (AttributeError, TypeError, ValueError):
        return None
    if terminal_session_id <= 0 or process_id <= 0:
        return None
    return terminal_session_id, process_id


def on_focus_change(_boss: Any, window: Any, data: dict[str, Any]) -> None:
    identity = _window_identity(window)
    if identity is None:
        return
    terminal_session_id, process_id = identity
    focused = data.get("focused") is True
    if not focused:
        # Opening Lyn necessarily moves OS focus away from Kitty. Keep the
        # invocation-bound pane alive until another Kitty pane takes focus,
        # the pane closes, or Rust expires the bounded observation.
        return
    with _lock:
        previous = tuple(
            (session_id, child_process_id)
            for session_id, child_process_id in _focused.items()
            if session_id != terminal_session_id
        )
        _focused.clear()
        _focused[terminal_session_id] = process_id
    for previous_session_id, previous_process_id in previous:
        send_message(
            create_message(previous_session_id, previous_process_id, "ended")
        )
    send_message(create_message(terminal_session_id, process_id, "focused"))


def on_close(_boss: Any, window: Any, _data: dict[str, Any]) -> None:
    identity = _window_identity(window)
    if identity is None:
        return
    terminal_session_id, process_id = identity
    with _lock:
        was_focused = _focused.pop(terminal_session_id, None) is not None
    if was_focused:
        send_message(create_message(terminal_session_id, process_id, "ended"))


def _heartbeat() -> None:
    while True:
        time.sleep(HEARTBEAT_SECONDS)
        with _lock:
            focused = tuple(_focused.items())
        for terminal_session_id, process_id in focused:
            send_message(create_message(terminal_session_id, process_id, "focused"))


def on_load(boss: Any, _data: dict[str, Any]) -> None:
    global _heartbeat_started
    with _lock:
        if _heartbeat_started:
            return
        _heartbeat_started = True
    active_window = getattr(boss, "active_window", None)
    if active_window is not None:
        on_focus_change(boss, active_window, {"focused": True})
    threading.Thread(
        target=_heartbeat,
        name="lyn-kitty-context",
        daemon=True,
    ).start()


def reset_for_test() -> None:
    global _heartbeat_started
    with _lock:
        _focused.clear()
        _heartbeat_started = False
