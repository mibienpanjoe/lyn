# Lyn — Software Requirements Specification

Version: v1.1, 2026-08-28

Derived from: [`01_requirements_prd.md`](01_requirements_prd.md)

## Normative Vocabulary

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHOULD**, **SHOULD NOT**, and **MAY** are normative:

- **MUST / MUST NOT / REQUIRED**: an absolute product or system requirement;
- **SHOULD / SHOULD NOT**: the expected behavior unless a documented, reviewed exception exists;
- **MAY**: an optional capability that cannot be assumed by callers.

This specification defines the intended MVP. Performance figures marked **provisional** are acceptance targets proposed by the PRD and require validation on an agreed reference machine.

## Actors

| Actor | Description |
|---|---|
| User | Person invoking Lyn, creating captures, browsing the Library, searching, and changing settings. |
| Operating System | Provides global shortcuts, window lifecycle, clipboard, active-window information, filesystem access, default media viewers, and microphone permissions. |
| Context Provider | Supplies a possible working directory or workspace from a shell, VS Code integration, foreground window, or manual selection. |
| Git Repository | Supplies repository-root and current-branch information for a detected project path. |
| Local Speech Engine | Optional on-device speech recognition runtime and model. It is absent or disabled by default. |

## Functional Requirements

### FR-010 — Invocation and capture lifecycle

- **FR-011**: Lyn MUST register a user-configurable global shortcut on every supported target operating system.
- **FR-012**: Invoking the shortcut MUST show one capture popup; repeated shortcut events MUST NOT create concurrent capture windows.
- **FR-013**: The primary text input MUST be ready to receive keyboard input as soon as the popup is presented.
- **FR-014**: Pressing `Enter` outside an active input-method composition MUST request save and, after durable acceptance, close the popup.
- **FR-015**: Pressing `Shift + Enter` MUST insert a newline and MUST NOT save the capture.
- **FR-016**: Pressing `Esc` MUST cancel the current session, remove session-owned staged media, and close the popup without creating a capture.
- **FR-017**: The popup MUST display the resolved context and Git branch subtly when they are available.
- **FR-018**: Text entry MUST NOT require the user to select a capture type or provide a title.
- **FR-019**: A save failure MUST leave the popup and entered content available for retry; Lyn MUST NOT close as if the capture succeeded.

### FR-020 — Context resolution

- **FR-021**: Lyn MUST implement context detection as a provider set with deterministic evidence ranking rather than as editor-specific core logic.
- **FR-022**: The initial provider set MUST support shell, VS Code workspace, foreground-window, and manual context sources.
- **FR-023**: Given a filesystem path inside a Git worktree, Lyn MUST resolve the worktree root, use the canonical Git common directory as stable project identity, and read the current named Git branch when available.
- **FR-024**: Context resolution MUST be deterministic for the same invocation-bound evidence. Evidence quality and exact foreground association MUST outrank provider recency; configured provider order MAY break ties only between equally reliable candidates.
- **FR-025**: If no project or standalone context is resolved, Lyn MUST require the user to select an existing context or create one before saving.
- **FR-026**: Lyn MUST support standalone contexts with no filesystem path or Git repository.
- **FR-027**: Each saved capture MUST retain a snapshot of its context identity and branch value at capture time.
- **FR-028**: A project MUST expose one capture history; branches MUST be filters or metadata and MUST NOT create separate project libraries.
- **FR-029**: Failure to read Git metadata MUST NOT prevent capture when a valid project or standalone context is otherwise known.

### FR-030 — Text capture

- **FR-031**: Lyn MUST accept non-empty plain text as a text capture.
- **FR-032**: A text capture MUST remain titleless; its body is its primary display content.
- **FR-033**: Lyn MUST preserve the user's Unicode text and line breaks without automatic rewriting, summarization, or title generation.
- **FR-034**: Lyn MUST reject a capture session that contains no text, screenshot, or recorded audio.

### FR-040 — Screenshot capture

- **FR-041**: Pasting image data into the normal capture surface MUST create an image preview without requiring a mode switch.
- **FR-042**: The user MUST be able to enter an optional screenshot caption before saving.
- **FR-043**: A saved screenshot MUST be copied into Lyn-managed local storage as a PNG file; an external source path MUST NOT be the only retained copy.
- **FR-044**: The Library MUST support inline screenshot preview.
- **FR-045**: The user MUST be able to open a screenshot from Lyn and with the operating system's default image viewer.
- **FR-046**: If the user supplied a non-blank caption, Lyn MUST store it unchanged and MUST NOT replace it with generated text.
- **FR-047**: If the caption is blank, Lyn MAY derive a caption from existing metadata such as context, branch, application, window title, or filename.
- **FR-048**: MVP screenshot enrichment MUST NOT require OCR, a vision model, or network access.
- **FR-049**: Screenshot caption enrichment MUST NOT delay durable save or popup dismissal.

### FR-050 — Voice capture

- **FR-051**: The user MUST be able to start and stop microphone recording within the capture popup.
- **FR-052**: Stopping a recording MUST expose playback and optional-caption controls in the same popup.
- **FR-053**: Saved MVP voice notes MUST use WAV, 16 kHz, mono, 16-bit PCM unless an ADR explicitly supersedes this contract.
- **FR-054**: A saved audio file MUST be held in Lyn-managed local storage.
- **FR-055**: If the user supplied a non-blank caption, Lyn MUST store it unchanged and MUST NOT generate or apply a replacement title.
- **FR-056**: If the caption is blank and local speech recognition is enabled, Lyn MAY transcribe the audio and derive a short local caption.
- **FR-057**: Voice notes MUST remain savable when the speech engine or model is absent, disabled, busy, or failing.
- **FR-058**: Transcription and derived-caption work MUST run after capture acceptance and MUST NOT delay saving.
- **FR-059**: Playback MUST use the saved or staged local audio and MUST NOT upload it.

### FR-060 — Library

- **FR-061**: The Library MUST provide Recent, All captures, Projects, Standalone contexts, and Search entry points.
- **FR-062**: Capture lists MUST default to reverse chronological order and display a stable timestamp.
- **FR-063**: Each Library item MUST indicate its capture type and context; it SHOULD show branch metadata when present.
- **FR-064**: A project view MUST combine captures from all of its branches into one stream.
- **FR-065**: The Library MUST render text bodies, screenshot previews, and playable voice notes.
- **FR-066**: A user MUST be able to inspect an individual capture and its stored metadata.
- **FR-067**: Organization controls MUST remain in the Library and MUST NOT be added to the default quick-capture path.

### FR-070 — Search

- **FR-071**: Lyn MUST provide local full-text search backed by SQLite FTS5.
- **FR-072**: The search index MUST include text-capture bodies, screenshot captions, and voice-note captions.
- **FR-073**: MVP search MUST NOT depend on OCR output, raw voice transcripts, embeddings, vector search, or a remote service.
- **FR-074**: Search results MUST identify the matching capture, context, capture type, timestamp, and branch when present.
- **FR-075**: Search SHOULD support filters for context, project, branch, date range, and capture type without changing the underlying single-stream model.
- **FR-076**: Empty or whitespace-only queries MUST return a defined recent/all view or an empty result according to the UI call; they MUST NOT execute malformed FTS syntax.

### FR-080 — Settings and optional local intelligence

- **FR-081**: Core capture, Library, media, and search features MUST work without an account or internet connection.
- **FR-082**: Local speech recognition MUST be opt-in.
- **FR-083**: Lyn MAY offer download and removal of a compatible local speech model through settings.
- **FR-084**: Model availability, download progress, and failure MUST be visible to the user.
- **FR-085**: Disabling local intelligence MUST stop new transcription work without changing or deleting existing captures.
- **FR-086**: A model download failure MUST NOT affect any core feature.
- **FR-087**: Lyn MUST provide settings for the global shortcut and provider tie-break order when those settings are supported by the target platform. A preference MUST NOT override stronger invocation-bound evidence.

### FR-090 — Persistence and data ownership

- **FR-091**: Structured application data MUST be stored in a local SQLite database accessed by the Rust core through `rusqlite`.
- **FR-092**: The frontend MUST NOT execute SQL directly.
- **FR-093**: The frontend MUST access core capabilities through typed Tauri commands and narrowly scoped events.
- **FR-094**: Screenshot and audio bytes MUST be stored as files; SQLite MUST store their metadata and Lyn-owned paths rather than binary blobs.
- **FR-095**: A capture and its required media metadata MUST become visible atomically or not at all.
- **FR-096**: Persistent identifiers MUST remain stable across restarts.
- **FR-097**: Timestamps MUST be stored in an unambiguous machine-readable form and rendered in the user's local timezone.
- **FR-098**: Lyn MUST maintain its FTS index consistently with capture creation and caption updates.
- **FR-099**: Core operation MUST NOT send capture content or metadata to a remote service.

### FR-100 — Concurrent work-session context

- **FR-101**: Platform Service MUST record the previously focused OS window identity before showing or focusing the Lyn capture popup.
- **FR-102**: Each live editor or shell observation MUST have an opaque source ID and enough local correlation data to distinguish its application instance, window or terminal session, workspace or working directory, observation time, and liveness without exposing raw correlation data to the frontend.
- **FR-103**: When a live observation is bound to the previously focused window or its verified process/session relationship, Lyn MUST prefer it over an unrelated provider that merely reported more recently.
- **FR-104**: Lyn MUST represent concurrently open VS Code windows, integrated terminals, external terminal sessions, and shell sessions as distinct selectable sources when they can be reliably distinguished.
- **FR-105**: The capture context control MUST list eligible live sources and saved project or standalone contexts using safe application, context/worktree, and branch labels.
- **FR-106**: Selecting another live source or saved context MUST preserve all draft text, captions, staged screenshots, staged audio, and recording state, and MUST override automatic detection for the current capture only.
- **FR-107**: Lyn MUST revalidate a selected live source and refresh its workspace or working directory and named branch before save. A dead, stale, or no-longer-correlatable source MUST require reselection without discarding the draft.
- **FR-108**: An integrated terminal source MUST be correlated through its owning editor window and active terminal session when available. An external terminal with multiple tabs MUST expose the active session through a supported integration; otherwise Lyn MUST treat the evidence as ambiguous.
- **FR-109**: A coding agent's context MUST derive from its shell working directory rather than its agent identity. Git worktrees sharing one Git common directory MUST map to one project context while retaining the selected worktree's named branch as capture metadata.

## Business Rules

- **BR-01 — Capture first:** Any optional behavior that would delay capture acceptance MUST be deferred until after save.
- **BR-02 — Manual input wins:** A non-blank user caption is authoritative and MUST NOT be overwritten by generated metadata.
- **BR-03 — Context required:** Every capture belongs to exactly one project or standalone context.
- **BR-04 — Branch as metadata:** A branch describes the capture-time context; it does not own a Library.
- **BR-05 — Titleless text:** A text note has a body, not a generated or required title.
- **BR-06 — Local by default:** Core storage, retrieval, and media behavior stays on the device.
- **BR-07 — Capture and organization separation:** Quick capture contains only controls needed to create the current capture and resolve or correct its context.
- **BR-08 — No silent false context:** If evidence is insufficient, manual selection is preferable to an unverified automatic assignment.
- **BR-09 — Invocation-bound detection:** The window/session that triggered capture is more authoritative than a global last-reported project.
- **BR-10 — Explicit context selection wins:** A user-selected source is authoritative for the current capture and never mutates the draft payload.

## Non-Functional Constraints

### Performance

The following thresholds are provisional acceptance targets from the PRD:

- The capture input SHOULD be ready within 250 ms at p95 after shortcut activation on the agreed reference machine.
- A valid text capture SHOULD be durably accepted within 150 ms at p95 after save is requested, excluding first-run database initialization.
- A query over 10,000 captures SHOULD return the first result set within 200 ms at p95.
- Optional enrichment MUST consume bounded background resources and SHOULD yield to active capture, playback, and Library work.

### Availability and resilience

- Lyn MUST remain usable offline for all core features.
- A failure in one optional provider or enrichment subsystem MUST NOT disable text capture.
- Interrupted media writes MUST NOT produce Library entries that appear valid but reference incomplete files.
- Database migrations MUST be transactional where SQLite permits and MUST fail without silently discarding existing user data.

### Security

- Tauri permissions and capabilities MUST grant only the OS and filesystem access required by each window and command.
- User-controlled text, filenames, window titles, and paths MUST be treated as untrusted input.
- Database queries MUST be parameterized.
- Paths returned across IPC MUST NOT permit arbitrary traversal outside Lyn-managed media or an explicitly user-selected file.
- Opening external media MUST pass a validated Lyn-owned path to the operating system without shell-string interpolation.
- Microphone access MUST follow operating-system permission controls and MUST begin only after an explicit user action.

### Data privacy

- Capture bodies, captions, context metadata, screenshots, and audio MUST remain local during core operation.
- Optional local transcription MUST process audio on the device.
- Lyn MUST NOT introduce telemetry containing capture content in the MVP.
- Logs MUST NOT contain capture bodies, transcripts, raw clipboard content, or audio bytes.
- Live-source observations and diagnostics MUST NOT contain terminal commands, terminal output, editor contents, coding-agent conversations, or continuously retained activity history.

### Data integrity

- Each committed capture MUST reference an existing context.
- Each image or audio capture MUST reference exactly one valid media asset.
- A text capture MUST contain non-blank text and MUST NOT require a media asset.
- Deleting staged media after cancellation or failure MUST NOT delete a committed asset belonging to another capture.
- Search-index updates MUST be repeatable and recoverable from canonical capture records.

### Scalability

- The MVP MUST support at least 10,000 captures in one local profile without changing storage technology.
- Listing MUST use bounded pages or cursors rather than loading every capture and media preview at once.
- Media thumbnails or waveform data SHOULD be loaded lazily.

### Portability

- The architecture MUST isolate platform-specific shortcut, active-window, clipboard, filesystem-open, and audio behavior behind adapters.
- The set of supported operating systems is not established by the overview. Each declared release target MUST pass the same behavioral contract before being described as supported.
- Platform limitations MUST degrade to manual context or unavailable optional controls rather than corrupting captures.
- If a platform cannot reliably associate the foreground window with an active editor/terminal tab, it MUST return ambiguous evidence rather than choose a global recent session.

### Accessibility and interaction

- All capture actions MUST be keyboard accessible.
- Focus MUST remain visible and deterministic in the popup and Library.
- Icon-only controls MUST have accessible names.
- Color MUST NOT be the only indicator of capture type, state, error, or selection.
- Motion MUST respect `prefers-reduced-motion` and MUST NOT delay keyboard interaction.

## Error Cases

| ID | Trigger | Required behavior |
|---|---|---|
| ERR-001 | Global shortcut cannot be registered or conflicts with another application | Keep Lyn running, identify the shortcut conflict in settings, and allow assignment of another shortcut. |
| ERR-002 | Popup cannot obtain focus on first attempt | Retry through the platform adapter, keep the popup visible, and allow pointer focus; do not discard the session. |
| ERR-003 | No context can be resolved | Require selection or creation of a context before save; preserve entered content while the user chooses. |
| ERR-004 | Git metadata cannot be read or the repository has no named branch | Save under the resolved project with a null branch and retain a diagnostic status that does not expose private paths in logs. |
| ERR-005 | SQLite rejects or cannot commit a capture | Keep the popup open with all recoverable input, report `STORAGE_WRITE_FAILED`, and do not report success. |
| ERR-006 | Screenshot staging, PNG encoding, or final media move fails | Do not commit the image capture; preserve the preview when possible and allow retry or cancellation. |
| ERR-007 | Clipboard content is not supported image or text data | Leave the current capture unchanged and show a non-blocking `UNSUPPORTED_CLIPBOARD_CONTENT` message. |
| ERR-008 | Microphone permission is denied or no input device is available | Do not start recording; keep text and screenshot capture available and link to relevant settings when the OS permits. |
| ERR-009 | Audio stream fails during recording | Stop recording, mark the staged recording unusable, retain any entered caption, and allow a new recording or another capture type. |
| ERR-010 | Local playback fails | Keep the staged or saved audio unchanged and report `AUDIO_PLAYBACK_FAILED`. |
| ERR-011 | Speech engine/model is absent, disabled, times out, or fails | Save remains successful; mark enrichment skipped or failed without replacing a manual caption. |
| ERR-012 | Model download is interrupted or invalid | Remove only the incomplete staged model, retain any previously valid model, and report the failure in settings. |
| ERR-013 | FTS query is invalid or the index is unavailable | Return a typed search error without modifying canonical captures; allow index rebuild from canonical data. |
| ERR-014 | A committed media file is missing or unreadable | Render an explicit unavailable-media state, keep metadata visible, and never substitute another file. |
| ERR-015 | An IPC payload is invalid or stale | Reject it with `VALIDATION_ERROR` or `STALE_SESSION`; do not partially mutate storage. |
| ERR-016 | Application exits while enrichment is pending | Preserve the accepted capture; enrichment MAY resume safely on the next launch. |
| ERR-017 | Multiple live sources are equally plausible or none is bound reliably to the pre-popup foreground window | Return `CONTEXT_AMBIGUOUS`, preserve the draft, and open or make available the context chooser. |
| ERR-018 | A user-selected live source has exited, become stale, or can no longer be correlated before save | Return `CONTEXT_SOURCE_STALE`, preserve all draft/staged content, refresh the source list, and require another selection. |
| ERR-019 | A provider submits malformed observation data or its local registration channel is unavailable | Reject or ignore that observation, continue with other providers, and retain manual context selection; core capture remains available. |

## Verification Basis

Each `FR-*`, `BR-*`, and `ERR-*` statement is intended to map to an automated Rust, frontend, IPC-contract, or integration test. Provisional timing thresholds additionally require measurement on a documented reference machine and target operating system.
