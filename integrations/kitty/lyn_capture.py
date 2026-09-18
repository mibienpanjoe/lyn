"""Kitty kitten: invoke Lyn from the exact focused pane, including under a TUI.

Mapped as `kitten lyn_capture.py`. It reports only pane and child-process
identity; Lyn assigns the OS window and derives cwd in Rust. No overlay UI.
"""

from __future__ import annotations

from typing import Any

from lyn_context_watcher import (
    _window_identity,
    create_invoke_request,
    send_message,
)


def main(_args: list[str]) -> None:
    return None


def handle_result(
    _args: list[str], _answer: str, target_window_id: int, boss: Any
) -> None:
    window = None
    window_id_map = getattr(boss, "window_id_map", None)
    if window_id_map is not None:
        window = window_id_map.get(target_window_id)
    if window is None:
        window = getattr(boss, "active_window", None)
    identity = _window_identity(window)
    if identity is None:
        return
    terminal_session_id, process_id = identity
    send_message(create_invoke_request(terminal_session_id, process_id))


handle_result.no_ui = True
