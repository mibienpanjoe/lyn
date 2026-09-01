# Lyn

Lyn is a fast, lightweight, local-first desktop working-memory companion for developers and focused computer work.

`local-first` · `desktop` · `developer-tools` · `tauri` · `rust` · `svelte` · `sqlite`

```text
global shortcut → type, paste, or record → Enter → return to work
```

Lyn captures titleless text notes, screenshots, and voice notes, associates them with project and Git-branch context, and keeps them accessible through a chronological Library and local search.

## Status

Implementation has started. Phase 0 provides a verified Svelte 5/Vite shell, a minimal Tauri 2/Rust desktop shell, narrow window capabilities, and frontend/Rust smoke-test harnesses. Phase 1 now includes Rust-owned shared IPC primitives, startup initialization of the canonical SQLite/FTS schema through ordered migrations, validated manual contexts, a single-active-session state machine, durable titleless text capture with FTS projection, and the quick-capture popup. Project directories enter only through a native picker and an expiring one-use token; raw paths remain inside Rust. Phase 2 now includes invocation-bound X11 foreground capture, Git worktree identity, the provider feasibility boundary, an ephemeral live-source registry, deterministic evidence ranking, safe source-list IPC, selection/save-time revalidation, an accessible focused source chooser, and delivered local VS Code, Kitty, and bounded shell/terminal providers for Linux X11. Checkpoint B is verified through the owner-confirmed VS Code and Kitty desktop flow plus automated ambiguity, staleness, privacy-boundary, and draft-preservation coverage. Phase 3 implements recoverable Lyn-owned media staging plus screenshot and WAV voice capture, opaque preview/playback, optional exact captions, cancellation, and durable save. Library behavior remains proposed.

The first implementation baseline is Pop!_OS 22.04 LTS (Ubuntu-compatible, x86_64, X11), Node 24.12.0 with pnpm 10.28.0, and Rust 1.96.1. macOS, Windows, Wayland, and packaging support remain unverified and are not yet claimed.

## Development

Install the [Tauri 2 Linux prerequisites](https://v2.tauri.app/start/prerequisites/), then run:

```bash
pnpm install
pnpm bindings
pnpm dev
pnpm format:check
pnpm check
pnpm test
pnpm provider:vscode:test
pnpm provider:vscode:package
pnpm provider:terminal:test
pnpm provider:terminal:build
pnpm icons
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
pnpm build
pnpm tauri dev
pnpm tauri build --no-bundle
```

`pnpm bindings` regenerates the tracked TypeScript IPC contract from Rust. `pnpm dev` runs only the frontend. `pnpm icons` regenerates platform icons from the tracked SVG master. `pnpm tauri dev` opens the desktop shell and initializes `lyn.db` in Tauri's application-data directory. Application packaging remains disabled until the distribution gate is resolved.

## Context providers on Linux X11

Context detection is optional enrichment: Lyn still captures when a provider is unavailable. Start Lyn first so its private user-only sockets exist:

```bash
pnpm tauri dev
```

The default development shortcut is `Ctrl+Shift+Space`. Focus the editor window or terminal pane that owns the work, then invoke Lyn with the shortcut. Automatic selection is bound to that exact pre-popup window. Focusing another provider after Lyn is already open does not silently replace the capture context; use the context chooser when you intentionally want another live session or saved context.

### VS Code

The VS Code provider is a separate local extension. Test, package, and install it with:

```bash
pnpm provider:vscode:test
pnpm provider:vscode:package
code --install-extension /tmp/lyn-context-provider.vsix --force
```

Reload existing VS Code windows after installation. While Lyn is running, the extension reconnects to its user-only runtime socket automatically. It reports only per-window focus state and local workspace folders; remote and multi-root workspaces remain manual rather than guessed.

### Kitty

Add the watcher to `~/.config/kitty/kitty.conf`, using the absolute path to this checkout:

```text
watcher /absolute/path/to/lyn/integrations/kitty/lyn_context_watcher.py
```

Fully restart Kitty after adding or updating the watcher; config reloads affect only newly created Kitty windows. Keep Lyn running, allow up to two seconds for the first heartbeat, focus the intended pane, and invoke Lyn with `Ctrl+Shift+Space`. The watcher retains the originating pane while Lyn owns focus, replaces it when another Kitty pane takes focus, and revokes it when the pane closes. Kitty remote control is neither required nor enabled.

### Live and saved contexts

- **Live sessions** are ephemeral provider observations. A row marked **Current window** is associated with the window that invoked the current capture.
- **Saved contexts** are durable manual choices. They remain available without a provider.
- Lyn automatically selects only exact invocation-bound evidence. Being the only or most recently reported live session is intentionally insufficient.
- Unsupported multi-session cases remain ambiguous or require manual selection. The generic helper supports one distinguishable GNOME Terminal or VS Code integrated-terminal session per OS window; it does not guess between several sessions sharing that window.
- Changing context preserves the current text, screenshot, recording, and capture session.

If a provider does not appear, confirm Lyn was running before the provider heartbeat, reload the VS Code window or fully restart Kitty, wait two seconds, and reopen Lyn from the source window using the shortcut. If a live source appears but is not marked **Current window**, dismiss the capture with `Esc`, focus the intended source, and invoke Lyn again instead of clicking the existing Lyn window.

Detailed provider boundaries and tests are documented under [`integrations/vscode/`](integrations/vscode/README.md), [`integrations/kitty/`](integrations/kitty/README.md), and [`integrations/shell/`](integrations/shell/README.md).

## Documentation

- [Project overview](docs/project_overview.md)
- [Product requirements](docs/01_requirements_prd.md)
- [Software requirements](docs/02_requirements_srs.md)
- [System architecture](docs/05_architecture.md)
- [Typed Tauri IPC specification](docs/06_api_specification.md)
- [Visual identity](docs/07_visual_identity.md)
- [Context provider feasibility](docs/08_context_provider_feasibility.md)

The implemented shell uses Tauri 2, Rust, Svelte 5, TypeScript, Vite, tree-shaken `@lucide/svelte` action icons, and bundled SQLite through `rusqlite`. Rust owns the shared domain and IPC primitives, generated TypeScript bindings, database initialization, canonical schema, transactional migrations, context and text/media capture repositories, native project-directory selection, capture-session lifecycle, recoverable Lyn-owned media staging, microphone input, audio playback, and bounded native popup sizing. Screenshot paste and voice recording expose only opaque media IDs to the popup; both support optional manual captions, replacement, removal back to the preserved text draft, cancellation, retry, and durable save. The popup switches between compact, completed-audio, inline-error, context-chooser, and screenshot-preview layouts so each state stays dense without accepting arbitrary frontend dimensions.
