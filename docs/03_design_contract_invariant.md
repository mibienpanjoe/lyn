# Lyn — System Contract & Invariants

Version: v1.0, 2026-08-27

Derived from: [`02_requirements_srs.md`](02_requirements_srs.md)

## Actors and Allowed Actions

| Actor | Permitted actions |
|---|---|
| User | Invoke and cancel capture; enter text; paste images; record and play audio; select context; browse and search; open media; change settings; enable optional local intelligence. |
| Capture UI | Render session state; collect user intent; call typed core commands; display returned errors. It may not write the database or application-data filesystem directly. |
| Rust Core | Validate commands; coordinate capture, context, storage, media, search, settings, and enrichment; expose narrowly scoped results to the UI. |
| Context Provider | Return evidence about a shell directory, editor workspace, foreground window, or manual selection. It may not persist a capture. |
| Platform Adapter | Access only the OS capability assigned to it: shortcut, focus, clipboard, active-window metadata, external-open, filesystem, or microphone. |
| Local Speech Engine | Read a Lyn-owned audio asset when enabled; produce a local transcript or derived caption; report status. It may not determine whether the capture can be saved. |

No actor may bypass the Rust command boundary to mutate canonical capture data. No optional actor may become a prerequisite for text, screenshot, or voice capture.

## System Guarantees

### Capture integrity

**INV-01 — Durable acceptance**

A capture acknowledged as saved MUST already have a committed canonical database record and, for an image or audio capture, a committed reference to its finalized Lyn-owned media asset. The popup MUST NOT close as if saving succeeded before this condition holds.

Protects: FR-014, FR-019, FR-095.

**INV-02 — Enrichment independence**

Caption generation, speech transcription, model availability, and all future enrichment MUST remain outside the synchronous acceptance condition. Their absence, delay, or failure MUST NOT cause an otherwise valid capture to fail or remain blocked.

Protects: FR-049, FR-057, FR-058, BR-01.

**INV-03 — User-authored metadata authority**

A non-blank caption supplied by the user is authoritative. Automatic enrichment MUST NOT overwrite, replace, or race it into a different value.

Protects: FR-046, FR-055, BR-02.

**INV-10 — Text fidelity and titlelessness**

The canonical body of a text capture MUST preserve the user's Unicode text and line breaks. It MUST NOT be rewritten, summarized, or displaced by an automatically generated title.

Protects: FR-032, FR-033, BR-05.

**INV-11 — Cancellation non-persistence**

Cancelling a capture session MUST leave no canonical capture, search row, or session-owned staged media. Cleanup MUST be scoped to that session and MUST NOT remove committed media.

Protects: FR-016.

**INV-12 — Single-session, single-save semantics**

At most one quick-capture session may be active per application instance, and one session identifier may create at most one canonical capture. Repeated shortcuts, key repeat, retries, or duplicate IPC delivery MUST NOT create duplicate captures.

Protects: FR-012 and capture-attempt integrity in the PRD.

### Context integrity

**INV-04 — Exactly one context**

Every committed capture MUST belong to exactly one existing project or standalone context. A capture with no context or multiple contexts is invalid.

Protects: FR-025, FR-026, BR-03.

**INV-05 — Capture-time context snapshot**

The context identity and branch stored on a capture describe the moment of capture. Later branch changes, provider updates, project renames, or missing Git data MUST NOT silently rewrite that historical snapshot.

Protects: FR-027, BR-04.

**INV-06 — One project chronology**

All captures belonging to one project MUST remain queryable as one chronological stream regardless of branch. Branch filtering MAY narrow the stream but MUST NOT redefine ownership or create branch-specific libraries.

Protects: FR-028, FR-064, BR-04.

### Local data and retrieval integrity

**INV-07 — Local core privacy boundary**

Core capture content and metadata MUST remain on the device. Core saving, browsing, media access, and search MUST neither require nor perform a remote request. Optional speech processing MUST be local.

Protects: FR-081, FR-099, BR-06.

**INV-08 — Media referential integrity**

Every committed image or audio capture MUST resolve to exactly one matching, complete Lyn-owned media asset. A missing, partial, external-only, cross-linked, or type-mismatched asset is a contract violation.

Protects: FR-043, FR-054, FR-094, FR-095.

**INV-09 — Search is derived and bounded**

The FTS index is a rebuildable derivative of canonical captures. It MUST index only text bodies and screenshot or voice captions in the MVP. Search-index failure or drift MUST NOT mutate or become the sole copy of canonical capture data.

Protects: FR-071, FR-072, FR-073, FR-098.

### Boundary security

**INV-13 — Narrow, validated privilege boundary**

Every UI-to-core operation MUST pass through a typed, validated Tauri command or narrowly scoped event. The UI MUST NOT receive unrestricted SQL, shell, arbitrary-path filesystem, microphone, or operating-system execution capability.

Protects: FR-092, FR-093 and the SRS security constraints.

## Absolute Prohibitions

| ID | The system MUST NEVER... |
|---|---|
| FRB-01 | Close the capture popup with a success outcome before the canonical capture and required media reference are durable. |
| FRB-02 | Wait for optional enrichment before accepting an otherwise valid capture. |
| FRB-03 | Replace a non-blank user caption with generated text. |
| FRB-04 | Commit a capture without exactly one valid context. |
| FRB-05 | Rewrite historical branch or context metadata because the current workspace later changes. |
| FRB-06 | Split a project's canonical history into branch-owned libraries. |
| FRB-07 | Upload capture content, screenshots, audio, transcripts, or captions as part of core MVP behavior. |
| FRB-08 | Treat an external media path, incomplete file, or temporary recording as a valid committed asset. |
| FRB-09 | Treat the FTS index as canonical data or index OCR/raw transcripts in the MVP contract. |
| FRB-10 | Generate or require a title for a text capture. |
| FRB-11 | Leave a canonical item or staged file after the user cancels its session. |
| FRB-12 | Create more than one capture from the same session identifier. |
| FRB-13 | Give the WebView direct database access, arbitrary shell execution, or unrestricted local-filesystem access. |
| FRB-14 | Log capture bodies, captions, transcripts, raw clipboard contents, or audio bytes. |
| FRB-15 | Require an account, network connection, paid API, speech model, OCR, vision model, embedding model, or vector database for core capture and retrieval. |

## Exception Handlers

| ID | Threatened invariant | Trigger | Contracted recovery |
|---|---|---|---|
| EXC-01 | INV-01 | Database commit fails after a save request | Return `STORAGE_WRITE_FAILED`, retain recoverable session input, keep the popup open, and do not acknowledge success. |
| EXC-02 | INV-01, INV-08 | Media finalization fails before capture commit | Roll back the capture transaction, retain or safely restage the media for retry when possible, and expose a media-specific error. |
| EXC-03 | INV-02 | Enrichment is unavailable, slow, or fails | Keep the accepted capture; record a bounded enrichment status and continue without generated metadata. |
| EXC-04 | INV-03 | Enrichment completes after a user caption exists or its input generation has changed | Discard the generated caption result; never write it over the current user value. |
| EXC-05 | INV-04 | Automatic context cannot produce one valid context | Preserve capture input and require manual selection or creation before save. |
| EXC-06 | INV-05 | Git branch cannot be read or the repository is detached | Store the known project context with a null named branch; do not infer or later backfill a branch silently. |
| EXC-07 | INV-06 | A query requests a project plus branch filter | Scope ownership by project first, then apply branch as an optional filter over that stream. |
| EXC-08 | INV-07 | Network is unavailable | Continue core operation; only an explicit model-download action may report a network error. |
| EXC-09 | INV-08 | A committed media file is missing or unreadable | Show an unavailable-media state with its metadata; do not substitute, relink, or delete unrelated content. |
| EXC-10 | INV-09 | FTS index is corrupt, unavailable, or out of sync | Disable affected search results, preserve canonical data, and rebuild the index transactionally from captures. |
| EXC-11 | INV-10 | An enrichment path proposes a text-note title or rewritten body | Reject the mutation and retain the original body unchanged. |
| EXC-12 | INV-11 | Cancellation cleanup cannot remove staged media immediately | Mark the session abandoned and retry scoped cleanup; never publish the staged data as a capture. |
| EXC-13 | INV-12 | Duplicate save command arrives for a completed session | Return the original capture identifier as an idempotent result or `STALE_SESSION`; do not create another record. |
| EXC-14 | INV-13 | IPC payload is invalid, path is outside allowed scope, or capability is not granted | Reject before side effects with a typed validation or permission error and record only non-sensitive diagnostic metadata. |

## Change Control

An implementation or future feature that would weaken an invariant requires an explicit architectural decision that updates this document, its single owner in [`04_transition_req_arch.md`](04_transition_req_arch.md), its enforcement mechanism in [`05_architecture.md`](05_architecture.md), and the affected tests. Historical invariants must not be silently reinterpreted.
