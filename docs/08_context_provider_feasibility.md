# Context Provider Feasibility — G1

**Status:** Accepted for implementation on the first target; live desktop matrix pending owner verification.

**Date:** 2026-08-30.

**Target:** Pop!_OS 22.04 LTS, GNOME on X11.

## Decision

Lyn accepts local provider observations only through the Rust-side contract in `context/provider.rs`. An observation may contain provider/source kind, opaque window/process/session correlation, one workspace or working directory, observation time, and liveness. It cannot contain window titles, terminal commands or output, editor buffers, clipboard data, or agent conversations.

Automatic selection requires a relationship to the window captured before Lyn appears. A merely recent observation is never sufficient. If a provider cannot establish the active editor window or terminal tab/session, it contributes an ambiguous candidate or no candidate; manual context selection remains available.

## First-target support matrix

| Source | Required local evidence | G1 decision |
|---|---|---|
| X11 foreground window | `_NET_ACTIVE_WINDOW` captured before popup focus | Supported as an opaque invocation correlation; it does not reveal a project by itself. |
| VS Code workspace window | A local VS Code integration supplies the exact editor-window correlation and workspace directory | Supported by the provider contract; integration delivery is deferred. Window-title parsing is rejected. |
| VS Code integrated terminal | Owning editor-window correlation, distinct active-terminal session correlation, and that terminal's cwd/workspace | Supported only with all three relationships. Missing active-terminal evidence is ambiguous. |
| GNOME Terminal or Kitty tab | Exact foreground-window correlation plus an integration that identifies the active tab/session and cwd | Unsupported without a terminal-specific integration; multiple tabs must remain ambiguous. |
| Shell session | Distinct process/session correlation and cwd, related to the invocation window by verified local evidence | Supported by the provider contract; a globally recent shell report cannot auto-select. |
| Non-Git directory | Verified directory evidence | Degrades to explicit/manual standalone flow; no project identity is fabricated. |

VS Code, GNOME Terminal, and Kitty are the applications selected for the first reproducible desktop matrix. The provider contract deliberately keeps VS Code windows, integrated terminals, external terminals, and shell sessions as distinct source kinds.

## Reproducible desktop matrix

Run the following cases through the normal `pnpm tauri dev` application on the reference desktop. Use two different repositories and branches so an incorrect choice is visible. No test step may inspect commands, output, editor contents, or agent conversations.

1. Focus VS Code workspace A, invoke Lyn, and confirm A is the only exact-window candidate.
2. Focus a VS Code integrated terminal in workspace B, invoke Lyn, and confirm the editor window plus active terminal session resolves B.
3. Open two integrated terminals with different cwd values; when active-terminal correlation is absent, confirm Lyn reports ambiguity.
4. Open two GNOME Terminal tabs, then two Kitty tabs, with different cwd values; without a supported tab integration, confirm Lyn reports ambiguity rather than selecting the newest report.
5. Keep a newer background observation for workspace B, focus the verified window for A, and confirm A wins.
6. Close a selected live source before save and confirm the draft remains intact while Lyn asks for another context.
7. Repeat with a linked Git worktree and confirm the common project identity is shared while branch/worktree labels reflect the selected source.

The current automated environment cannot attach to the desktop X display, so these live rows are acceptance evidence for Checkpoint C and are not yet marked verified.

## Rejected approaches

- Parsing window titles: titles are untrusted, configurable, truncated, and may expose content.
- Choosing the globally most recent provider report: this violates invocation-bound context under concurrent windows.
- Persisting live observations: correlations and cwd/workspace evidence are ephemeral and must not enter SQLite or logs.
- Treating a terminal process as its active tab: one process may own several tabs or panes with different working directories.

## Consequences

- T14 may validate and retain observations in memory, issue opaque source IDs, and expire them deterministically.
- T15 must rank exact foreground association ahead of verified relations and recency.
- T16 must revalidate a selected live source at selection and save time.
- Unsupported integrations reduce convenience, not core capture availability.
