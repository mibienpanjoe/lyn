# Lyn Context Provider for Kitty

The Kitty provider uses Kitty's [global watcher callbacks](https://sw.kovidgoyal.net/kitty/launch/#watching-launched-windows) to report the exact focused pane, and a no-UI kitten to invoke Lyn from that pane (including under a TUI). The watcher and kitten send only the Kitty pane ID, child process ID, focus/liveness or invoke generation, and protocol version. Rust validates the active Kitty X11 window, verifies that the process belongs to the current runtime user, and derives cwd from `/proc`. Neither script reads or sends terminal text, commands, output, titles, environment values, or filesystem paths. The generic `lyn-context` shell helper does not start inside Kitty (`KITTY_WINDOW_ID`); Rust also ignores a generic shell observation for a window that already has an exact Kitty pane.

Kitty remote control is not required and should not be enabled for Lyn.

Test the watcher and kitten from the repository root:

```sh
pnpm provider:terminal:test
```

For a development checkout, add these absolute paths to `kitty.conf`:

```text
watcher /home/mj/projects/lyn/integrations/kitty/lyn_context_watcher.py
map ctrl+alt+shift+l kitten /home/mj/projects/lyn/integrations/kitty/lyn_capture.py
```

Restart Kitty after adding the watcher and kitten. Kitty applies watcher configuration to newly created windows. Keep Lyn running while testing so the private runtime socket exists.

The release installer location remains gated by packaging work; do not copy this development path into release documentation.
