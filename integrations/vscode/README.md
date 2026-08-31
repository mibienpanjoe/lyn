# Lyn Context Provider for VS Code

This local extension lets Lyn associate a capture invocation with the exact
focused VS Code window and its single local workspace folder.

The extension sends only an ephemeral per-window identifier, focus state, and
local workspace folder over Lyn's user-only Unix socket. It does not inspect or
send editor contents, terminal commands or output, window titles, clipboard
data, or agent conversations. Remote and multi-root workspaces are not selected
automatically.

Package and install the extension from the repository root:

```sh
pnpm provider:vscode:package
code --install-extension /tmp/lyn-context-provider.vsix --force
```

Reload existing VS Code windows after installation. While Lyn is running, the
extension reconnects to its local socket automatically.
