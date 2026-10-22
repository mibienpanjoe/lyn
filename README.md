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

Installers are published on **[GitHub Releases](https://github.com/mibienpanjoe/lyn/releases)** (manual download; no in-app auto-updater).

| Artifact | Platform | Notes |
|---|---|---|
| `.deb` | Linux (Debian/Ubuntu-family, x86_64) | Preferred on Pop!_OS / Ubuntu |
| `.tar.gz` | Linux x86_64 | Portable binary + LICENSE + README |
| `.exe` (NSIS) | Windows x86_64 | **Experimental** until owner smoke-tested |

**Signing:** v1 artifacts are **unsigned**. Windows may show SmartScreen / “unknown publisher”; that is expected until Authenticode is added later.

### Support matrix

| Target | Status |
|---|---|
| Pop!_OS 22.04 LTS / Ubuntu-compatible, x86_64, **X11** | Verified baseline (dev); package install pending owner smoke |
| Windows x86_64 | Expansion — buildable; claim support only after `.exe` smoke |
| macOS | Deferred |
| Wayland | Deferred |

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
# Linux host → .deb under src-tauri/target/release/bundle/deb/
#            → .tar.gz under dist-packages/
pnpm package:linux

# Windows host → NSIS .exe under src-tauri/target/release/bundle/nsis/
pnpm package:windows
```

Upload those assets to a GitHub Release, or push a `v*` tag / run the **release** workflow so CI builds Linux + Windows and attaches them automatically.

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
| Library | Chronology by project, detail inspection, capture deletion, play/open media by opaque ID |
| Search | Bounded local FTS over note bodies and user-visible captions |
| Settings | Shortcut, theme, provider tie-break order, 1-click integrations installer, optional local speech model, installed app version |

## Optional local speech

Settings can install a **CPU-only** whisper.cpp engine and multilingual Whisper `base` model after an explicit user action. Model install is the only network-capable path; capture and Library work offline without it. See [`docs/09_local_speech_distribution_decision.md`](docs/09_local_speech_distribution_decision.md).

## Context providers (Linux X11)

Providers are optional. You can configure all of them with **1-click directly in Lyn Settings (under Integrations & Context Providers)** with zero terminal commands, or manually using the guides below:

- **VS Code & Cursor:** [`integrations/vscode/`](integrations/vscode/README.md) — 1-click install in Lyn Settings, or `pnpm provider:vscode:package` → install `/tmp/lyn-context-provider.vsix` via `code` or `cursor`
- **Browser (Chrome, Chromium, Brave, Edge, Firefox):** [`integrations/browser/`](integrations/browser/README.md) — 1-click register in Lyn Settings, or `pnpm provider:browser:install` → load unpacked extension in browser
- **Kitty:** [`integrations/kitty/`](integrations/kitty/README.md) — 1-click configure in Lyn Settings, or watcher path in `kitty.conf`
- **Shell helper:** [`integrations/shell/`](integrations/shell/README.md) — 1-click add to `~/.bashrc` in Lyn Settings. Opening Lyn refreshes the helper automatically; start a new terminal after an update.

Providers never send terminal output, editor buffers, or agent chat into Lyn.

### Native GNOME prototype (development only)

`pnpm provider:gnome:test` compiles an experimental GTK3/VTE module and the current Rust test harness. It tests real GNOME Terminal windows, tabs, TUI identity, and native module-to-Rust broker exchanges on private Xvfb/D-Bus, including late broker startup and reconnection with a fresh generation. Channel start/stop tests check FD release and cancellation of retry. It does not install anything or enable native automatic context in your daily terminal.

The backend suite also saves synthetic PNG/WAV captures and checks media preservation through native generation loss and manual context correction. This does not exercise the desktop media controls or microphone; see the [native coverage and limits](docs/gnome_native_protocol.md#capture-integration-and-continuity).

`pnpm provider:gnome:test --pid-reuse` additionally reuses a real PID inside a disposable user/PID/mount namespace and requires the old pidfd-backed peer to remain refused. This optional kernel test needs `/usr/bin/unshare` with unprivileged user namespaces enabled, runs only as an ordinary non-root user, and never writes the host PID counter. It is not GNOME window-ID reuse or installed-package acceptance.

`pnpm provider:gnome:test --reconnect-cycles=20` repeats real Rust broker shutdown/restart against one private GNOME server. Every reconnect requires a distinct generation, unchanged native metadata and an exact backend save; the server's open-FD count must match before and after the series. Add `--reconnect-discovery` to exercise fresh published endpoints through the real C discovery reader instead of the fixed fixture address. These bounded clean-restart checks do not establish long-running memory stability, repeated crash recovery or installed acceptance.

`pnpm provider:gnome:test --native-safety` adds ASan/UBSan transport, lifecycle and discovery fixtures with leak detection enabled (requires GCC sanitizer runtimes). Ordinary runs also test silent peers, partial input and saturated private sockets, checking exact native deadline boundaries separately from real-time non-blocking behavior. This does not instrument the GNOME-loaded module or establish full GTK/memory safety.

`pnpm provider:gnome:test --native-metrics` compares short CPU/RSS observations from separate private two-window GNOME servers, without and with a test-instrumented module, then sends 100 metadata queries. Callback-body count/total/max are reported separately as cumulative measurements. This is not an exact module-overhead benchmark, prolonged stability test or installed acceptance.

`pnpm provider:gnome:test --event-storm` exercises 200 private GNOME tab switches concurrently with bounded metadata queries, reporting CPU ticks, sampled RSS, callback bodies and before/after FD counts. It does not capture notes or establish installed acceptance; see the [coverage limits](docs/gnome_native_protocol.md#capture-integration-and-continuity).

`pnpm provider:gnome:desktop:test` exercises the real Tauri popup, global shortcut, native context, SQLite save, rendered Library and focus return in the same isolated environment. It additionally requires Openbox and system Python GI AT-SPI/GDK. Both native test commands use `/usr/bin/python3` so a Conda environment cannot replace the GI-enabled interpreter. Alongside the explicit-readiness scenario, it creates two windows concurrently and captures once from each without waiting for native readiness. Use `pnpm provider:gnome:desktop:test --startup-pairs=20` for twenty independent startup pairs; each fixture keeps its private 60-second lifetime. Reconnect and installed-package acceptance remain separate checks. Screenshots are kept under local, ignored `tasks/`.

`pnpm provider:gnome:desktop:test --warm-alternations=100` adds a continuous series on the same two windows, then the same two tabs with distinct project directories. Each series saves an initial note followed by 100 alternating captures, checks every SQLite result and focus return, and inspects the latest five Library rows. Tab creation uses a synthetic command in the fixture's private settings, never the user's shell profiles. The per-fixture 60-second limit still applies.

The warm runner separately reports shortcut-to-focused-input and shortcut-to-rendered-context p50/p95/max, excluding only the separately reported initial invocation. Its 100-sample gates require p95 ≤ 250 ms for each conservative AT-SPI observation bound; this is isolated fixture coverage, not installed-package or cold-start acceptance.

`pnpm provider:gnome:desktop:test --tab-relocation` adds five captures across two tabs, detaches one into a new window and verifies source/PTY/process continuity, changed window/topology and captures from both resulting windows.

`pnpm provider:gnome:desktop:test --invocation-races` switches A→B and A→B→A after Rust freezes the real invocation. A private `cfg(test)` scheduling barrier preserves the original 120 ms reply deadline. Both cases must receive the native topology-change refusal, keep the draft visible and refuse saving without a context; a timeout does not count as success.

`pnpm provider:gnome:desktop:test --late-app` starts Lyn only after its private GNOME window is mapped and reporting native metadata, with no broker endpoint present. It checks the same server/source survives app startup, then exercises popup, exact save, painted Library and focus return. This is not app-restart or installed-package acceptance.

`pnpm provider:gnome:desktop:test --image-closed-source` pastes a synthetic image through the private desktop clipboard, invokes capture again, then closes the original terminal. It checks that the refused save preserves the image and caption, and that manual context correction saves the exact media and renders it in Library. It does not use your clipboard or microphone.

`pnpm provider:gnome:desktop:test --audio-closed-source` exercises Voice/Stop with a test-only synthetic input, then checks the same closed-source/refused-save/manual-correction flow for WAV and its caption. No microphone is opened, and playback/transcription are not tested by this case.

`pnpm provider:gnome:desktop:test --module-absent` runs GNOME without the Lyn module and checks that typing and manual capture still work, with the draft preserved, an exact save, focus return and a rendered Library row. This is not a latency benchmark or slow/disconnected-module acceptance.

`pnpm provider:gnome:desktop:test --module-slow` pauses only the pidfd-pinned private GNOME server after a successful native invocation. It checks typing and manual save while that module cannot respond, then resumes the server and checks the original source survives. No terminal in your session is paused; this is not a latency benchmark or explicit channel-cut acceptance.

`pnpm provider:gnome:desktop:test --channel-cut` shuts down only the private fixture's native broker after admission, then checks typing, draft preservation, manual save, focus and rendered Library while the endpoint remains unavailable. GNOME stays alive. This is not a latency benchmark, isolated stream-cut recovery or installed acceptance.

`pnpm provider:gnome:desktop:test --restart-app` saves before and after a clean Tauri exit/relaunch, keeping the same private GNOME server and SQLite database. It requires endpoint cleanup, unchanged terminal identity, both exact notes painted in the Library and focus return. Crash recovery and stale-draft refusal remain separate checks.

`pnpm provider:gnome:desktop:test --crash-app` kills only the verified, pidfd-pinned Lyn fixture process, then relaunches it with the same GNOME server and database. It requires the abandoned socket to remain untouched, a fresh endpoint/generation, the original foreground process still alive, exact saves on both sides, painted Library notes and focus return. It installs nothing and does not establish installed-package recovery.

`pnpm provider:gnome:desktop:test --stale-reconnect` opens a draft, disconnects only the admitted private native channel and requires a real fresh handshake. It checks the old context cannot save, the draft stays visible and SQLite stays empty, then cancels and saves a fresh capture through the reconnected channel. It does not restart GNOME or bypass peer admission.

`pnpm provider:gnome:desktop:test --closed-source` closes the original GNOME window while its Lyn draft remains open, with another private GNOME window still alive. Saving must refuse the closed source without substituting the survivor or losing the draft. Only an explicit new capture from the surviving window may save.

`pnpm provider:gnome:desktop:test --closed-tab` closes only the original tab, keeping its server and window alive with a second project's tab. The old draft must refuse saving without losing text; after cancellation, a new capture must save only to the surviving tab's project and render in Library with focus restored.

`pnpm provider:gnome:desktop:test --shell-exit` lets a synthetic Bash exit normally through a private FIFO, without terminal input or process signals. Its window stays open, but the old draft must become stale with no save and its text intact. A fresh capture from the surviving project's window must then save/render/focus correctly.

`pnpm provider:gnome:desktop:test --stale-selection` opens the context chooser before a real native reconnection, then clicks its original live-source choice afterward. It requires an explicit stale refusal, a visible error and preserved draft with no save, followed by a successful fresh capture after cancellation.

`pnpm provider:gnome:desktop:test --cwd-change` changes the synthetic foreground process from project alpha to beta while a draft is open, without replacing its process, PTY, native source or window. The old save must refuse with its draft intact; a fresh invocation after cancellation must save only in beta.

`pnpm provider:gnome:desktop:test --pty-reuse` closes the original source and creates a replacement in the same project, requiring actual reuse of its PTY number. The old draft must remain stale with no save; only a fresh invocation may save from the replacement. This does not claim PID or XID number reuse.

`pnpm provider:gnome:desktop:test --xid-reuse` replaces only the pidfd-verified GNOME server in the private desktop fixture, requiring the new window to reuse the original XID. The old draft must refuse saving without losing its text; a fresh invocation must then save, render in Library and return focus. No user terminal is targeted and nothing is installed.

`pnpm provider:gnome:desktop:test --foreground-shapes` checks a shell, an exec-replaced htop, and a child htop whose project differs from its parent shell. Each private fixture verifies its actual foreground process shape, exact project/branch save, painted Library and focus return. Shell profiles are disabled and htop displays only synthetic fixture PIDs.

Requires the Rust toolchain above, Linux with pidfd support, GTK3/VTE development headers, GCC/pkg-config, Xvfb with X-Resource 1.2/xauth, GNOME Terminal, xdotool/xprop, Python with pidfd APIs and GI GTK3, and htop. Developers can set `LYN_GNOME_SDK_ROOT` to a local directory with packages extracted beneath `root/`; the test never installs system packages or modifies terminal profiles outside its temporary environment.

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
