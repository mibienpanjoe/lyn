# Lyn Context Provider for Kitty

The Kitty provider uses Kitty's [global watcher callbacks](https://sw.kovidgoyal.net/kitty/launch/#watching-launched-windows) to report the exact focused pane to Lyn's private terminal socket. It sends only the Kitty pane ID, child process ID, focus/liveness state, and protocol version. Rust validates the active Kitty X11 window, verifies that the process belongs to the current runtime user, and derives cwd from `/proc`; the watcher never reads or sends terminal text, commands, output, titles, environment values, or filesystem paths.

Kitty remote control is not required and should not be enabled for Lyn.

Test the watcher from the repository root:

```sh
pnpm provider:terminal:test
```

For a development checkout, add this absolute path to `kitty.conf`:

```text
watcher /home/mj/projects/lyn/integrations/kitty/lyn_context_watcher.py
```

Restart Kitty after adding the watcher. Kitty applies watcher configuration to newly created windows. Keep Lyn running while testing so the private runtime socket exists.

The release installer location remains gated by packaging work; do not copy this development path into release documentation.
