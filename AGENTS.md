# Repository Guidelines

## Project Structure & Module Organization

Lyn has a Svelte/Tauri implementation; `README.md` is the entry point. Read `docs/project_overview.md`, then `01_requirements_prd.md` through `08_context_provider_feasibility.md`. Preserve requirement, invariant, and error IDs across documents.

Svelte 5/TypeScript UI lives under `src/`; the Tauri/Rust core and narrow capability manifest live under `src-tauri/`. Separately delivered local context integrations live under `integrations/`: VS Code reports focus plus local workspace folders, the Kitty watcher reports exact pane focus plus child process identity, and the generic shell bootstrap starts the bounded `lyn-context` helper. All provider traffic uses private Rust-owned sockets and excludes commands, output, environment values, and client-supplied paths. Ordered SQLite migrations live under `src-tauri/migrations/` and must remain immutable after release. Keep presentation and IPC clients in `src/`; storage, capture-session state, context detection, media, and OS integration belong in Rust. Capture-session transitions live in `src-tauri/src/capture/session.rs`; provider observations and their ephemeral registry live under `src-tauri/src/context/`; command gateways live under `src-tauri/src/commands/`, parameterized context/capture persistence stays under `src-tauri/src/storage/`, and platform-specific shortcut/focus/audio/popup-layout adapters stay behind `src-tauri/src/platform/` ports. Recoverable media staging is in `src-tauri/src/media/staging.rs`; PNG/WAV normalization stays under `src-tauri/src/media/`, and startup reconciliation reads durable media references through `src-tauri/src/storage/media_assets.rs`. Staged previews resolve through the read-only `lyn-media://` protocol by opaque ID; none of these boundaries may expose raw filesystem paths to the UI.

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
- `pnpm provider:terminal:test` — run Rust broker, shell bootstrap, and Kitty watcher contract tests.
- `pnpm provider:terminal:build` — build the bounded `lyn-context` shell observation helper.
- `pnpm icons` — regenerate platform icons from the tracked SVG master.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml` — check Rust formatting.
- `cargo test --manifest-path src-tauri/Cargo.toml` — run Rust tests.
- `pnpm build` — build frontend assets.
- `pnpm tauri dev` — open the desktop development shell.
- `pnpm tauri build --no-bundle` — compile the production binary without packaging.

For documentation changes, also run:

- `git diff --check` — detect whitespace errors.
- `rg -n "FR-|INV-|ERR-" docs/` — inspect cross-document identifiers.

Packaging remains disabled until distribution requirements are accepted and verified. Add new exact commands here and to `README.md` in the same change.

## Coding Style & Naming Conventions

Write concise Markdown with descriptive headings and relative links. Keep numbered filenames stable. Never present proposed designs as implemented or verified.

Use `rustfmt` and Prettier. Use `snake_case` for Rust modules/functions, `PascalCase` for Rust types and Svelte components, and kebab-case for TypeScript utility files. Import individual outline icons from `@lucide/svelte/icons/*`; keep visible labels on capture actions and match icon stroke to adjacent text. Preserve typed Tauri IPC; the frontend must not access SQLite or arbitrary filesystem paths.

## Testing Guidelines

Every behavior change must include tests at its owning boundary. Prioritize Rust domain tests, IPC serialization/negative tests, media recovery tests, and keyboard/accessibility UI tests. Follow the verification matrices in documents 05–07. Record commands once a runner exists.

## Commit & Pull Request Guidelines

History is small; prefer concise Conventional Commit-style messages such as `docs: clarify capture invariants`, `feat: add text capture`, or `test: cover duplicate saves`. Keep commits atomic.

Pull requests should explain scope/rationale, link requirements or issues, list verification, and call out deferred work. Include screenshots or recordings for UI changes and identify contract, invariant, dependency, permission, or data-model changes.

## Security & Product Boundaries

Lyn is local-first. Core capture must not depend on accounts, cloud APIs, or remote AI. Treat clipboard data, paths, window titles, and media as untrusted. Never log capture content or commit secrets. Project directories must enter through the Rust-owned native picker; expose only expiring one-use tokens and safe labels to the frontend, never raw paths or generic filesystem permissions. Bind automatic context to the pre-popup foreground window; never use a global last-reported session or expose terminal, editor, or agent content. Context correction must preserve the draft. Preserve “save first, enrich afterward” and user-authored metadata precedence.

The Rust capture-session service owns the single active session and save idempotency. Commands must not derive session state from popup mount/unmount events or perform persistence outside the service's serialized `save_once` boundary. Text blankness checks must not rewrite the body; durable text and its FTS projection are accepted only after the SQLite transaction commits.

Update this guide in the same change whenever repository state, commands, structure, contracts, or contributor workflows change. Breaking API, invariant, data-model, security, or platform changes require matching guidance updates.
