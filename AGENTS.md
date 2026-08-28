# Repository Guidelines

## Project Structure & Module Organization

Lyn is documentation-only; `README.md` is the entry point. Read `docs/project_overview.md`, then `01_requirements_prd.md` through `07_visual_identity.md`. Preserve requirement, invariant, and error IDs across documents.

The proposed layout places Svelte 5/TypeScript UI under `src/` and the Tauri/Rust core under `src-tauri/src/`. Keep presentation and IPC clients in `src/`; storage, context detection, media, and OS integration belong in Rust.

## Build, Test, and Development Commands

No package manifest or executable tooling is committed, so no build, test, lint, or development commands exist. For documentation-only changes, run:

- `git diff --check` — detect whitespace errors.
- `rg -n "FR-|INV-|ERR-" docs/` — inspect cross-document identifiers.

When tooling is introduced, add its exact commands here and to `README.md` in the same change.

## Coding Style & Naming Conventions

Write concise Markdown with descriptive headings and relative links. Keep numbered filenames stable. Never present proposed designs as implemented or verified.

For future code, use `rustfmt` and the configured Svelte/TypeScript formatter. Use `snake_case` for Rust modules/functions, `PascalCase` for Rust types and Svelte components, and kebab-case for TypeScript utility files. Preserve typed Tauri IPC; the frontend must not access SQLite or arbitrary filesystem paths.

## Testing Guidelines

Every behavior change must include tests at its owning boundary. Prioritize Rust domain tests, IPC serialization/negative tests, media recovery tests, and keyboard/accessibility UI tests. Follow the verification matrices in documents 05–07. Record commands once a runner exists.

## Commit & Pull Request Guidelines

History is small; prefer concise Conventional Commit-style messages such as `docs: clarify capture invariants`, `feat: add text capture`, or `test: cover duplicate saves`. Keep commits atomic.

Pull requests should explain scope/rationale, link requirements or issues, list verification, and call out deferred work. Include screenshots or recordings for UI changes and identify contract, invariant, dependency, permission, or data-model changes.

## Security & Product Boundaries

Lyn is local-first. Core capture must not depend on accounts, cloud APIs, or remote AI. Treat clipboard data, paths, window titles, and media as untrusted. Never log capture content or commit secrets. Bind automatic context to the pre-popup foreground window; never use a global last-reported session or expose terminal, editor, or agent content. Context correction must preserve the draft. Preserve “save first, enrich afterward” and user-authored metadata precedence.

Update this guide in the same change whenever repository state, commands, structure, contracts, or contributor workflows change. Breaking API, invariant, data-model, security, or platform changes require matching guidance updates.
