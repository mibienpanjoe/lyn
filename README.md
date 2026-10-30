# Lyn

<p align="center">
  <img src="src-tauri/icons/lyn-icon.svg" alt="" width="128" height="128" />
</p>

<p align="center">
  <strong>Lyn</strong><br />
  A fast, local-first desktop working-memory companion for developers and focused computer work.
</p>

<p align="center">
  <a href="https://github.com/mibienpanjoe/lyn/releases">Download</a> ·
  <a href="#from-source-development">Build from source</a> ·
  <a href="#context-providers-linux-x11">Context providers</a> ·
  <a href="#documentation">Documentation</a>
</p>

```text
global shortcut → type, paste, or record → Enter → return to work
```

Lyn captures titleless text notes, screenshots, and voice notes, associates them with the project and Git branch you were just in, and keeps them in a chronological Library with local search. Everything stays on your machine.

<p align="center">
  <img src="docs/screenshots/quick-capture.png" alt="Lyn Quick Capture popup" width="720" />
</p>

<p align="center">
  <img src="docs/screenshots/library-recent.png" alt="Lyn Library stream and detail inspection" width="900" />
</p>

## Why Lyn

Traditional note apps interrupt flow: open a notebook, name a page, pick a folder. Lyn is the opposite—an always-available capture surface that preserves the thought **with** the context it came from, then gets out of the way.

- **Local-first:** core capture never depends on accounts, cloud APIs, or remote AI.
- **Context-aware:** optional VS Code, Cursor, Browser, Kitty, and shell providers bind to the pre-popup window; you can always choose manually.
- **Save first:** enrichment (optional local speech captions) never blocks a successful save.

## Download / install

The latest published installers are the **[0.6.8 prerelease](https://github.com/mibienpanjoe/lyn/releases/tag/v0.6.8)**. Download them manually from GitHub Releases; Lyn has no in-app auto-updater. The 0.7.7 builds used for native GNOME validation are local builds and have not been published as a release.

| Artifact | Platform | Notes |
|---|---|---|
| `.deb` | Linux (Debian/Ubuntu-family, x86_64) | Preferred on Pop!_OS / Ubuntu |
| `.tar.gz` | Linux x86_64 | Portable binary + LICENSE + README |
| `.exe` (NSIS) | Windows x86_64 | **Experimental** until owner smoke-tested |

**Signing:** v1 artifacts are **unsigned**. Windows may show SmartScreen / “unknown publisher”; that is expected until Authenticode is added later.

### Support matrix

| Target | Status |
|---|---|
| Pop!_OS 22.04 LTS / Ubuntu-compatible, x86_64, **X11** | Reference package installed; core capture verified through 0.7.7; full GNOME acceptance still in progress |
| Windows x86_64 | Expansion — buildable; claim support only after `.exe` smoke |
| macOS | Deferred |
| Wayland | Deferred |

The native GNOME integration has been tested in local 0.7.7 builds on the reference X11/GTK 3.24.33/VTE 0.68.0 stack. These local results do not imply a published native release. See the [verified cases and remaining acceptance](docs/gnome_native_protocol.md#installed-reference-reception-matrix).

Toolchain for from-source builds: Node 24.12.0, pnpm 10.28.0, Rust 1.96.1, plus [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for the host OS.

### From source (development)

```bash
git clone https://github.com/mibienpanjoe/lyn.git
cd lyn
pnpm install
pnpm tauri dev
```

Default shortcut: `Ctrl+Shift+Space` opens quick capture. The Library window is the main shell.

### Build release packages

```bash
# Linux x86_64 → .deb under src-tauri/target/x86_64-unknown-linux-gnu/release/bundle/deb/
#            → .tar.gz under dist-packages/
pnpm package:linux

# Windows host → NSIS .exe under src-tauri/target/release/bundle/nsis/
pnpm package:windows
```

Linux packaging embeds the native GNOME module and requires the reference GTK 3.24.33 / VTE 0.68.0 headers and runtime, GCC and pkg-config. `LYN_GNOME_SDK_ROOT` can supply extracted VTE headers as described in [AGENTS.md](AGENTS.md); users of the resulting packages do not need that build SDK. Other native stacks refuse before packaging. The command verifies the embedded module in both packages and checks their binary contents, allowing only Tauri’s Debian format marker; it does not install or activate the integration. The release workflow uses the same native build and package checks.

For the isolated native resource check after packaging:

```bash
pnpm provider:gnome:package:test \
  --receipt dist-packages/lyn-0.7.7-linux-x86_64-native.json \
  --portable dist-packages/lyn-0.7.7-linux-x86_64.tar.gz \
  --deb src-tauri/target/x86_64-unknown-linux-gnu/release/bundle/deb/Lyn_0.7.7_amd64.deb
```

This developer check needs the [GNOME fixture prerequisites](AGENTS.md). It verifies and loads the package-extracted module in private Xvfb/D-Bus, without rebuilding that module or installing either package. It does not launch the packaged GUI.

`pnpm provider:gnome:package:gui:test` accepts the same three artifact arguments and runs the ordinary packaged GUI with no checkout access. It additionally needs Bubblewrap, Openbox, AT-SPI and Tesseract with English data (system-installed or extracted into the local SDK). It masks the real home and isolates files, display, bus and network, then verifies shortcut/manual capture, SQLite and the painted Library through local OCR. Native GNOME compatibility remains a separate check because the user namespace cannot attest unmapped root-owned files.

The **release** workflow builds Linux and Windows separately, then verifies both artifact sets before publication. A manual run on a branch only verifies and retains workflow artifacts. A matching `v<version>` tag uploads a draft and publishes the prerelease after both builds succeed; the version must agree in `package.json`, `Cargo.toml` and `tauri.conf.json`. Existing releases are never overwritten by this workflow.

Other useful commands (full list in [AGENTS.md](AGENTS.md)):

```bash
pnpm test
cargo test --manifest-path src-tauri/Cargo.toml
```

## Features

| Area | What you get |
|---|---|
| Quick capture | Text, screenshot paste, voice note; Enter saves and dismisses |
| Context | Live sessions + saved contexts; ambiguity never guesses |
| Library | Newest captures first, project timelines, detail inspection, media playback and capture deletion |
| Projects | Remove an empty project from the Library without deleting its folder |
| Search | Bounded local FTS over note bodies and user-visible captions |
| Settings | Shortcut, theme, provider tie-break order, 1-click integrations installer, optional local speech model, installed app version |

## Optional local speech

Settings can install a **CPU-only** whisper.cpp engine and multilingual Whisper `base` model after an explicit user action. Model install is the only network-capable path; capture and Library work offline without it. See [`docs/09_local_speech_distribution_decision.md`](docs/09_local_speech_distribution_decision.md).

## Context providers (Linux X11)

Providers are optional. Configure them in **Settings → Integrations & Context Providers**, or follow the manual guides below. Browser setup also requires loading the companion extension in your browser.

- **VS Code & Cursor:** [`integrations/vscode/`](integrations/vscode/README.md) — 1-click install in Lyn Settings, or `pnpm provider:vscode:package` → install `/tmp/lyn-context-provider.vsix` via `code` or `cursor`
- **Browser (Chrome, Chromium, Brave, Edge, Firefox):** [`integrations/browser/`](integrations/browser/README.md) — 1-click register in Lyn Settings, or `pnpm provider:browser:install` → load unpacked extension in browser
- **Kitty:** [`integrations/kitty/`](integrations/kitty/README.md) — 1-click configure in Lyn Settings, or watcher path in `kitty.conf`
- **Shell helper:** [`integrations/shell/`](integrations/shell/README.md) — 1-click add to `~/.bashrc` in Lyn Settings. Opening Lyn refreshes the helper automatically; start a new terminal after an update.

On the reference Linux/X11 stack, a locally built 0.7.7 package can also prepare the native GNOME Terminal integration from Settings. It identifies the invoking window and tab using verified local metadata. When an exact source cannot be established, Lyn asks you to choose a context. Activation and compatibility are limited to the [documented reference stack and acceptance scope](docs/gnome_native_protocol.md#installed-reference-reception-matrix).

Providers never send terminal output, editor buffers, or agent chat into Lyn.

## Documentation

- [Project overview](docs/project_overview.md)
- [Requirements](docs/01_requirements_prd.md) · [SRS](docs/02_requirements_srs.md)
- [Architecture](docs/05_architecture.md) · [IPC](docs/06_api_specification.md)
- [Visual identity](docs/07_visual_identity.md)
- [HEX (Anomaly) research note](docs/research_hex_anomaly.md)
- [Contributor guidelines](AGENTS.md)

## License

SPDX: [MIT](LICENSE)

Optional local speech downloads (whisper.cpp / Whisper `base`) are also MIT-licensed upstream; release notes must retain their attribution when those components are documented.
