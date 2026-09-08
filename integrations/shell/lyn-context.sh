# Source this file from a local Bash or Zsh startup file. Lyn keeps the helper
# at ~/.local/share/lyn/bin/lyn-context; no extra build command is required.

_lyn_resolve_context_helper() {
  if [ -n "${LYN_CONTEXT_HELPER:-}" ]; then
    printf '%s\n' "$LYN_CONTEXT_HELPER"
    return 0
  fi

  _lyn_data_helper="${XDG_DATA_HOME:-$HOME/.local/share}/lyn/bin/lyn-context"
  if [ -x "$_lyn_data_helper" ]; then
    printf '%s\n' "$_lyn_data_helper"
    unset _lyn_data_helper
    return 0
  fi
  unset _lyn_data_helper

  if command -v lyn-context >/dev/null 2>&1; then
    command -v lyn-context
    return 0
  fi
  if command -v lyn >/dev/null 2>&1; then
    command -v lyn
    return 0
  fi
  return 1
}

_lyn_start_context_provider() {
  # Kitty uses its exact-pane watcher; running the generic observer as well
  # would create duplicate candidates for the same terminal pane.
  if [ -n "${KITTY_WINDOW_ID:-}" ]; then
    return
  fi

  if [ -n "${LYN_CONTEXT_WATCHER_PID:-}" ] && kill -0 "$LYN_CONTEXT_WATCHER_PID" 2>/dev/null; then
    return
  fi

  _lyn_context_helper=$(_lyn_resolve_context_helper) || return
  if [ ! -x "$_lyn_context_helper" ]; then
    unset _lyn_context_helper
    return
  fi

  "$_lyn_context_helper" watch --process "$$" >/dev/null 2>&1 &
  LYN_CONTEXT_WATCHER_PID=$!
  export LYN_CONTEXT_WATCHER_PID
  unset _lyn_context_helper
}

_lyn_start_context_provider
unset -f _lyn_start_context_provider
unset -f _lyn_resolve_context_helper
