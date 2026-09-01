# Context Provider Feasibility — G1

**Status:** Accepted and Checkpoint B verified; VS Code, Kitty, and bounded shell/terminal providers are implemented. Exact VS Code and Kitty invocation binding is owner-confirmed on the reference desktop, with ambiguity, staleness, privacy-boundary, and draft-preservation cases covered by automated tests.

**Date:** 2026-09-01.

**Target:** Pop!_OS 22.04 LTS, GNOME on X11.

## Decision

Lyn accepts local provider observations only through the Rust-side contract in `context/provider.rs`. An observation may contain provider/source kind, opaque window/process/session correlation, one workspace or working directory, observation time, and liveness. It cannot contain window titles, terminal commands or output, editor buffers, clipboard data, or agent conversations.

Automatic selection requires a relationship to the window captured before Lyn appears. A merely recent observation is never sufficient. If a provider cannot establish the active editor window or terminal tab/session, it contributes an ambiguous candidate or no candidate; manual context selection remains available.

## First-target support matrix

| Source | Required local evidence | G1 decision |
|---|---|---|
| X11 foreground window | `_NET_ACTIVE_WINDOW` captured before popup focus | Supported as an opaque invocation correlation; it does not reveal a project by itself. |
| VS Code workspace window | A local VS Code integration supplies the exact editor-window correlation and workspace directory | Implemented and owner-verified for Linux X11 by the local VSIX and Rust-owned user-only socket. Delayed reports are accepted only while a supported VS Code window is active; remote and multi-root workspaces remain manual. |
| VS Code integrated terminal | Owning editor-window correlation, distinct active-terminal session correlation, and that terminal's cwd/workspace | Implemented through the bounded shell helper for one distinguishable terminal session. Multiple sessions sharing the editor window remain ambiguous unless exact active-session evidence is added. |
| Kitty tab or pane | Exact foreground-window correlation plus an integration that identifies the active pane/session and cwd | Implemented through a global Kitty focus watcher. It reports only pane focus and child-process identity; Rust derives cwd and revalidates the active Kitty X11 window. |
| GNOME Terminal tab | Exact foreground-window correlation plus a distinguishable shell session and cwd | Implemented for one distinguishable session per OS window through the bounded shell helper. Multiple tabs sharing the window remain ambiguous rather than using recency. |
| Shell session | Distinct process/session correlation and cwd, related to the invocation window by verified local evidence | Implemented by the `lyn-context` helper and private Rust broker. The helper sends no cwd; Rust derives it from the validated same-user process. |
| Non-Git directory | Verified directory evidence | Degrades to explicit/manual standalone flow; no project identity is fabricated. |

VS Code, GNOME Terminal, and Kitty are the applications selected for the first reproducible desktop matrix. The provider contract deliberately keeps VS Code windows, integrated terminals, external terminals, and shell sessions as distinct source kinds.

## Reproducible desktop matrix

Run the following cases through the normal `pnpm tauri dev` application on the reference desktop. Use two different repositories and branches so an incorrect choice is visible. No test step may inspect commands, output, editor contents, or agent conversations.

1. Focus VS Code workspace A, invoke Lyn, and confirm A is the only exact-window candidate.
2. Focus a VS Code integrated terminal in workspace B, invoke Lyn, and confirm the editor window plus active terminal session resolves B.
3. Open two integrated terminals with different cwd values; when active-terminal correlation is absent, confirm Lyn reports ambiguity.
4. Open two GNOME Terminal tabs with different cwd values and confirm Lyn reports ambiguity; repeat with two Kitty panes and confirm the installed Kitty watcher resolves only the focused pane.
5. Keep a newer background observation for workspace B, focus the verified window for A, and confirm A wins.
6. Close a selected live source before save and confirm the draft remains intact while Lyn asks for another context.
7. Repeat with a linked Git worktree and confirm the common project identity is shared while branch/worktree labels reflect the selected source.

Checkpoint B was accepted on 2026-09-01 from the owner-confirmed VS Code and Kitty reference-desktop flow together with the automated provider matrix. The live run confirmed automatic project/branch binding from the invoking VS Code window and Kitty pane, provider persistence while Lyn owns focus, and deliberate non-selection of an unrelated live source. Automated tests cover bounded messages, same-user process validation, focus-class validation, exact Kitty-pane replacement, safe generic-terminal ambiguity, stale remapping, exact-window resolution, provider failure isolation, and draft-preserving chooser behavior.

## Rejected approaches

- Parsing window titles: titles are untrusted, configurable, truncated, and may expose content.
- Choosing the globally most recent provider report: this violates invocation-bound context under concurrent windows.
- Persisting live observations: correlations and cwd/workspace evidence are ephemeral and must not enter SQLite or logs.
- Treating a terminal process as its active tab: one process may own several tabs or panes with different working directories. The Kitty watcher supplies exact pane focus; generic multi-tab terminals remain ambiguous.

## Consequences

- T14 may validate and retain observations in memory, issue opaque source IDs, and expire them deterministically.
- T15 must rank exact foreground association ahead of verified relations and recency.
- T16 must revalidate a selected live source at selection and save time.
- Unsupported integrations reduce convenience, not core capture availability.
- The delivered VS Code extension reports only an ephemeral instance ID, focus state, and local workspace folders. The Rust broker validates the active X11 window class before attaching an opaque correlation and never persists observations.
- The Kitty watcher reports only exact pane focus plus child-process identity. It does not enable Kitty remote control or inspect screen text, commands, output, titles, environment values, or cwd.
- The generic helper reports only opaque session, process, and X11-window correlation. The broker validates the same-user process and derives cwd inside Rust before registration.
