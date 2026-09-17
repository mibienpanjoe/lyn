# Lyn Context Provider for VS Code and Cursor

This local extension lets Lyn associate a capture invocation with the exact
focused VS Code or Cursor window, or with the integrated terminal that actually
has focus.

The extension sends only an ephemeral per-window identifier, focus state, a
`surface` of `editor` or `terminal`, and one local directory hint over Lyn's
user-only Unix socket. Heartbeats are protocol v1 window observations.
`Lyn: Capture from this window` (`lyn.capture`, `Ctrl+Alt+Shift+L`) sends a
v2 invoke frame with a request generation. Editor invocations use the workspace
folder list and ignore the last-used `activeTerminal`. Terminal invocations
send that focused terminal's live `shellIntegration.cwd` when it is a local
file path; they do not send the initial `creationOptions.cwd`, names, titles,
commands, or output. Remote and multi-root workspaces are not selected
automatically. The local shortcut is distinct from Lyn's global
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
