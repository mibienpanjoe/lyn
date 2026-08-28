# Lyn — Product Requirements Document

Version: v1.1, 2026-08-28

Source: [`project_overview.md`](project_overview.md)

## 1. Problem Statement

Developers and other focused computer users encounter useful information continuously while working: implementation ideas, bugs to revisit, prompt fragments, decisions, screenshots, and short spoken observations. These items are valuable working context, but recording them in a conventional notes application usually requires enough navigation, naming, and organization to interrupt concentration.

When capture is slower than the thought, users either break their flow or lose the context. The problem is therefore not a lack of places to store notes. It is the absence of an always-available, low-friction working-memory surface that preserves a thought together with the context in which it occurred.

Lyn must solve that capture problem before expanding into broader note management or agent collaboration.

## 2. Personas

### Primary persona — Developer in active flow

A developer moving between an editor, terminal, browser, and coding agent who needs to preserve a thought without leaving the current task.

Needs:

- invoke capture from anywhere with one global shortcut;
- start typing or paste immediately;
- retain the current project and Git branch without manual filing;
- save and return to work using the keyboard;
- find the capture later by chronology or search.

### Secondary persona — Focused computer worker

A researcher, designer, writer, or technical operator who wants the same fast capture loop but may not be working inside a Git repository.

Needs:

- create and reuse standalone contexts;
- capture text, screenshots, and voice;
- keep all material local by default;
- retrieve captures without adopting a full knowledge-management system.

### Tertiary persona — Developer preparing context for a coding agent

A developer who wants to accumulate reliable project observations now and later select them as input for Claude, Codex, or another coding agent.

This persona informs the long-term direction, but agent-oriented export is not part of the MVP.

## 3. Solution Overview

Lyn is a lightweight, local-first desktop working-memory companion. A global shortcut opens a compact capture popup with the input already focused. The user types, pastes a screenshot, or records a voice note, then presses Enter to save and close. Lyn associates the capture with the project and Git branch bound to the previously focused VS Code window or terminal session. The user can replace that detection with another live session or saved context without losing the draft.

Captured information is reviewed separately in a Library. Each project or standalone context has one chronological stream. Branches remain metadata on individual captures rather than fragmenting the project into separate libraries. Full-text search covers text notes and user-visible media captions.

Optional local intelligence may enrich saved material, but saving never depends on enrichment, an account, an internet connection, or a paid AI service.

## 4. MVP Scope

### 4.1 Desktop capture lifecycle

- Register a configurable global shortcut.
- Open a compact capture popup from another application.
- Focus the text input immediately.
- Save and close with `Enter`.
- Insert a newline with `Shift + Enter`.
- Cancel without saving with `Esc`.
- Keep Lyn available in the background without requiring the Library to remain visible.

### 4.2 Context detection and selection

- Resolve context through provider-based detection, beginning with shell, VS Code, foreground-window, and manual providers.
- Record the previously focused OS window before the Lyn popup takes focus.
- Correlate that window with the exact live VS Code window, integrated terminal, external terminal, shell, or coding-agent working directory when reliable evidence exists.
- Prefer invocation-bound evidence over a global “most recently reported” project; use configured provider order only to break ties between equally reliable evidence.
- Associate a detected repository with a project context.
- Record the current Git branch as capture metadata when a branch is available.
- Treat Git worktrees sharing one Git common directory as one project while retaining each worktree's current branch on its captures.
- Make the displayed context selectable from the popup and list other live sessions alongside saved project and standalone contexts.
- Preserve all draft text and staged media when the user changes the selected source.
- Make explicit user selection authoritative for the current capture and refresh its branch before saving.
- Remove dead sessions from the chooser and require reselection when a chosen session becomes stale or ambiguous.
- Ask the user to select or create a context before saving when automatic detection cannot produce one.
- Support standalone contexts that do not map to a Git repository.
- Keep a single continuous history per project; do not create a separate library per branch.

### 4.3 Text capture

- Accept plain text without requiring a capture-type mode.
- Preserve text notes without generating or requiring titles.
- Support multi-line notes through `Shift + Enter`.

### 4.4 Screenshot capture

- Accept a pasted image in the normal capture surface.
- Present an image preview before saving.
- Accept an optional manual caption.
- Copy saved images into Lyn-managed local storage as PNG files.
- Allow images to be previewed in Lyn, opened directly, or opened in the operating system's default viewer.
- When no caption is supplied, optionally derive a lightweight caption from already available context metadata without a vision model.

### 4.5 Voice capture

- Record audio inside the capture popup.
- Allow the user to stop and play back the recording before saving.
- Accept an optional manual caption.
- Store initial voice notes as 16 kHz, mono, 16-bit PCM WAV files.
- Optionally produce a local transcript-derived caption when the user leaves the caption blank.
- Allow saving immediately whether or not local transcription is installed or complete.

### 4.6 Library and retrieval

- Display captures as a chronological stream.
- Provide entry points for Recent, All captures, Projects, Standalone contexts, and Search.
- Show capture time, type, context, and Git branch metadata when present.
- Preview text, screenshots, and playable voice notes.
- Search text notes, screenshot captions, and voice-note captions with local full-text search.

### 4.7 Local storage and settings

- Store structured data and settings in local SQLite storage.
- Store screenshots and audio as local files under Lyn-managed application data.
- Keep the frontend behind typed Tauri commands rather than giving it direct SQL or unrestricted filesystem access.
- Allow the user to enable optional local speech recognition and download a compatible local model.

## 5. Out of Scope for the MVP

The following are explicitly excluded:

- a web backend or hosted service;
- user accounts or authentication;
- cloud synchronization or collaboration;
- Firebase, Supabase, PostgreSQL, or another remote database;
- cloud LLM or paid AI API dependencies;
- vector databases, embeddings, or semantic search;
- OCR or screenshot-content search;
- vision-model screenshot analysis;
- guaranteed search over raw voice transcripts;
- a rich-text editor or document-oriented note pages;
- folders, complex tagging, workflow statuses, or task management in quick capture;
- manual branch editing in quick capture; branch follows the selected live session;
- persistent automatic pinning of all future captures to a manually selected live session;
- separate project libraries for each Git branch;
- a plugin ecosystem;
- automatic agent interaction or agent-context export;
- broad metadata collection such as active-file history unless separately justified later.

## 6. Success Criteria

The overview does not provide validated numerical targets. The following are **proposed MVP acceptance targets** and must be confirmed through prototype measurement and user testing rather than treated as existing evidence.

### Capture effectiveness

- A user can invoke Lyn, enter a text thought, save it, and return to the previous application without touching the mouse.
- At least 95% of valid capture attempts in acceptance testing produce exactly one durable capture.
- Cancelled captures produce no Library item or retained staged media.
- A saved capture never waits for optional caption generation or transcription.

### Perceived speed

- The capture popup is ready for keyboard input within 250 ms at the 95th percentile on the agreed reference machine.
- A text capture is durably accepted within 150 ms at the 95th percentile after Enter, excluding first-run database initialization.
- Search results for 10,000 local captures appear within 200 ms at the 95th percentile on the agreed reference machine.

### Context usefulness

- When invoked from a supported shell or VS Code workspace inside a Git repository, Lyn assigns the correct project and branch in at least 95% of scripted acceptance cases.
- Concurrent-session acceptance covers multiple VS Code windows, integrated terminals, external terminal tabs, coding agents, and Git worktrees without allowing an unrelated recent provider report to win.
- A user can replace the detected source with another live session or saved context without losing any draft content or staged media.
- Selecting a live session refreshes its project/worktree and named branch before save.
- When context cannot be resolved confidently, Lyn requests a manual choice instead of silently filing the capture under an unrelated context.

### Local-first integrity

- Text, images, audio, search, and Library browsing work with network access disabled.
- No core capture data leaves the device during normal MVP operation.
- Removing or disabling the optional speech model does not impair capture, Library, or caption search.

### Product discipline

- The quick-capture surface contains no folders, tag manager, rich-text toolbar, status selector, or branch selector.
- A first-time user can complete text, screenshot, and voice capture scenarios without external instructions during usability validation.

## 7. Product Decision Rule

When a proposed feature conflicts with the primary capture loop, preserve the loop:

```text
global shortcut → capture → Enter → return to work
```

Organization and agent collaboration may grow around that loop, but must not make it slower, less predictable, or dependent on remote infrastructure.
