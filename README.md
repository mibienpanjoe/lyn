# Lyn

<p align="center">
  <img src="src-tauri/icons/lyn-icon.svg" alt="" width="128" height="128" />
</p>

<p align="center">
  <strong>Lyn</strong><br />
  A fast, local-first desktop working-memory companion for developers and focused computer work.
</p>

<p align="center">
  <code>local-first</code> · <code>desktop</code> · <code>tauri</code> · <code>rust</code> · <code>svelte</code> · <code>sqlite</code>
</p>

```text
global shortcut → type, paste, or record → Enter → return to work
```

Lyn captures titleless text notes, screenshots, and voice notes, associates them with the project and Git branch you were just in, and keeps them in a chronological Library with local search. Everything stays on your machine.

## Why Lyn

Traditional note apps interrupt flow: open a notebook, name a page, pick a folder. Lyn is the opposite—an always-available capture surface that preserves the thought **with** the context it came from, then gets out of the way.

- **Local-first:** core capture never depends on accounts, cloud APIs, or remote AI.
- **Context-aware:** optional VS Code, Kitty, and shell providers bind to the pre-popup window; you can always choose manually.
- **Save first:** enrichment (optional local speech captions) never blocks a successful save.

## Download / run

**Supported baseline today:** Pop!_OS 22.04 LTS (Ubuntu-compatible, x86_64, X11), Node 24.12.0, pnpm 10.28.0, Rust 1.96.1, plus [Tauri 2 Linux prerequisites](https://v2.tauri.app/start/prerequisites/).

macOS, Windows, Wayland, and signed installers are **not** claimed yet. Packaging remains disabled until the distribution gate (G3) is accepted.

```bash
git clone https://github.com/mibienpanjoe/lyn.git
cd lyn
pnpm install
pnpm tauri dev
```

Default shortcut: `Ctrl+Shift+Space` opens quick capture. The Library window is the main shell.

Other useful commands (full list in [AGENTS.md](AGENTS.md)):

```bash
pnpm test
cargo test --manifest-path src-tauri/Cargo.toml
pnpm tauri build --no-bundle   # production binary without installer packaging
```

## Features

| Area | What you get |
|---|---|
| Quick capture | Text, screenshot paste, voice note; Enter saves and dismisses |
| Context | Live sessions + saved contexts; ambiguity never guesses |
| Library | Chronology by project, detail, play/open media by opaque ID |
| Search | Bounded local FTS over note bodies and user-visible captions |
| Settings | Shortcut, theme, provider tie-break order, optional local speech model |

## Optional local speech

Settings can install a **CPU-only** whisper.cpp engine and multilingual Whisper `base` model after an explicit user action. Model install is the only network-capable path; capture and Library work offline without it. See [`docs/09_local_speech_distribution_decision.md`](docs/09_local_speech_distribution_decision.md).

## Context providers (Linux X11)

Providers are optional. Start Lyn first so private user-only sockets exist, focus the editor or terminal that owns the work, then invoke the shortcut.

- **VS Code:** [`integrations/vscode/`](integrations/vscode/README.md) — `pnpm provider:vscode:package` → install `/tmp/lyn-context-provider.vsix`
- **Kitty:** [`integrations/kitty/`](integrations/kitty/README.md) — watcher path in `kitty.conf`
- **Shell helper:** [`integrations/shell/`](integrations/shell/README.md) — bounded `lyn-context` observations

Providers never send terminal output, editor buffers, or agent chat into Lyn.

## Documentation

- [Project overview](docs/project_overview.md)
- [Requirements](docs/01_requirements_prd.md) · [SRS](docs/02_requirements_srs.md)
- [Architecture](docs/05_architecture.md) · [IPC](docs/06_api_specification.md)
- [Visual identity](docs/07_visual_identity.md)
- [Contributor guidelines](AGENTS.md)

## License

A repository license file is not published yet; treat the project as source-available for local development until the first release records an SPDX license.
