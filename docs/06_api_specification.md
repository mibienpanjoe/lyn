# Lyn — Typed Tauri IPC Specification

Version: v1.0, 2026-08-27

Derived from: [`05_architecture.md`](05_architecture.md)

Contract status: **Proposed for v1 implementation.** Lyn has no HTTP API or web backend. This document specifies the typed interface between the Svelte Frontend Shell and Rust Command Gateway.

## Conventions

### Transport

- Commands use Tauri 2 `invoke` transport.
- Rust owns command registration and validation.
- The frontend consumes generated or compile-checked TypeScript bindings from the same contract definitions.
- Commands use `snake_case` names at the Tauri boundary and `camelCase` JSON fields.
- Events are notifications only. Any state-changing action requires a command.

### Data conventions

- Identifiers are opaque UUID strings. The frontend MUST NOT infer meaning from them.
- Timestamps are RFC 3339 UTC strings and are localized only for display.
- Durations are integer milliseconds.
- Byte sizes are non-negative integers.
- Optional fields are explicit `null`, not omitted, in command results.
- User text is UTF-8 and is not normalized or rewritten by the transport layer.
- Commands accept media IDs and context IDs, never arbitrary filesystem paths.
- Enums are lowercase string literals.

### Result envelope

Every command resolves to one of these shapes rather than returning an untyped thrown value:

```ts
type CommandResult<T> =
  | { ok: true; data: T }
  | { ok: false; error: AppError };

interface AppError {
  code: ErrorCode;
  message: string;              // Safe, localized by UI when desired
  retryable: boolean;
  details: Record<string, string | number | boolean | null>;
}
```

`details` MUST NOT include capture content, transcript text, clipboard data, raw OS errors, or absolute private paths.

### Error codes

```ts
type ErrorCode =
  | "VALIDATION_ERROR"
  | "STALE_SESSION"
  | "CONTEXT_REQUIRED"
  | "EMPTY_CAPTURE"
  | "CAPTURE_NOT_FOUND"
  | "CONTEXT_NOT_FOUND"
  | "STORAGE_UNAVAILABLE"
  | "STORAGE_WRITE_FAILED"
  | "MEDIA_STAGE_FAILED"
  | "MEDIA_FINALIZE_FAILED"
  | "MEDIA_NOT_FOUND"
  | "UNSUPPORTED_CLIPBOARD_CONTENT"
  | "PERMISSION_DENIED"
  | "AUDIO_DEVICE_UNAVAILABLE"
  | "AUDIO_RECORDING_FAILED"
  | "AUDIO_PLAYBACK_FAILED"
  | "SEARCH_FAILED"
  | "SHORTCUT_CONFLICT"
  | "MODEL_NOT_AVAILABLE"
  | "MODEL_DOWNLOAD_FAILED"
  | "ENRICHMENT_FAILED"
  | "INTERNAL_ERROR";
```

## Shared Type Reference

```ts
type CaptureKind = "text" | "image" | "audio";
type ContextKind = "project" | "standalone";
type CaptionSource = "user" | "context_generated" | "transcript_generated";
type ContextProviderKind = "manual" | "vscode" | "shell" | "foreground_window";

interface ContextRef {
  id: string;
  kind: ContextKind;
  name: string;
}

interface ContextCandidate {
  context: ContextRef;
  branchName: string | null;
  provider: ContextProviderKind;
  requiresConfirmation: boolean;
}

interface CaptureSession {
  sessionId: string;
  contextCandidate: ContextCandidate | null;
  contextRequired: boolean;
  stagedMedia: StagedMedia | null;
  recordingState: RecordingState;
}

interface StagedMedia {
  stagedMediaId: string;
  kind: "image" | "audio";
  previewUri: string;            // Opaque lyn-media:// URI
  mimeType: "image/png" | "audio/wav";
  byteSize: number;
  durationMs: number | null;
  widthPx: number | null;
  heightPx: number | null;
}

type RecordingState =
  | { state: "idle" }
  | { state: "recording"; elapsedMs: number }
  | { state: "stopped"; elapsedMs: number; stagedMediaId: string };

interface SaveCaptureResult {
  captureId: string;
  capturedAt: string;
  enrichmentScheduled: boolean;
}

interface CaptureSummary {
  id: string;
  kind: CaptureKind;
  context: ContextRef;
  branchName: string | null;
  capturedAt: string;
  textExcerpt: string | null;
  caption: string | null;
  captionSource: CaptionSource | null;
  media: MediaSummary | null;
}

interface MediaSummary {
  mediaId: string;
  kind: "image" | "audio";
  previewUri: string;            // Opaque read-only application URI
  durationMs: number | null;
  widthPx: number | null;
  heightPx: number | null;
  available: boolean;
}

interface CaptureDetail extends CaptureSummary {
  textBody: string | null;
  sourceApp: string | null;
  sourceWindowTitle: string | null;
  updatedAt: string;
  enrichmentStatus: "not_requested" | "pending" | "completed" | "skipped" | "failed";
}

interface Page<T> {
  items: T[];
  nextCursor: string | null;
}
```

`previewUri` is created by Rust for a validated media ID. It is not a user-controlled URL and MUST resolve only through Lyn's read-only media protocol.

## Capture Session Commands

### `get_active_capture_session`

Return the one active session prepared by shortcut invocation. It MAY prepare a session when the capture window is opened through an explicit application menu action.

**Input:** `{}`

**Success:** `CommandResult<CaptureSession>`

**Errors:**

| Code | Trigger |
|---|---|
| `STORAGE_UNAVAILABLE` | Core storage could not initialize. |
| `INTERNAL_ERROR` | A safe session could not be prepared. |

### `select_capture_context`

Assign an existing context to the active session. The branch remains the current resolver result only when it belongs to the selected project; otherwise it is null.

**Input:**

```ts
interface SelectCaptureContextInput {
  sessionId: string;
  contextId: string;
}
```

**Success:** `CommandResult<CaptureSession>`

**Errors:** `STALE_SESSION`, `CONTEXT_NOT_FOUND`, `VALIDATION_ERROR`.

### `cancel_capture_session`

Cancel the session and remove only its staged media.

**Input:** `{ sessionId: string }`

**Success:** `CommandResult<{ cancelled: true }>`

**Idempotency:** Repeating cancellation for the most recently cancelled session returns `{ cancelled: true }` and creates no side effect.

**Errors:** `VALIDATION_ERROR`, `INTERNAL_ERROR` when cleanup is deferred. A deferred cleanup error MUST NOT create a capture.

### `save_text_capture`

Commit a titleless text capture.

**Input:**

```ts
interface SaveTextCaptureInput {
  sessionId: string;
  contextId: string;
  textBody: string;
}
```

There is intentionally no title field.

**Success:** `CommandResult<SaveCaptureResult>`

**Errors:**

| Code | Trigger |
|---|---|
| `STALE_SESSION` | Session is unknown, cancelled, or superseded. |
| `CONTEXT_REQUIRED` | No valid context is assigned. |
| `EMPTY_CAPTURE` | `textBody` is blank after the blankness check; original text is never rewritten. |
| `STORAGE_WRITE_FAILED` | Atomic database commit failed. |
| `VALIDATION_ERROR` | Payload exceeds a documented implementation limit or has invalid types. |

**Idempotency:** A repeated call with the same completed `sessionId` MUST return the original `captureId` or `STALE_SESSION`; it MUST NOT create a second capture.

## Screenshot Commands

### `stage_clipboard_image`

Read the current clipboard through Platform Service, encode supported image data as PNG, and attach the staged image to the active session.

**Input:** `{ sessionId: string }`

**Success:** `CommandResult<StagedMedia>` where `kind = "image"`.

**Errors:** `STALE_SESSION`, `UNSUPPORTED_CLIPBOARD_CONTENT`, `MEDIA_STAGE_FAILED`, `PERMISSION_DENIED`.

The frontend does not send raw clipboard bytes or a source path through IPC.

### `save_image_capture`

Finalize and commit the staged PNG.

**Input:**

```ts
interface SaveImageCaptureInput {
  sessionId: string;
  contextId: string;
  stagedMediaId: string;
  caption: string | null;
}
```

Whitespace-only captions are treated as null. A non-blank caption is stored with `captionSource = "user"` and prevents automatic replacement.

**Success:** `CommandResult<SaveCaptureResult>`

**Errors:** `STALE_SESSION`, `CONTEXT_REQUIRED`, `MEDIA_NOT_FOUND`, `MEDIA_FINALIZE_FAILED`, `STORAGE_WRITE_FAILED`, `VALIDATION_ERROR`.

## Voice Commands

### `start_audio_recording`

Begin an explicit microphone recording for the active session.

**Input:**

```ts
interface StartAudioRecordingInput {
  sessionId: string;
  inputDeviceId: string | null;
}
```

`null` requests the OS/default CPAL input device.

**Success:** `CommandResult<{ state: "recording" }>`

**Errors:** `STALE_SESSION`, `PERMISSION_DENIED`, `AUDIO_DEVICE_UNAVAILABLE`, `AUDIO_RECORDING_FAILED`.

### `stop_audio_recording`

Stop the current stream, finalize the staged WAV header, validate the format, and return preview metadata.

**Input:** `{ sessionId: string }`

**Success:** `CommandResult<StagedMedia>` where `kind = "audio"`.

**Errors:** `STALE_SESSION`, `AUDIO_RECORDING_FAILED`, `MEDIA_STAGE_FAILED`.

### `play_staged_audio`

Play session-owned staged audio through the native playback adapter.

**Input:** `{ sessionId: string; stagedMediaId: string }`

**Success:** `CommandResult<{ playing: true; durationMs: number }>`

**Errors:** `STALE_SESSION`, `MEDIA_NOT_FOUND`, `AUDIO_PLAYBACK_FAILED`.

### `stop_audio_playback`

Stop staged or committed Lyn audio playback.

**Input:** `{ playbackTargetId: string }`

`playbackTargetId` is a staged-media ID or committed media ID, never a path.

**Success:** `CommandResult<{ playing: false }>`

**Errors:** `MEDIA_NOT_FOUND`, `AUDIO_PLAYBACK_FAILED`.

### `save_audio_capture`

Finalize and commit staged WAV audio.

**Input:**

```ts
interface SaveAudioCaptureInput {
  sessionId: string;
  contextId: string;
  stagedMediaId: string;
  caption: string | null;
}
```

**Success:** `CommandResult<SaveCaptureResult>`

`enrichmentScheduled` is true only when the caption is null, local intelligence is enabled, and an eligible local model is available or installation policy permits a pending job. Save success never depends on the job outcome.

**Errors:** `STALE_SESSION`, `CONTEXT_REQUIRED`, `MEDIA_NOT_FOUND`, `MEDIA_FINALIZE_FAILED`, `STORAGE_WRITE_FAILED`, `VALIDATION_ERROR`.

## Context Commands

### `list_contexts`

List reusable contexts for manual selection and Library navigation.

**Input:**

```ts
interface ListContextsInput {
  kind: ContextKind | null;
  query: string | null;
  limit: number;                 // 1..100
}
```

**Success:** `CommandResult<{ contexts: ContextRef[] }>`

**Errors:** `VALIDATION_ERROR`, `STORAGE_UNAVAILABLE`.

### `create_context`

Create a standalone context or an explicitly selected project context.

**Input:**

```ts
type CreateContextInput =
  | { kind: "standalone"; name: string }
  | { kind: "project"; name: string; selectedDirectoryToken: string };
```

`selectedDirectoryToken` is issued by a native directory picker and is not a raw path supplied by the WebView.

**Success:** `CommandResult<{ context: ContextRef }>`

**Errors:** `VALIDATION_ERROR`, `PERMISSION_DENIED`, `STORAGE_WRITE_FAILED`.

Context rename and deletion are not in the MVP contract.

## Library and Search Commands

### `list_captures`

Return a bounded, deterministic chronological page.

**Input:**

```ts
interface ListCapturesInput {
  scope:
    | { kind: "all" }
    | { kind: "recent" }
    | { kind: "context"; contextId: string };
  branchName: string | null;
  captureKinds: CaptureKind[];
  capturedFrom: string | null;
  capturedTo: string | null;
  cursor: string | null;
  limit: number;                 // 1..100, default 50
}
```

**Success:** `CommandResult<Page<CaptureSummary>>`

**Errors:** `CONTEXT_NOT_FOUND`, `VALIDATION_ERROR`, `STORAGE_UNAVAILABLE`.

Ordering is `capturedAt DESC, id DESC`. A branch filter never changes context ownership.

### `get_capture`

Return canonical display data for one capture.

**Input:** `{ captureId: string }`

**Success:** `CommandResult<CaptureDetail>`

**Errors:** `CAPTURE_NOT_FOUND`, `STORAGE_UNAVAILABLE`.

### `search_captures`

Search the allowed FTS5 projection and return canonical capture summaries.

**Input:**

```ts
interface SearchCapturesInput {
  query: string;
  contextId: string | null;
  branchName: string | null;
  captureKinds: CaptureKind[];
  capturedFrom: string | null;
  capturedTo: string | null;
  cursor: string | null;
  limit: number;                 // 1..100, default 50
}
```

**Success:**

```ts
interface SearchResultItem {
  capture: CaptureSummary;
  matchedField: "text_body" | "caption";
  snippet: string;
}

type SearchCapturesResult = Page<SearchResultItem>;
```

Snippets MUST be generated from canonical allowed fields and safely escaped by Svelte when rendered. The command MUST compile/escape plain user input; it MUST NOT expose raw FTS query execution.

**Errors:** `VALIDATION_ERROR`, `CONTEXT_NOT_FOUND`, `SEARCH_FAILED`.

For a blank query, the UI MUST call `list_captures` for a recent/all view rather than relying on FTS behavior.

## Media Commands

### `open_media_external`

Open a committed Lyn-owned media asset with the operating system's default application.

**Input:** `{ mediaId: string }`

**Success:** `CommandResult<{ opened: true }>`

**Errors:** `MEDIA_NOT_FOUND`, `PERMISSION_DENIED`, `INTERNAL_ERROR`.

The command resolves and validates containment from `mediaId`; it never accepts a path or command string.

### `play_media`

Play a committed audio asset.

**Input:** `{ mediaId: string }`

**Success:** `CommandResult<{ playing: true; durationMs: number }>`

**Errors:** `MEDIA_NOT_FOUND`, `AUDIO_PLAYBACK_FAILED`.

Image preview uses the read-only opaque `previewUri` returned in media summaries and does not require a generic file-read command.

## Settings and Local Intelligence Commands

```ts
interface AppSettings {
  globalShortcut: string;
  providerOrder: ContextProviderKind[];
  theme: "system" | "light" | "dark";
  localSpeechEnabled: boolean;
}
```

### `get_settings`

**Input:** `{}`

**Success:** `CommandResult<AppSettings>`

**Errors:** `STORAGE_UNAVAILABLE`.

### `update_settings`

Update only the supplied fields after validation.

**Input:** `{ patch: Partial<AppSettings> }`

**Success:** `CommandResult<AppSettings>`

**Errors:** `VALIDATION_ERROR`, `SHORTCUT_CONFLICT`, `STORAGE_WRITE_FAILED`, `PERMISSION_DENIED`.

Shortcut registration and settings persistence MUST behave transactionally from the user's perspective: a failed new shortcut leaves the last working shortcut configured.

### `get_speech_model_status`

**Input:** `{}`

**Success:**

```ts
interface SpeechModelStatus {
  state: "not_installed" | "downloading" | "installed" | "invalid";
  modelId: string | null;
  downloadedBytes: number | null;
  totalBytes: number | null;
}
```

**Errors:** `INTERNAL_ERROR`.

### `install_speech_model`

Install one backend-allowlisted model artifact. The frontend selects an ID exposed by the build's trusted manifest and cannot supply a URL, checksum, or destination.

**Input:** `{ modelId: string }`

**Success:** `CommandResult<{ accepted: true; modelId: string }>`

**Errors:** `MODEL_NOT_AVAILABLE`, `MODEL_DOWNLOAD_FAILED`, `PERMISSION_DENIED`, `VALIDATION_ERROR`.

The command returns after the installation job is accepted. Progress arrives through events. An incomplete artifact remains staged and cannot become the active model.

### `remove_speech_model`

Remove the installed local model after active transcription releases it. Existing captures and captions remain unchanged.

**Input:** `{ modelId: string }`

**Success:** `CommandResult<{ removed: true }>`

**Errors:** `MODEL_NOT_AVAILABLE`, `INTERNAL_ERROR`.

## Events

Events are namespaced, contain no absolute paths or capture bodies, and cannot be used to authorize a mutation.

### `capture://session-ready`

Emitted after shortcut invocation and session preparation.

Payload: `CaptureSession`.

### `recording://state-changed`

```ts
interface RecordingStateEvent {
  sessionId: string;
  state: RecordingState;
  inputLevel: number | null;     // normalized 0..1, transient only
}
```

Input-level events SHOULD be rate-limited for UI rendering and MUST NOT be persisted as capture data.

### `playback://state-changed`

```ts
interface PlaybackStateEvent {
  playbackTargetId: string;
  state: "playing" | "paused" | "stopped" | "ended" | "failed";
  positionMs: number;
  durationMs: number;
}
```

### `enrichment://updated`

```ts
interface EnrichmentUpdatedEvent {
  captureId: string;
  status: "completed" | "skipped" | "failed";
  captionChanged: boolean;
}
```

The UI re-reads `get_capture` when it needs updated content.

### `model://download-progress`

```ts
interface ModelDownloadProgressEvent {
  modelId: string;
  state: "downloading" | "verifying" | "installed" | "failed";
  downloadedBytes: number;
  totalBytes: number | null;
}
```

## Outbound Calls

Core capture, Library, search, media, and transcription make **no outbound calls**.

The optional `install_speech_model` operation is the only network-capable v1 operation described here. Its implementation MUST:

- use an application-controlled, build-time allowlist of model IDs and HTTPS artifact origins;
- reject frontend-supplied URLs and redirect targets outside the allowlist;
- declare expected byte size and cryptographic checksum in the trusted manifest;
- download to a staging location;
- enforce a finite timeout and bounded retry policy;
- verify the complete artifact before atomic activation;
- preserve a previously valid installed model when download or verification fails;
- avoid sending capture content or a user identifier with the request.

No concrete model distributor is selected in the source overview. A release MUST NOT enable model download until its artifact source, license, checksum publication, and update policy are recorded in a dedicated ADR and build manifest.

## Error Mapping to SRS

| SRS case | Public error/status |
|---|---|
| ERR-001 shortcut conflict | `SHORTCUT_CONFLICT` |
| ERR-002 focus failure | Platform retry plus recoverable UI state; `INTERNAL_ERROR` only if surfaced |
| ERR-003 missing context | `CONTEXT_REQUIRED` |
| ERR-004 missing Git branch | Successful result with `branchName: null` |
| ERR-005 SQLite commit failure | `STORAGE_WRITE_FAILED` |
| ERR-006 screenshot/media finalization | `MEDIA_STAGE_FAILED` or `MEDIA_FINALIZE_FAILED` |
| ERR-007 unsupported clipboard | `UNSUPPORTED_CLIPBOARD_CONTENT` |
| ERR-008 microphone unavailable | `PERMISSION_DENIED` or `AUDIO_DEVICE_UNAVAILABLE` |
| ERR-009 recording stream failure | `AUDIO_RECORDING_FAILED` |
| ERR-010 playback failure | `AUDIO_PLAYBACK_FAILED` |
| ERR-011 speech unavailable/fails | Successful save; enrichment event reports skipped/failed |
| ERR-012 model installation failure | `MODEL_DOWNLOAD_FAILED` |
| ERR-013 FTS failure | `SEARCH_FAILED` |
| ERR-014 committed media missing | Summary has `available: false`; open/play returns `MEDIA_NOT_FOUND` |
| ERR-015 invalid/stale IPC | `VALIDATION_ERROR` or `STALE_SESSION` |
| ERR-016 exit during enrichment | Accepted capture persists; local job remains resumable |

## Command Summary

| Group | Command | Primary component | Mutation |
|---|---|---|---|
| Session | `get_active_capture_session` | Capture Service | Session only |
| Session | `select_capture_context` | Capture Service | Session only |
| Session | `cancel_capture_session` | Capture Service | Staging cleanup |
| Capture | `save_text_capture` | Capture Service | Yes |
| Screenshot | `stage_clipboard_image` | Media Service | Staging only |
| Screenshot | `save_image_capture` | Capture Service | Yes |
| Voice | `start_audio_recording` | Media Service | Staging only |
| Voice | `stop_audio_recording` | Media Service | Staging only |
| Voice | `play_staged_audio` | Media Service | No canonical mutation |
| Voice | `stop_audio_playback` | Media Service | No canonical mutation |
| Voice | `save_audio_capture` | Capture Service | Yes |
| Context | `list_contexts` | Context Resolver | No |
| Context | `create_context` | Context Resolver | Yes |
| Library | `list_captures` | Library Service | No |
| Library | `get_capture` | Library Service | No |
| Search | `search_captures` | Library Service | No |
| Media | `open_media_external` | Media Service | No |
| Media | `play_media` | Media Service | No |
| Settings | `get_settings` | Storage Service | No |
| Settings | `update_settings` | Storage Service | Yes |
| Intelligence | `get_speech_model_status` | Local Speech Adapter | No |
| Intelligence | `install_speech_model` | Local Speech Adapter | Local model state |
| Intelligence | `remove_speech_model` | Local Speech Adapter | Local model state |

## Contract Verification

- Generate or compile TypeScript bindings from Rust contract types in CI.
- Run serialization round-trip tests for every shared type.
- Run negative tests for unknown fields, invalid enums, malformed UUIDs/timestamps, stale sessions, duplicate saves, path traversal attempts, and over-limit pages.
- Assert that no core command accepts a URL, SQL statement, shell string, absolute media path, or raw clipboard/audio payload.
- Verify every `ErrorCode` renders an actionable UI state and no error details leak sensitive content.
- Treat any command rename or incompatible payload change as a coordinated contract migration.
