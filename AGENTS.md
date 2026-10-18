# Repository Guidelines

## Project Structure & Module Organization

Lyn has a Svelte/Tauri implementation; `README.md` is the entry point. Read `docs/project_overview.md`, then `01_requirements_prd.md` through `09_local_speech_distribution_decision.md`. Preserve requirement, invariant, and error IDs across documents.

Svelte 5/TypeScript UI lives under `src/`; the Tauri/Rust core lives under `src-tauri/`. Window IPC is gated by `src-tauri/capabilities/` (`main` Library vs `capture` popup) referencing permission sets in `src-tauri/permissions/` plus `build.rs` `AppManifest::commands` allowlists—never `core:default` path/image escape hatches. Separately delivered local context integrations live under `integrations/`: VS Code reports v1 focus heartbeats plus versioned v2 invoke frames (workspace folders as correlation hints; `surface` editor vs focused terminal; window identity is assigned by Rust), the Kitty watcher reports exact pane focus plus child process identity and a no-UI kitten invokes capture from that pane, and the generic shell bootstrap starts the bounded `lyn-context` helper (Lyn refreshes that helper next to the app on launch so users do not build a second binary). Native Messaging uses the same running binary via `~/.local/share/lyn/bin/lyn-browser-host`; the companion reports v1 tab heartbeats plus a v2 invoke from the focused tab (`lyn.capture`). Editor, Cursor, and Kitty invoke with `Ctrl+Alt+Shift+L`. The browser companion suggests `Alt+Shift+L` because Chromium rejects `Ctrl+Alt+*` suggested keys; Firefox MV3 loads `background.scripts` (not a service worker). The Settings global shortcut is a distinct grab (default `Control+Shift+Space`) and cannot reuse the integration combination. All provider traffic uses private Rust-owned sockets and excludes commands, output, environment values, titles, and client-supplied paths. Linux editor and browser sockets also check the connecting peer uid, bound the read, and ignore a replayed v2 invoke generation. Localhost mapping requires one same-user process cwd after skipping `docker-proxy`; IPv4/IPv6 sockets of the same directory are one source, and multiple distinct cwds stay unresolved. 1-click integration status checks and installations are managed under `src-tauri/src/integrations/` and exposed via `commands::integrations` to the Library window. Ordered SQLite migrations live under `src-tauri/migrations/` and must remain immutable after release. Keep presentation and IPC clients in `src/`; storage, capture-session state, context detection, media, and OS integration belong in Rust. Capture-session transitions live in `src-tauri/src/capture/session.rs`; provider observations and their ephemeral registry live under `src-tauri/src/context/`; command gateways live under `src-tauri/src/commands/`, parameterized context/capture persistence stays under `src-tauri/src/storage/`, and platform-specific shortcut/focus/audio/popup-layout adapters stay behind `src-tauri/src/platform/` ports. Recoverable media staging is in `src-tauri/src/media/staging.rs`; PNG/WAV normalization stays under `src-tauri/src/media/`, and startup reconciliation reads durable media references through `src-tauri/src/storage/media_assets.rs`. Staged previews resolve through the read-only `lyn-media://` protocol by opaque ID; the capture/library CSP allows that scheme (and the Windows `lyn-media.localhost` form) on `img-src`/`media-src` only. None of these boundaries may expose raw filesystem paths to the UI.

On Linux, GNOME Terminal may inherit `KITTY_WINDOW_ID` from its launcher. Its own `GNOME_TERMINAL_SCREEN` marker keeps the shell helper enabled, while Rust rejects generic shell observations for actual Kitty windows. A new shell helper binds only to the actual foreground window, never Lyn's remembered window. Once bound, it keeps its process/window heartbeat alive in the background; background reports must not replace the remembered foreground window. Multiple sessions in one terminal window still require explicit selection. Editor integration status reports the current embedded extension version as installed, but file presence alone does not prove a live connection or successful capture.

The developer-only native GNOME prototype is under `integrations/gnome/prototype/`. It uses GTK3/VTE to relate terminal widgets to native windows and PTYs, without terminal stream access. Its version-0 diagnostic datagrams are not accepted by the production brokers; its v1 stream queries use only a private fixture broker. The module is neither packaged nor installed by Lyn. Its harness uses Xvfb, a private D-Bus and isolated XDG directories; never point its actions at the user's terminal server. On Pop!_OS use `gnome-terminal.real` to address the already-created fixture server instead of the per-launch wrapper. Keep ordinary GNOME support limitations until the native Rust broker and installed-package acceptance are delivered.

The native GNOME v1 contract is owned by `docs/gnome_native_protocol.md` and `context/gnome_protocol.rs`, with its C codec in `integrations/gnome/wire.c`. Its fixed 112-byte codecs have shared valid/refused wire vectors; those and single-use invocation validation are not a running integration. The future dedicated broker must attest peer/server/window/PTY/process identities before promoting decoded metadata to a source; ordinary editor/browser payloads still cannot supply native IDs. Reject topology changes at or after invocation, including equal microsecond timestamps and A→B→A; opening Lyn and losing OS focus alone does not invalidate an unchanged tab. No PTY FD or stream access is permitted. Keep the 120 ms reply refusal bound distinct from measured input/context latency targets.

Native peer admission lives in `context/gnome_peer.rs`: kernel credentials, allowlisted root-owned executable inode, proc identity and a polled pidfd. Do not replace unavailable pidfd support/permissions with a numeric-PID heuristic. Its tests require Linux pidfd support and Python3 for an isolated connecting-process fixture; restricted sandboxes may require running these tests outside the sandbox. Peer admission alone does not attest the window/PTY or register a context source.

`context/gnome_channel.rs` owns the native stream handshake and one-at-a-time exchange. Deadlines are absolute Linux monotonic times, not per-read timeouts; reject future invocation timestamps and never reset the 120 ms budget on partial input. Transport/correlation errors close/drop the socket; only a correlated explicit unresolved reply may keep it for a fresh request. Never receive PTY FDs: the metadata channel rejects ancillary data. Channel tests use real socket pairs with test-only fixture-peer authentication; they do not establish a running broker or GNOME kernel/window/PTY attestation.

`context/gnome_listener.rs` owns only the private native endpoint and cancellable idle accept. Never unlink occupied endpoints; shutdown may remove only the recorded socket inode. Preserve private directory/socket permissions, final-component symlink refusal and the explicit pending-connection backlog. Accepted streams are not authenticated sources: they must pass the native peer/channel boundary and later window/PTY attestation.

`context/gnome_broker.rs` composes admission and exact-server routing on one worker, with at most eight admitted server PIDs and one pending handshake. No registry/session mutex may span channel I/O; a busy channel or reconnect refuses rather than queues. Reconnection invalidates the previous generation. Evidence is still untrusted until window/PTY/process attestation, and the manager is not yet wired into application startup or capture. Broker tests authenticate only the current test executable under cfg(test); production always uses the system-server peer policy.

## Build, Test, and Development Commands

Use Node 24.12.0, pnpm 10.28.0, Rust 1.96.1, and the documented Tauri system prerequisites. Canonical commands are:

- `pnpm install` — install the pinned frontend/tooling graph.
- `pnpm bindings` — regenerate the tracked TypeScript IPC contract from Rust.
- `pnpm dev` — run the frontend-only Vite server.
- `pnpm format:check` — check frontend/config formatting.
- `pnpm check` — run Svelte and TypeScript checks.
- `pnpm test` — run frontend component/accessibility tests once.
- `pnpm provider:vscode:test` — run the VS Code provider's dependency-free Node contract tests.
- `pnpm provider:vscode:package` — create `/tmp/lyn-context-provider.vsix` for local installation.
- `pnpm provider:cursor:install` — install `/tmp/lyn-context-provider.vsix` into local Cursor.
- `pnpm provider:browser:test` — run browser companion extension contract tests.
- `pnpm provider:browser:build` — build the standalone `lyn-browser-host` binary for isolated helper debugging (not required for users; Lyn refreshes `~/.local/share/lyn/bin/lyn-browser-host` from its own binary).
- `pnpm provider:browser:install` — install the Native Messaging manifest for Chrome, Chromium, Brave, Edge, and Firefox.
- `pnpm provider:terminal:test` — run Rust broker, shell bootstrap, and Kitty watcher contract tests.
- `pnpm provider:terminal:build` — build the standalone `lyn-context` binary for isolated helper debugging (not required for users; Lyn's own binary handles `watch --process`).
- `pnpm provider:gnome:test` — compile and run the developer-only native GTK3/VTE prototype on private Xvfb/D-Bus; requires GTK3/VTE development headers, GCC/pkg-config, Xvfb/xauth, GNOME Terminal, xdotool/xprop, Python GI GTK3, and htop. No module is installed. `LYN_GNOME_SDK_ROOT` may point to a local SDK directory containing extracted packages under `root/` instead of installing VTE headers/Xvfb system-wide.
- `pnpm icons` — regenerate platform icons from the tracked SVG master.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml` — check Rust formatting.
- `cargo test --manifest-path src-tauri/Cargo.toml` — run Rust tests.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib perf:: -- --nocapture` — measure provisional latency budgets (text save, 10k search, media reconcile).
- `pnpm build` — build frontend assets.
- `pnpm package:linux` — build unsigned Linux `.deb` plus portable `.tar.gz` (host must be Linux).
- `pnpm package:windows` — build unsigned Windows NSIS `.exe` (host must be Windows).
- Prefer CI: push a `v*` tag or run `.github/workflows/release.yml` so GitHub Actions builds Linux and Windows artifacts onto a prerelease.
- `pnpm tauri dev` — open the desktop development shell.
- `pnpm tauri build --no-bundle` — compile the production binary without packaging.

For documentation changes, also run:

- `git diff --check` — detect whitespace errors.
- `rg -n "FR-|INV-|ERR-" docs/` — inspect cross-document identifiers.

Packaging targets are `.deb`, Linux binary tarball, and Windows NSIS `.exe`. Artifacts are unsigned for v1 and distributed via GitHub Releases only (no in-app updater). macOS and Wayland remain deferred. Add new exact commands here and to `README.md` in the same change.

## Coding Style & Naming Conventions

Write concise Markdown with descriptive headings and relative links. Keep numbered filenames stable. Never present proposed designs as implemented or verified.

Use `rustfmt` and Prettier. Use `snake_case` for Rust modules/functions, `PascalCase` for Rust types and Svelte components, and kebab-case for TypeScript utility files. Import individual outline icons from `@lucide/svelte/icons/*`; keep visible labels on capture actions and match icon stroke to adjacent text. Preserve typed Tauri IPC; the frontend must not access SQLite or arbitrary filesystem paths.

## Testing Guidelines

The editor extension 0.1.3 contributes a `terminal.integrated.commandsToSkipShell` default containing `lyn.capture`; it never writes user settings. Explicit user/workspace routing overrides remain authoritative. Validate terminal invocation without a manually added skip-shell entry, including a terminal cwd different from the editor workspace.

The editor broker uses blocking Unix `accept` rather than polling idle connections. Accepted messages retain their uid/size/read-deadline checks; only accept errors back off. Measure shortcut-to-input and shortcut-to-context separately on the current packaged binary before claiming latency budgets.

The X11 editor classifier accepts both legacy `Code` and the installed VS Code namespaced `com.microsoft.VSCode` WM_CLASS, by exact case-insensitive match. Do not broaden this to substring matching; confirm the actual class when validating a new editor package.

Linux text, image, and audio saves share native shell source revalidation against the registered process uid/inode and current project directory, not only the heartbeat TTL. A closed or changed source must return `CONTEXT_SOURCE_STALE` without modifying the active draft or finalizing staged media. Tests cover each capture kind; packaged runtime acceptance is a separate check.

Every behavior change must include tests at its owning boundary. Prioritize Rust domain tests, IPC serialization/negative tests, media recovery tests, and keyboard/accessibility UI tests. Follow the verification matrices in documents 05–07. Record commands once a runner exists.

## Commit & Pull Request Guidelines

History is small; prefer concise Conventional Commit-style messages such as `docs: clarify capture invariants`, `feat: add text capture`, or `test: cover duplicate saves`. Keep commits atomic.

Pull requests should explain scope/rationale, link requirements or issues, list verification, and call out deferred work. Include screenshots or recordings for UI changes and identify contract, invariant, dependency, permission, or data-model changes.

## Security & Product Boundaries

Lyn is local-first. Core capture must not depend on accounts, cloud APIs, or remote AI. Treat clipboard data, paths, window titles, and media as untrusted. Never log capture content or commit secrets. Project directories must enter through the Rust-owned native picker; expose only expiring one-use tokens and safe labels to the frontend, never raw paths or generic filesystem permissions. Bind automatic context to the pre-popup foreground window; never use a global last-reported session or expose terminal, editor, or agent content. Context correction must preserve the draft. Preserve “save first, enrich afterward” and user-authored metadata precedence.

The Rust capture-session service owns the single active session and save idempotency. Commands must not derive session state from popup mount/unmount events or perform persistence outside the service's serialized `save_once` boundary. Text blankness checks must not rewrite the body; durable text and its FTS projection are accepted only after the SQLite transaction commits.

Update this guide in the same change whenever repository state, commands, structure, contracts, or contributor workflows change. Breaking API, invariant, data-model, security, or platform changes require matching guidance updates.
