# Source this file from a local Bash or Zsh startup file after installing the
# `lyn-context` helper in PATH, or set LYN_CONTEXT_HELPER to its absolute path.

_lyn_start_context_provider() {
  # Kitty uses its exact-pane watcher; running the generic observer as well
  # would create duplicate candidates for the same terminal pane.
  if [ -n "${KITTY_WINDOW_ID:-}" ]; then
    return
  fi

  if [ -n "${LYN_CONTEXT_WATCHER_PID:-}" ] && kill -0 "$LYN_CONTEXT_WATCHER_PID" 2>/dev/null; then
    return
  fi

  _lyn_context_helper=${LYN_CONTEXT_HELPER:-lyn-context}
  if ! command -v "$_lyn_context_helper" >/dev/null 2>&1; then
    return
  fi

  "$_lyn_context_helper" watch --process "$$" >/dev/null 2>&1 &
  LYN_CONTEXT_WATCHER_PID=$!
  export LYN_CONTEXT_WATCHER_PID
  unset _lyn_context_helper
}

_lyn_start_context_provider
unset -f _lyn_start_context_provider
