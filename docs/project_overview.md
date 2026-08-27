# Lyn Project Overview

## Why We Are Building Lyn

Lyn is being built to solve a very specific problem: **capturing fleeting thoughts, remarks, prompts, screenshots, and voice notes while working—especially while coding—without interrupting flow**.

When working on a project, useful context appears constantly:

- a bug or inconsistency worth revisiting,
- an implementation idea,
- a remark to give to a coding agent later,
- a prompt fragment,
- a screenshot of an unexpected state,
- a short voice note,
- a decision that should not be forgotten.

Traditional note-taking apps usually introduce too much friction for this kind of working memory. Opening a full notes application, choosing a notebook, creating a page, naming it, and organizing it breaks focus.

Lyn is designed around a much faster interaction:

```text
global shortcut
→ type or paste
→ Enter
→ Lyn disappears
```

The app should act like a **developer's external working memory**: always available, extremely fast, local, organized by context, and easy to revisit later.

---

## What Lyn Is

Lyn is a **local-first, lightweight desktop capture and context-management application**.

Its primary function is fast capture. Its secondary function is organizing those captures into useful project context. Over time, it can also become a bridge between a developer's own working notes and coding agents.

The product hierarchy is:

1. **Fast capture**
2. **Context organization**
3. **Agent collaboration**

Lyn should never become a general-purpose notes application before succeeding at the first problem.

---

## Core Product Experience

### Quick Capture

The main interaction is intentionally minimal:

```text
shortcut
→ capture popup
→ type / paste / record
→ Enter
→ save and close
```

Keyboard behavior:

- `Enter` → save and close
- `Shift + Enter` → new line
- `Esc` → cancel

Text capture should require no mode selection. The cursor is ready immediately when the popup appears.

The capture popup should show the detected context subtly, but organization controls should remain out of the way.

Example:

```text
┌─────────────────────────────────────┐
│ Stipen · feature/auth               │
│                                     │
│ Type or paste anything...           │
│                                     │
│ 📷 Screenshot        🎙 Voice       │
└─────────────────────────────────────┘
```

### Automatic Context Detection

When Lyn is invoked while the user is working on a coding project, it should automatically detect:

- the current project,
- the current Git branch.

Example:

```text
Project: stipen
Branch: feature/auth
```

If no project can be detected, Lyn should ask the user to:

- choose an existing context, or
- create a new context before saving.

Contexts do **not** have to correspond to Git repositories. Standalone contexts such as:

```text
Random ideas
Learning
Digifo
Things to ask later
```

should also be supported.

### Library

Capture and organization are intentionally separated.

The **capture popup** is minimal and optimized for speed.

The **Library** is where users browse, search, inspect, and organize captured information.

Within a project or context, captures are displayed as a **chronological stream**.

Example:

```text
Today

22:14  [feature/auth]
Refresh token should probably be stored differently.

22:08  [feature/auth]
🖼 login-error.png

21:52  [feature/auth]
Need tests for expired access tokens.

21:37  [feature/auth]
🎙 00:34
```

Branches do not create separate libraries. Instead, each item carries its branch as metadata so that a project keeps a single continuous history.

The Library should offer:

```text
Recent
All captures
Projects
Standalone contexts
Search
```

Search in the first version covers:

- text notes,
- screenshot captions,
- voice-note captions.

Screenshot contents and voice-note transcripts do not need to be searchable in the first version.

---

## Capture Types

### Text Notes

Text notes are the simplest and most important capture type.

They remain **titleless**. The text itself is the note.

Examples:

```text
Need to revisit pagination before merging.
```

```text
Ask the agent to preserve backwards compatibility for this command.
```

### Screenshots

A screenshot can be captured or pasted directly into the normal capture surface.

Preferred flow:

```text
shortcut
→ Ctrl+V
→ image preview
→ optional caption
→ Enter
```

Screenshots are copied into Lyn's own local storage instead of merely referencing an external file.

Captions are optional.

If the user writes a caption manually, Lyn uses it directly and does **not** generate another one.

If no caption is written, Lyn may generate a lightweight context-derived title using information already available, such as:

- project,
- Git branch,
- active application,
- active window title,
- filename.

No vision model is required for this first version.

The user should be able to:

- preview screenshots inside Lyn,
- open them directly,
- or open them with the operating system's default image viewer.

### Voice Notes

Voice recording happens inside the same capture popup rather than opening a separate window.

After recording stops, the user can:

- play the recording,
- add a short caption,
- save it.

If the user writes a caption manually, Lyn should not generate another one.

If the caption is left blank, Lyn may optionally generate one locally based on speech transcription.

This automatic enrichment must never block saving.

---

## Intelligence Principle

Lyn may use local intelligence, but **intelligence is an enhancement, never a dependency**.

The rule is:

> If automatic enrichment takes noticeably long, the capture must still be savable immediately and Lyn can finish enrichment afterward locally.

This applies to:

- voice transcription,
- voice-note title generation,
- screenshot metadata enrichment,
- future local AI features.

No paid AI API should be required for Lyn's core experience.

---

# Tech Stack

## Desktop Framework — Tauri 2

**Tauri 2** is the desktop framework.

Why:

- native desktop application,
- significantly lighter than Electron,
- uses the operating system's WebView instead of bundling Chromium,
- Rust backend provides direct access to native capabilities,
- suitable for global shortcuts, clipboard access, window control, filesystem access, tray behavior, and native integrations.

Tauri is a particularly good fit because Lyn is expected to spend most of its time quietly running in the background and appear instantly when summoned.

## Backend / Core — Rust

The application's core logic is implemented in **Rust**.

Rust handles:

- storage,
- capture processing,
- context detection,
- Git branch detection,
- media management,
- audio recording/playback,
- global shortcut infrastructure,
- operating-system integration,
- optional local intelligence.

The Svelte frontend should communicate with Rust through typed Tauri commands instead of containing core application logic.

```text
Svelte UI
   ↓
Tauri IPC
   ↓
Rust service
   ↓
SQLite / Filesystem / OS
```

## Frontend — Svelte 5 + TypeScript

The user interface is built with:

- **Svelte 5**
- **TypeScript**
- **Vite**

Svelte is chosen because Lyn needs a small, responsive, highly polished interface rather than a large browser-style application.

The frontend mainly manages:

- capture UI,
- Library UI,
- search,
- media previews,
- settings,
- transitions and interactions.

SvelteKit is unnecessary because Lyn does not need server-side rendering or web routing infrastructure.

## Database — SQLite + rusqlite

Lyn uses **SQLite** as its local database.

Rust interacts with SQLite through **rusqlite**.

SQLite stores structured information such as:

- contexts,
- projects,
- captures,
- captions,
- branch metadata,
- media metadata,
- timestamps,
- settings.

The frontend should not execute SQL directly.

```text
Svelte
   ↓
Rust command
   ↓
rusqlite
   ↓
SQLite
```

This keeps database logic centralized in the Rust core.

## Search — SQLite FTS5

**FTS5** provides full-text search over Lyn's local data.

It is used for searching:

- text captures,
- screenshot captions,
- voice-note captions.

This avoids introducing external search engines, vector databases, or cloud services.

Search can later support filtering by:

- context,
- project,
- branch,
- date,
- capture type.

## Images — Filesystem + SQLite Metadata

Actual screenshots should be stored as local files rather than SQLite blobs.

Initial format:

```text
PNG
```

This is appropriate for screenshots because it preserves text, UI details, terminal output, and code clearly.

SQLite stores only the associated metadata and file path.

Example:

```text
Lyn/
├── lyn.db
├── media/
│   ├── images/
│   │   ├── <capture-id>.png
│   │   └── ...
│   └── audio/
│       ├── <capture-id>.wav
│       └── ...
└── models/
```

## Audio — CPAL / Rodio

Voice recording and playback should be handled natively in Rust.

### CPAL

Used when low-level control over microphones, devices, streams, and recording is required.

### Rodio

Used for higher-level audio playback and, where appropriate, recording APIs built on top of CPAL.

Initial voice-note format:

```text
WAV
16 kHz
mono
16-bit PCM
```

This format is simple, reliable, and directly suitable for local speech recognition.

## Optional Local Speech Recognition — whisper.cpp

**whisper.cpp** can provide local speech-to-text functionality.

Its role in Lyn is optional.

It should not be bundled as a mandatory heavyweight dependency. A user may enable it through a Local Intelligence setting and download a small model locally.

Voice workflow:

```text
audio
→ whisper.cpp
→ transcript
→ lightweight local title extraction
```

A separate LLM is not required just to produce a short title.

---

# Project Architecture

A possible Rust structure:

```text
src-tauri/src/
│
├── capture/
│   ├── mod.rs
│   └── service.rs
│
├── context/
│   ├── mod.rs
│   ├── resolver.rs
│   ├── git.rs
│   ├── shell.rs
│   └── vscode.rs
│
├── storage/
│   ├── mod.rs
│   ├── db.rs
│   ├── migrations.rs
│   ├── captures.rs
│   ├── contexts.rs
│   └── search.rs
│
├── media/
│   ├── mod.rs
│   ├── images.rs
│   └── audio.rs
│
├── platform/
│   ├── shortcuts.rs
│   ├── active_window.rs
│   └── clipboard.rs
│
├── intelligence/
│   ├── mod.rs
│   └── transcription.rs
│
└── lib.rs
```

Frontend structure:

```text
src/
├── capture/
├── library/
├── search/
├── settings/
├── components/
└── lib/
```

---

# Context Detection Architecture

Project detection should use a provider-based design rather than assuming a single editor.

```text
ContextResolver
│
├── ShellProvider
├── VSCodeProvider
├── ForegroundWindowProvider
└── ManualProvider
```

### Shell Provider

A shell integration can expose the current working directory.

### Editor Provider

A small editor integration—starting with VS Code—can expose the active workspace.

### Foreground Window Provider

The OS can provide information about the previously focused application and window. This is weaker than explicit workspace integration but useful as a fallback.

### Manual Provider

If automatic detection fails, the user chooses or creates a context.

Once a project path is known, Lyn can locate the `.git` directory and determine the current branch.

---

# Key Product and Engineering Principles

## 1. Capture First

Every feature must protect the fundamental interaction:

```text
shortcut
→ capture
→ Enter
→ return to work
```

Anything that adds unnecessary friction to this path should be questioned.

## 2. Capture and Organization Are Separate

Do not put Library concepts into the quick-capture interface.

Avoid showing:

- complex tags,
- folders,
- filters,
- status selectors,
- rich formatting toolbars,
- branch selectors.

Those belong in the Library.

## 3. Local First

Core functionality should work without:

- internet access,
- accounts,
- cloud infrastructure,
- external AI APIs.

The user's notes and media remain local by default.

## 4. Lightweight by Design

Lyn should avoid dependencies or architectural choices that contradict its role as an always-available utility.

This is one of the reasons for choosing Tauri over Electron.

## 5. Intelligence Must Never Block Capture

AI and automation can enrich stored information, but they must not slow down the primary workflow.

**Save first. Enrich afterward.**

## 6. Manual Input Wins

When Lyn can generate metadata automatically but the user already provided it, the user's input should always win.

```text
manual screenshot caption
→ skip automatic caption
```

```text
manual voice-note caption
→ skip generated title
```

## 7. Context Should Be Useful, Not Excessive

For coding projects, the default automatic context should initially be:

```text
Project + Git branch
```

Avoid attaching excessive metadata such as every active file or task unless there is a clear future need.

## 8. Search Before AI Search

Normal full-text search through SQLite FTS5 should be the first solution.

Lyn does not need embeddings or a vector database merely because it stores developer context.

More sophisticated semantic search can be considered later if real product usage justifies it.

## 9. Keep v1 Focused

The first version should deliberately avoid unnecessary infrastructure.

Not required for v1:

```text
No web backend
No Firebase
No Supabase
No PostgreSQL
No accounts
No cloud sync
No vector database
No embeddings
No cloud LLM dependency
No OCR
No vision model
No heavy rich-text editor
No plugin ecosystem
```

The essential foundation is:

```text
Tauri
Rust
Svelte
SQLite
Filesystem
Audio
```

---

# Visual Identity

**Lyn should have its own visual identity.**

It should not simply inherit the appearance of a generic component library, Material dashboard, or web application.

Heavy UI frameworks should therefore be avoided unless they solve a genuinely difficult problem.

A lightweight design system can be built with:

```text
Svelte
CSS custom properties
small reusable components
headless primitives where needed
a consistent icon system
```

That will make it much easier to produce something that feels like:

> **Raycast × Linear × a tiny developer notebook**

From Raycast:

- speed,
- keyboard-first interaction,
- compact desktop utility feel.

From Linear:

- precision,
- clean hierarchy,
- restrained interface,
- polished interaction design.

From a tiny developer notebook:

- personal,
- calm,
- contextual,
- focused on thoughts rather than management overhead.

Lyn should feel like something a developer is comfortable leaving open all day, but rarely notices until it is needed.

---

# Long-Term Direction

Lyn starts as a fast local capture tool.

Its stored context can later become useful for coding-agent workflows.

Examples of future capabilities:

```text
Select captures
→ Copy as agent context
```

```text
Generate a clean context bundle for Claude / Codex / other agents
```

```text
Project context
├── relevant notes
├── screenshots
├── voice notes
├── decisions
└── prompt fragments
```

The important principle is that these agent features should be built **on top of the capture system**, not injected into the capture experience itself.

Lyn's value comes first from helping the user preserve context.

Agent collaboration becomes more powerful because that context already exists.

---

## Product Summary

Lyn is:

> **A fast, lightweight, local-first desktop working-memory companion for developers and focused computer work.**

It captures thoughts with almost no interruption, automatically associates them with useful project context, keeps them accessible through a chronological Library and search, supports text, screenshots, and voice, and creates a foundation for better collaboration with coding agents.

Its defining qualities should remain:

**Fast. Local. Lightweight. Minimal. Context-aware.**

