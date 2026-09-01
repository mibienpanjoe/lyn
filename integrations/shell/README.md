# Lyn Context Provider for Shells and Terminals

The bounded `lyn-context` helper observes one local Bash or Zsh process. It sends only an opaque session ID, process ID, verified X11 window correlation, liveness, and protocol version to Lyn's user-only runtime socket. Rust derives cwd from the same-user process; commands, output, environment values, and client-supplied paths never enter the provider message.

Build and test it from the repository root:

```sh
pnpm provider:terminal:build
pnpm provider:terminal:test
```

For this development checkout, add the following to `~/.bashrc` or `~/.zshrc`:

```sh
export LYN_CONTEXT_HELPER=/home/mj/projects/lyn/src-tauri/target/debug/lyn-context
source /home/mj/projects/lyn/integrations/shell/lyn-context.sh
```

Start a new terminal session after building the helper. The bootstrap deliberately skips Kitty because [`../kitty/lyn_context_watcher.py`](../kitty/lyn_context_watcher.py) provides stronger exact-pane evidence there.

One GNOME Terminal window or one VS Code integrated-terminal session can resolve automatically. When multiple generic terminal tabs or integrated terminals share one OS window, Lyn returns ambiguity instead of guessing which session is active.
