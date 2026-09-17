# Lyn Context Provider for VS Code and Cursor

This local extension lets Lyn associate a capture invocation with the exact
focused VS Code or Cursor window and its single local workspace folder.

The extension sends only an ephemeral per-window identifier, focus state, and
local workspace folder over Lyn's user-only Unix socket. Heartbeats are protocol
v1. `Lyn: Capture from this window` (`lyn.capture`, `Ctrl+Alt+Shift+L`) sends a
v2 invoke frame with a request generation. It does not inspect or send editor
contents, terminal commands or output, window titles, clipboard data, native
window identifiers, or agent conversations. Remote and multi-root workspaces are
not selected automatically. The local shortcut is distinct from Lyn's global
`Control+Shift+Space`.

Package and install the extension from the repository root:

For VS Code:
```sh
pnpm provider:vscode:package
code --install-extension /tmp/lyn-context-provider.vsix --force
```

For Cursor:
```sh
pnpm provider:vscode:package
cursor --install-extension /tmp/lyn-context-provider.vsix --force
```

Reload existing VS Code or Cursor windows after installation. While Lyn is running, the
extension reconnects to its local socket automatically.
