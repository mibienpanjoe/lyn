# Lyn — System Architecture

Version: v1.1, 2026-08-28

Derived from: [`04_transition_req_arch.md`](04_transition_req_arch.md)

Documentary status: **Proposed for v1 implementation.** This document defines the intended architecture; it does not claim that components are already implemented or verified.

## Architectural Style

Lyn is a **local modular monolith with a ports-and-adapters boundary** inside one Tauri desktop application.

The application has two runtime surfaces:

```text
Svelte WebView
    ↓ typed Tauri IPC
Rust application core
    ↓
SQLite / Lyn-owned files / narrow OS adapters
```

The Rust core is divided into cohesive domain services, but all services ship and run as one desktop unit. Platform integrations and optional speech recognition sit behind replaceable adapters.

This style is chosen because:

- the core workload is local and single-user;
- capture latency benefits from in-process coordination;
- a web backend or distributed system would contradict offline operation and add no MVP value;
- Tauri provides a narrow native bridge without bundling a browser runtime;
- Rust can own persistence and OS-sensitive capabilities while Svelte stays focused on interaction;
- provider and adapter interfaces isolate the genuinely variable parts: editor context, operating systems, audio devices, and speech engines.

Microservices, cloud services, frontend-owned SQL, and a generic plugin runtime are explicitly rejected for v1.

## Component Architecture

### Frontend Shell

**Responsibility:** Render the quick-capture popup, Library, search, previews, settings, and all recoverable UI states.

**Owned invariants:** None. Correctness is enforced in Rust.

**Inputs:** Typed command results and narrowly scoped application events.

**Outputs:** Validated user intent expressed as typed command payloads.

**Key behaviors:**

1. Focus the capture input when a prepared session is shown.
2. Map `Enter`, `Shift + Enter`, and `Esc` to the documented session operations while respecting input-method composition.
3. Preserve unsaved UI state after a command error.
4. Render Library pages incrementally and lazy-load media.
5. Present loading, empty, unavailable-media, permission, and retry states.
6. Let the user replace the proposed capture context without changing text, staged media, or recording state.
7. Render only safe live-source labels supplied by the Context Resolver.

**MUST NOT:** Execute SQL; construct arbitrary filesystem paths; invoke a shell; treat an optimistic UI transition as durable save; hide a storage error by closing the popup.

### Command Gateway

**Responsibility:** Provide the complete, typed Tauri IPC surface and reject malformed, stale, or over-privileged requests before delegation.

**Owned invariants:** INV-07 (local core privacy boundary), INV-13 (narrow, validated privilege boundary).

**Inputs:** Serialized command payloads from the Frontend Shell.

**Outputs:** Versioned result types, domain errors, and safe UI events.

**Key behaviors:**

1. Deserialize and validate command inputs.
2. Require domain identifiers rather than raw SQL or media paths.
3. Map internal errors to stable public error codes.
4. Expose no remote operation in the core capture, Library, media, or search surface.
5. Permit model download only through an explicit settings command with a constrained destination and artifact validation.

**MUST NOT:** Accept shell command strings, database statements, arbitrary URLs, arbitrary output paths, or generic filesystem operations from the WebView.

### Capture Service

**Responsibility:** Own the lifecycle and durable acceptance of one quick-capture session.

**Owned invariants:** INV-01, INV-10, INV-11, INV-12.

**Inputs:** Prepared context candidate or explicit context-source selection, user content, staged-media identifier, and session identifier.

**Outputs:** Active session state, saved capture identifier, cancellation result, or typed error.

**Key behaviors:**

1. Issue one active session identifier when the popup is invoked.
2. Preserve the resolved context candidate for display while allowing explicit correction or a required choice.
3. Validate that the session contains text or one supported staged media item.
4. Treat session ID as an idempotency key.
5. Ask Context Resolver to revalidate a selected live source and refresh its Git branch before persistence.
6. Coordinate Media Service finalization and Storage Service transaction.
7. Return success only when the canonical capture is durable.
8. Schedule eligible enrichment after success without waiting for it.

**MUST NOT:** Generate a text-note title, rewrite text, own SQL, accept a missing context, wait for transcription, or create two captures for one session.

### Context Resolver

**Responsibility:** Correlate invocation-bound provider evidence, maintain an ephemeral live-source registry, and produce exactly one project or standalone capture-time context snapshot.

**Owned invariants:** INV-04 (exactly one context), INV-05 (capture-time context snapshot), INV-14 (invocation-bound automatic context), INV-15 (explicit context correction authority).

**Inputs:** Pre-popup foreground identity, editor-window and integrated-terminal evidence, external-terminal and shell evidence, explicit selection, known contexts, and Git inspection results.

**Outputs:** Safe `ContextSourceOption` values, `ResolvedContext`, `ContextChoiceRequired`, `ContextSourceStale`, or a typed provider diagnostic.

**Key behaviors:**

1. Accept validated observations through a shared local provider trait and assign opaque source identifiers.
2. Correlate observations to the operating-system window that held focus immediately before Lyn appeared.
3. Normalize candidate paths without exposing raw correlation tokens, processes, terminal content, or editor content to the UI.
4. Locate the enclosing Git worktree, current named branch, and Git common directory when available.
5. Use the canonical Git common directory as stable project identity while retaining worktree and branch as capture metadata.
6. Produce safe labels for distinct live sources and combine them with saved contexts for the chooser.
7. Revalidate the selected live source and refresh its branch immediately before save.
8. Return an explicit-choice outcome when evidence is absent, ambiguous, or stale.

**Deterministic evidence order:** explicit selection for this capture → source exactly associated with the pre-popup foreground window → verified editor/process/terminal relation → recent foreground inference → unresolved. Configured provider order breaks ties only between evidence of equal quality.

**MUST NOT:** Use a global last-reported project as invocation context, guess between equally plausible sources, persist live observations, expose terminal commands/output or agent conversations, require Git for standalone contexts, silently substitute a stale source, or retain a live provider reference that can rewrite old captures.

### Storage Service

**Responsibility:** Own SQLite, schema migrations, canonical entities, transactions, settings, and application-data location policy.

**Owned invariants:** None directly; it supplies enforcement primitives to the owning services.

**Inputs:** Validated domain operations from Capture Service, Context Resolver, Library Service, Enrichment Service, and settings commands.

**Outputs:** Domain entities and transactional results.

**Key behaviors:**

1. Open the database in the operating system's Tauri application-data directory.
2. Apply ordered, versioned migrations before accepting commands.
3. Enable foreign-key enforcement for every connection.
4. Use parameterized `rusqlite` statements.
5. Store UTC timestamps and stable identifiers.
6. Commit capture, media metadata, and FTS projection consistently.
7. Provide a canonical-data scan for FTS rebuild and orphan reconciliation.

**MUST NOT:** Expose a SQL command through IPC, store screenshot/audio bytes as blobs, include a cloud-database client, or log user content.

### Media Service

**Responsibility:** Own the complete lifecycle of screenshot and audio bytes inside Lyn-managed storage.

**Owned invariants:** INV-08 (media referential integrity).

**Inputs:** Clipboard image bytes obtained through Platform Service, microphone sample streams, staged-media IDs, and committed media IDs.

**Outputs:** Safe preview handles, validated staged metadata, finalized media metadata, playback state, and external-open results.

**Key behaviors:**

1. Stage media under a session-scoped temporary identifier.
2. Encode screenshots as PNG and audio as WAV 16 kHz mono 16-bit PCM.
3. Compute byte size and a checksum before finalization.
4. Move completed media to a capture-ID-derived final location on the same application-data volume.
5. Validate media kind and path containment before read, playback, or external open.
6. Remove abandoned staged files and reconcile unreferenced final files after interrupted commits.

**MUST NOT:** Commit an incomplete file, follow a user-controlled path outside Lyn storage, interpolate a path into a shell command, substitute media when a file is missing, or delete committed media during session cleanup.

### Library Service

**Responsibility:** Provide chronological read models, bounded filters, capture detail, and FTS5 search.

**Owned invariants:** INV-06 (one project chronology), INV-09 (search is derived and bounded).

**Inputs:** Context, branch, type, date, query, and cursor filters.

**Outputs:** Paged capture summaries, capture detail, search results, and index-maintenance status.

**Key behaviors:**

1. Sort by `captured_at DESC, id DESC` for deterministic reverse chronology.
2. Scope project streams by context ID before applying optional branch filters.
3. Index only text bodies and media captions.
4. Escape or compile user queries into valid FTS expressions rather than accepting raw FTS syntax blindly.
5. Rebuild FTS entirely from canonical rows.
6. Return media identifiers, not filesystem paths.

**MUST NOT:** Treat a branch as a parent context, index raw transcript/OCR content in v1, mutate canonical capture content during search, or load all rows/media at once.

### Enrichment Service

**Responsibility:** Schedule and apply optional, asynchronous metadata enrichment to already-accepted captures.

**Owned invariants:** INV-02 (enrichment independence), INV-03 (user-authored metadata authority).

**Inputs:** Committed capture identifier, enrichment job, contextual metadata, optional transcript result, current caption source and revision.

**Outputs:** Generated caption update, skipped/failed/completed job status, or a discarded stale result.

**Key behaviors:**

1. Queue eligible work only after a capture is committed.
2. Derive screenshot captions only from existing metadata in v1.
3. Request transcription only when local intelligence is enabled and the compatible model is available.
4. Compare caption source and revision before applying a generated caption.
5. Bound retries and allow safe resumption after restart.

**MUST NOT:** Block Capture Service, overwrite a user caption, call a cloud AI service, require a vision model, or promote raw transcripts into the v1 search contract.

### Local Speech Adapter

**Responsibility:** Wrap optional whisper.cpp execution and local model lifecycle.

**Owned invariants:** None; Enrichment Service owns the behavioral guarantees.

**Inputs:** Lyn-owned WAV asset, installed-model identifier, cancellation signal.

**Outputs:** Local transcript, progress, or typed engine/model failure.

**Key behaviors:**

1. Verify model presence and compatibility before execution.
2. Process local audio without upload.
3. Honor cancellation and bounded resource policy.
4. Keep incomplete model downloads outside the active-model location until verified.

**MUST NOT:** Read arbitrary user paths, decide capture success, expose model internals to the UI, or silently fall back to a remote service.

### Platform Service

**Responsibility:** Provide narrow, target-specific implementations for operating-system capabilities.

**Owned invariants:** None; domain services own the guarantees supported by these adapters.

**Ports:** `ShortcutPort`, `WindowFocusPort`, `ForegroundIdentityPort`, `ClipboardPort`, `AudioInputPort`, `AudioPlaybackPort`, and `ExternalOpenPort`.

**Key behaviors:**

1. Register/unregister the configured global shortcut.
2. Record the foreground window identity before showing Lyn, then preserve it for return-to-work behavior.
3. Read supported clipboard content on explicit paste intent.
4. Provide minimal, opaque foreground-window correlation evidence.
5. Stream microphone samples through CPAL and play audio through Rodio where supported.
6. Open a Media Service-validated file through the native default application.

**MUST NOT:** Expose a generic platform-command escape hatch or collect continuous foreground-window history.

## Data Architecture

### Entity relationship model

```text
Context 1 ───────< Capture 1 ─────── 0..1 MediaAsset
                       │
                       ├──────────── 0..N EnrichmentJob
                       │
                       └──────────── 1 FTS projection (derived)

Setting                           SchemaMigration
```

### Canonical entities

#### `contexts`

| Field | Type | Rule |
|---|---|---|
| `id` | text UUID | Primary key; stable across restarts. |
| `kind` | enum | `project` or `standalone`. |
| `name` | text | Non-blank user-visible name. |
| `project_key` | text nullable | Stable local project identifier derived from the canonical Git common directory; null for standalone or non-Git contexts. |
| `project_path` | text nullable | Preferred local root for display/management, not project identity; null for standalone. Never exposed as a generic IPC path. |
| `created_at` | UTC timestamp | Immutable creation time. |
| `updated_at` | UTC timestamp | Last metadata change. |

Constraints:

- `kind = standalone` implies `project_key IS NULL AND project_path IS NULL`.
- A `project_key` is unique when present; multiple worktrees may resolve to that same context.
- Context deletion behavior is outside the MVP and is not defined by this contract.

### Ephemeral live-source registry

Context Resolver keeps validated `ContextSourceObservation` values in memory only. Each contains an opaque source ID, source kind/provider, internal window/process/session correlation tokens, an internal workspace or working directory, a safe display label, observation time, and liveness state. It never contains terminal commands/output, editor contents, or agent conversations. Observations expire and are never written to `contexts` or `captures`.

A coding agent has no special identity in this model: its context is the verified working directory of its owning terminal session. Worktrees sharing one Git common directory contribute to one project chronology; `branch_name` remains a per-capture snapshot.

#### `captures`

| Field | Type | Rule |
|---|---|---|
| `id` | text UUID | Primary key. |
| `session_id` | text UUID | Unique idempotency key; enforces INV-12. |
| `context_id` | text UUID | Required foreign key to `contexts`. |
| `kind` | enum | `text`, `image`, or `audio`. |
| `text_body` | text nullable | Required and non-blank only for `text`. |
| `caption` | text nullable | Optional only for image/audio. |
| `caption_source` | enum nullable | `user`, `context_generated`, `transcript_generated`, or null. |
| `caption_revision` | integer | Monotonic revision used to reject stale enrichment. |
| `branch_name` | text nullable | Named branch snapshot at capture time. |
| `source_app` | text nullable | Minimal optional context metadata. |
| `source_window_title` | text nullable | Minimal optional context metadata; not required. |
| `captured_at` | UTC timestamp | Capture acceptance time. |
| `updated_at` | UTC timestamp | Last allowed metadata update. |

Constraints:

- A text capture has non-blank `text_body`, no caption requirement, and no media asset.
- An image/audio capture has null `text_body` and exactly one matching media asset after commit.
- A non-blank user caption sets `caption_source = user`.
- `session_id` is unique.

#### `media_assets`

| Field | Type | Rule |
|---|---|---|
| `id` | text UUID | Primary key. |
| `capture_id` | text UUID | Unique foreign key to `captures`. |
| `kind` | enum | `image` or `audio`; must match capture kind. |
| `relative_path` | text | Lyn-managed relative path only. |
| `mime_type` | text | `image/png` or `audio/wav` in v1. |
| `byte_size` | integer | Positive. |
| `checksum` | text | Content checksum for integrity diagnostics. |
| `duration_ms` | integer nullable | Required for audio, null for image. |
| `width_px` | integer nullable | Required for image when decodable. |
| `height_px` | integer nullable | Required for image when decodable. |
| `created_at` | UTC timestamp | Finalization time. |

`relative_path` is unique and never accepted directly from the UI.

#### `enrichment_jobs`

| Field | Type | Rule |
|---|---|---|
| `id` | text UUID | Primary key. |
| `capture_id` | text UUID | Foreign key to an accepted capture. |
| `kind` | enum | `context_caption` or `speech_caption`. |
| `status` | enum | `pending`, `running`, `completed`, `skipped`, or `failed`. |
| `input_revision` | integer | Caption revision observed when scheduled. |
| `attempt_count` | integer | Bounded retry counter. |
| `last_error_code` | text nullable | Non-sensitive typed failure. |
| `created_at` / `updated_at` | UTC timestamp | Queue lifecycle. |

The queue is local and persistent so an accepted capture never depends on process lifetime.

#### `captures_fts`

An FTS5 virtual table containing `capture_id` as the external key and one normalized `search_text` projection:

- text body for `text`;
- caption for `image`;
- caption for `audio`.

It stores no unique canonical information and can be rebuilt from `captures`.

#### `settings` and `schema_migrations`

`settings` stores validated application settings such as shortcut, provider tie-break order, theme, and local-intelligence enablement. Secrets are not expected in v1. `schema_migrations` records monotonically applied migration versions.

### Media layout

```text
Lyn application data/
├── lyn.db
├── media/
│   ├── images/<capture-id>.png
│   ├── audio/<capture-id>.wav
│   └── staging/<session-id>/...
└── models/
    └── speech/<model-id>/...
```

The exact base directory is selected through the platform's Tauri application-data API. Persisted database paths are relative to that base.

### Media commit protocol

SQLite and the filesystem do not share a native transaction, so Media Service and Capture Service use a recoverable protocol:

1. Write and close the complete staged file.
2. Validate format and compute metadata/checksum.
3. Begin the SQLite capture transaction.
4. Atomically rename the staged file to its final path on the same volume.
5. Insert capture and media rows plus FTS projection.
6. Commit SQLite.
7. If steps 5–6 fail, remove the unreferenced final file when possible.
8. On startup, reconcile stale staging files and unreferenced final files without touching referenced assets.

Success is returned only after step 6.

## Flow Architecture

### Application startup

```text
Tauri startup
  → Storage Service opens application data
  → apply transactional migrations
  → Media Service performs scoped orphan reconciliation
  → register configured global shortcut
  → resume eligible local enrichment jobs
  → remain available in background
```

If storage initialization fails, Lyn may show diagnostics and settings but MUST NOT accept captures deceptively.

### Quick invocation and context preparation

```text
Global shortcut
  → Platform Service records the previously focused OS window
  → Capture Service creates or restores one active session
  → Platform Service shows/focuses popup; input becomes usable
  → Context Resolver correlates live observations to that window
  → Frontend Shell receives resolved, ambiguous, or required context state
```

Context preparation may continue briefly after the input becomes usable. Saving waits only for required manual context resolution, not for optional metadata enrichment.

### Context correction

```text
Open context control
  → Context Resolver lists safe live-source options + saved contexts
  → user selects one option
  → Capture Service preserves text, staged media, and recording state
  → selected live source is revalidated and branch metadata refreshed
  → context candidate updates for this capture only
```

If the source disappears or changes identity, the command returns `CONTEXT_SOURCE_STALE`; the draft remains intact and Lyn asks for another selection. Choosing a source never changes the default for later captures.

### Text save

```text
Enter
  → Command Gateway validates SaveTextCapture
  → Capture Service revalidates any selected live source
  → Capture Service validates session, context snapshot, and non-blank body
  → Storage Service transaction inserts capture + FTS projection
  → Capture Service returns capture ID
  → popup closes and focus returns to prior application
```

### Screenshot save

```text
Paste image
  → Platform Service reads clipboard image
  → Media Service stages PNG + returns preview handle
  → user optionally enters caption
  → Capture Service finalizes media and commits image capture
  → popup closes
  → [optional, post-commit] Enrichment Service derives context caption
```

The enrichment branch is not scheduled when the saved caption source is `user`.

### Voice save and transcription

```text
Record
  → Platform Service/CPAL streams microphone samples
  → Media Service stages WAV
Stop → playback/caption UI
Enter
  → Capture Service finalizes media and commits audio capture
  → popup closes
  → [optional, post-commit] Enrichment Service
       → Local Speech Adapter/whisper.cpp
       → compare caption revision/source
       → apply derived caption or discard stale result
```

### Project Library and branch filter

```text
Open project
  → Library Service queries captures WHERE context_id = project
  → optional branch predicate is applied
  → page ordered by captured_at DESC, id DESC
  → Frontend Shell lazy-loads visible media
```

### Search

```text
Search input
  → Command Gateway validates query and filters
  → Library Service compiles safe FTS expression
  → FTS5 returns capture IDs and match metadata
  → canonical capture summaries are loaded
  → Frontend Shell renders paged results
```

### Provisional latency budgets

| Flow segment | Target | Notes |
|---|---|---|
| Shortcut event to input-ready | p95 ≤ 250 ms | Agreed reference machine; context may complete after input-ready. |
| Text save command to durable result | p95 ≤ 150 ms | Warm database; no enrichment in path. |
| Search over 10,000 captures | p95 ≤ 200 ms | First bounded result page. |
| Popup dismiss transition | ≤ 150 ms | Must not postpone durable result; reduced motion can make it immediate. |

## Technology Mapping

| Technology | Architectural use |
|---|---|
| Tauri 2 | Desktop lifecycle, windows, capabilities, global shortcut integration, and Command Gateway transport. |
| Rust | All core services, validation, context/Git inspection, storage, media, platform adapters, and optional intelligence coordination. |
| Svelte 5 + TypeScript + Vite | Frontend Shell and typed IPC client bindings. |
| SQLite + `rusqlite` | Storage Service canonical data, migrations, settings, and transactions. |
| SQLite FTS5 | Library Service full-text projection and search. |
| Local filesystem | Media Service screenshots/audio and Local Speech Adapter models. |
| CPAL | Platform Service microphone/device stream adapter. |
| Rodio | Platform Service audio playback adapter where suitable. |
| whisper.cpp | Optional Local Speech Adapter implementation. |

No technology in the table authorizes cloud storage, cloud AI, or direct frontend access to local resources.

## Deployment Architecture

Lyn is packaged as one native desktop application per supported operating system:

```text
Installed Lyn application
├── Tauri/Rust binary
├── bundled Svelte frontend assets
└── platform capabilities and permissions

Per-user local application data
├── SQLite database
├── screenshots
├── voice notes
└── optional speech model
```

There is no server deployment, web database, account service, or mandatory model service. A speech model may be acquired only through an explicit user action and then executes locally.

The overview does not establish the first supported operating system. Release documentation must name each actual target only after shortcut, focus, clipboard, audio, storage, and external-open acceptance tests pass on that target.

## Project Structure

```text
src/
├── capture/
│   ├── components/
│   ├── capture-state.ts
│   └── capture-client.ts
├── library/
│   ├── components/
│   ├── library-state.ts
│   └── library-client.ts
├── search/
├── settings/
├── components/
└── lib/
    ├── ipc-types.ts
    └── ui/

src-tauri/src/
├── commands/             # Command Gateway only
├── capture/              # Capture Service
├── context/              # Context Resolver and providers
│   ├── session_registry.rs
│   ├── shell.rs
│   ├── vscode.rs
│   ├── foreground.rs
│   ├── manual.rs
│   ├── git.rs
│   └── worktree.rs
├── storage/              # Storage Service
│   ├── db.rs
│   ├── migrations.rs
│   ├── captures.rs
│   ├── contexts.rs
│   └── settings.rs
├── media/                # Media Service
│   ├── images.rs
│   ├── audio.rs
│   └── staging.rs
├── library/              # Library Service and FTS projection
├── enrichment/           # Enrichment Service
├── intelligence/         # Local Speech Adapter
├── platform/             # Narrow OS ports/adapters
├── error.rs
└── lib.rs
```

Frontend feature folders contain presentation and command clients, not business rules. Rust service modules expose domain operations to `commands/`; they do not annotate every internal function as a Tauri command.

## Requirements Traceability Matrix

| Requirement IDs | Architecture enforcement | Verification level |
|---|---|---|
| FR-010, FR-011, FR-012, FR-013, FR-014, FR-015, FR-016, FR-017, FR-018, FR-019 | Capture Service + Platform Service + focus-aware Frontend Shell | Desktop integration and keyboard tests |
| FR-020, FR-021, FR-022, FR-023, FR-024, FR-025, FR-026, FR-027, FR-028, FR-029 | Context Resolver provider trait, invocation-bound evidence ranking, Git adapter | Provider unit tests and real-worktree integration tests |
| FR-030, FR-031, FR-032, FR-033, FR-034 | Titleless Capture Service text command and Storage constraints | Round-trip and validation tests |
| FR-040, FR-041, FR-042, FR-043, FR-044, FR-045, FR-046, FR-047, FR-048, FR-049 | Clipboard adapter, PNG staging/finalization, optional Enrichment Service | Media integration tests |
| FR-050, FR-051, FR-052, FR-053, FR-054, FR-055, FR-056, FR-057, FR-058, FR-059 | CPAL/Rodio adapters, WAV encoder, post-commit speech adapter | Device-contract and file-format tests |
| FR-060, FR-061, FR-062, FR-063, FR-064, FR-065, FR-066, FR-067 | Library Service paged chronological read model | Query and UI-state tests |
| FR-070, FR-071, FR-072, FR-073, FR-074, FR-075, FR-076 | Bounded FTS5 projection and safe query compilation | Search correctness/performance tests |
| FR-080, FR-081, FR-082, FR-083, FR-084, FR-085, FR-086, FR-087 | Settings persistence and isolated model lifecycle | Offline and configuration tests |
| FR-090, FR-091, FR-092, FR-093, FR-094, FR-095, FR-096, FR-097, FR-098, FR-099 | Rust-only SQLite/filesystem ownership and typed gateway | Boundary, migration, and recovery tests |
| FR-100, FR-101, FR-102, FR-103, FR-104, FR-105, FR-106, FR-107, FR-108, FR-109 | Foreground identity port, ephemeral source registry, chooser commands, save-time revalidation, Git common-directory identity | Concurrent-window, stale-source, privacy, and draft-preservation tests |

## Invariant Traceability Matrix

| Invariant | Owner | Concrete enforcement mechanism |
|---|---|---|
| INV-01 Durable acceptance | Capture Service | Success response after finalized media and committed SQLite transaction only. |
| INV-02 Enrichment independence | Enrichment Service | Post-commit persistent jobs; no enrichment future in save response. |
| INV-03 User-authored metadata authority | Enrichment Service | `caption_source` + monotonic `caption_revision` compare-and-set. |
| INV-04 Exactly one context | Context Resolver | Resolved result type plus required `captures.context_id` foreign key. |
| INV-05 Capture-time context snapshot | Context Resolver | Save-time context value with stored nullable `branch_name`. |
| INV-06 One project chronology | Library Service | Ownership query by `context_id`; branch only as predicate. |
| INV-07 Local core privacy boundary | Command Gateway | Core command allowlist has no remote target; offline acceptance tests. |
| INV-08 Media referential integrity | Media Service | Same-volume finalization, unique media relation, containment check, startup reconciliation. |
| INV-09 Search is derived and bounded | Library Service | FTS projection from allowed fields and full rebuild from canonical rows. |
| INV-10 Text fidelity and titlelessness | Capture Service | Text input maps directly to body; no title field or rewrite path. |
| INV-11 Cancellation non-persistence | Capture Service | Session-scoped cancellation and Media Service staging cleanup. |
| INV-12 Single-session, single-save semantics | Capture Service | One active session and unique `captures.session_id`. |
| INV-13 Narrow, validated privilege boundary | Command Gateway | Typed domain commands, Tauri capability allowlist, media IDs instead of paths. |
| INV-14 Invocation-bound automatic context | Context Resolver | Pre-popup foreground identity and evidence-quality ranking; provider order breaks ties only. |
| INV-15 Explicit context correction authority | Context Resolver | Current-capture selection overrides inference; Capture Service preserves the draft while revalidation fails safely. |

## Architectural Decisions

These decisions are accepted by the source overview or subsequent recorded product decisions unless marked provisional.

### ADR-001 — Tauri 2 instead of Electron

**Status:** Accepted.

Use Tauri 2 to support a lightweight always-on desktop utility with native access and the operating system WebView. Electron is rejected because bundling Chromium conflicts with Lyn's lightweight background role.

### ADR-002 — Rust owns the application core

**Status:** Accepted.

Storage, context resolution, media, platform integration, and optional local intelligence live in Rust. Svelte owns presentation. Direct frontend SQL or filesystem logic is rejected to keep correctness and privilege centralized.

### ADR-003 — SQLite for canonical data; filesystem for media

**Status:** Accepted.

Use `rusqlite` with SQLite for structured local state and use Lyn-owned files for PNG/WAV bytes. SQLite blobs and remote databases are rejected for the MVP.

### ADR-004 — FTS5 before semantic search

**Status:** Accepted.

Search text bodies and captions with SQLite FTS5. OCR, embeddings, vector databases, and semantic search remain outside v1 until usage demonstrates a need.

### ADR-005 — Provider-based context resolution

**Status:** Accepted.

Shell, VS Code, foreground-window, and manual sources implement a common provider contract. Evidence associated with the pre-popup foreground window outranks unrelated recent reports; provider order is only a same-confidence tie-breaker.

### ADR-006 — Enrichment is post-commit and optional

**Status:** Accepted.

Screenshot metadata and voice transcription run only after durable save. whisper.cpp is optional and locally installed. A cloud fallback is prohibited by the v1 privacy boundary.

### ADR-007 — Typed Tauri IPC is the public application interface

**Status:** Proposed.

Treat the IPC surface in [`06_api_specification.md`](06_api_specification.md) as a versioned internal public contract. This reduces UI/core drift, limits privileges, and permits contract testing. Exact command names may change only through coordinated specification and binding updates.

### ADR-008 — Invocation-bound live-session context selection

**Status:** Accepted, 2026-08-28.

Lyn records the OS window active before capture, correlates it with validated editor/terminal observations, and offers safe live-session choices when inference is wrong or ambiguous. Explicit selection applies only to the current capture and cannot discard draft content. A global “last active project,” persistent live-session records, and exposure of terminal/editor/agent content are rejected because they become incorrect and invasive under concurrent windows and agents.
