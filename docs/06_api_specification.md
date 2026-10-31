# Lyn — Typed Tauri IPC Specification

Version: v1.20, 2026-10-27

Derived from: [`05_architecture.md`](05_architecture.md)

Contract status: **Shared primitives, manual and live contexts, capture-session lifecycle, durable text/media save, popup window controls, paged Library reads, committed-media actions, bounded local search, and transactional settings are implemented. Persistent post-commit enrichment coordination is implemented behind an adapter boundary; local speech/model delivery uses the optional Parakeet package; installed recognition acceptance remains pending.** Lyn has no HTTP API or web backend. This document specifies the typed interface between the Svelte Frontend Shell and Rust Command Gateway.

## Conventions

### Transport

- Commands use Tauri 2 `invoke` transport.
- Each command receives one JSON argument named `input`; commands documented with input `{}` reject any fields in that object.
- Rust owns command registration and validation.
- The frontend consumes generated or compile-checked TypeScript bindings from the same contract definitions.
- Commands use `snake_case` names at the Tauri boundary and `camelCase` JSON fields.
- Events are notifications only. Any state-changing action requires a command.

### Data conventions

- Identifiers are opaque UUID strings. The frontend MUST NOT infer meaning from them.
- Timestamps are RFC 3339 UTC strings and are localized only for display.
- Durations are integer milliseconds.
- Byte sizes are non-negative integers.
- Optional fields are explicit `null`, not omitted, in command results, except the additive `IntegrationStatus.native` and `native.activation` extensions, which are omitted when absent for legacy compatibility.
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
  details: Partial<Record<ErrorDetailKey, string | number | boolean | null>>;
}

type ErrorDetailKey =
  | "field"
  | "limit"
  | "operation"
  | "permission"
  | "resourceKind"
  | "retryAfterMs"
  | "state";
```

The Rust-owned allowlist rejects other detail keys at deserialization boundaries. `details` MUST NOT include capture content, transcript text, clipboard data, raw OS errors, or absolute private paths in either keys or values.

### Error codes

```ts
type ErrorCode =
  | "VALIDATION_ERROR"
  | "STALE_SESSION"
  | "CONTEXT_REQUIRED"
  | "CONTEXT_AMBIGUOUS"
  | "CONTEXT_SOURCE_NOT_FOUND"
  | "CONTEXT_SOURCE_STALE"
  | "EMPTY_CAPTURE"
  | "CAPTURE_NOT_FOUND"
  | "CONTEXT_NOT_FOUND"
  | "CONTEXT_NOT_EMPTY"
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
type ContextSourceKind =
  | "vscode_window"
  | "integrated_terminal"
  | "external_terminal"
  | "shell"
  | "foreground_window";

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

type ContextSelection =
  | { kind: "live_source"; sourceId: string }
  | { kind: "saved_context"; contextId: string };

interface ContextSourceOption {
  sourceId: string;              // Opaque and valid only while the source is live
  kind: ContextSourceKind;
  provider: ContextProviderKind;
  applicationName: string;
  label: string;                 // Safe project/worktree label; never terminal content
  context: ContextRef;
  branchName: string | null;
  isForeground: boolean;
}

type ContextResolution =
  | { state: "resolved"; candidate: ContextCandidate; selection: ContextSelection | null }
  | { state: "ambiguous"; candidate: null; selection: null }
  | { state: "required"; candidate: null; selection: null };

interface CaptureSession {
  sessionId: string;
  contextRevision: number;       // Rust-owned u32, increasing within this session
  contextResolution: ContextResolution;
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

### Local provider observation contract

The separate [native GNOME protocol](gnome_native_protocol.md) defines a fixed-width, metadata-only channel with connection/request generations and pre-invocation topology proof. Its Rust transport, peer/window/PTY/process attestation and native capture/Library backend wiring are implemented and exercised against isolated real GNOME. The owning contract records desktop fixture coverage and the remaining module activation, cold-start/lifecycle and packaged acceptance work. It grants no Tauri capability and does not permit native identifiers in the editor/browser JSON channels below. A decoded native candidate is not a validated observation.

Context providers are Rust-side adapters, not frontend IPC. They submit only validated correlation metadata: provider/source kind, opaque OS window/process/session tokens, workspace or working directory, observation time, and liveness. The private terminal broker accepts only bounded version, process, session, window, and focus fields; Rust derives the process working directory after same-user and active-window validation. Context Resolver derives the public `ContextSourceOption`; raw paths and tokens never cross the Tauri boundary. Provider observations MUST NOT contain terminal commands/output, environment values, editor contents, clipboard content, or agent conversations, and MUST NOT be persisted.

Linux editor integrations share `lyn-context-v1.sock`. Heartbeats remain protocol v1 `{version, instanceId, state, workspaceFolders}`. An editor-originated capture uses protocol v2 `{version: 2, kind: "invoke", instanceId, requestId, workspaceFolders, surface?}`. `surface` is `editor` (default) or `terminal`. `requestId` is the invoke generation: a later frame may complete that invoke only when it carries the same v2 generation. Rust assigns window and process correlation at receive time; client payloads MUST NOT include native window or process identifiers (including a `cwd` field). The Linux Unix channel checks the peer user id, a read deadline, and a max message size before parsing; a replayed v2 `requestId` is ignored. A v1 payload with `intent: "invoke"` is a compatibility shim that may open capture but MUST NOT receive v2 generation guarantees. Workspace folders are correlation hints for local directories; they never become UI filesystem paths. An editor invoke uses workspace folders and ignores the last-used integrated terminal. A terminal invoke may send that focused terminal's live working directory as the single hint; without it, Lyn does not guess the workspace. Multi-root editor folders register distinct live sources and resolve as ambiguous. Remote folders remain manual. Manual selection (INV-15) still outranks a later provider frame for the same capture.

Linux terminal integrations share `lyn-shell-v1.sock`. Kitty heartbeats remain `{version: 1, terminalSessionId, processId, state}`. A Kitty-originated capture uses `{version: 2, kind: "invoke", requestId, terminalSessionId, processId}`. Rust assigns the OS window at receive time, validates the same-user process, and derives cwd from `/proc`. A generic shell observation for a Kitty window that already has an exact pane mapping is ignored. A generic shell Live names the helper-bound OS window; Rust classifies that window even if Lyn has already taken focus, and a global shortcut while Lyn is focused reuses that last terminal window instead of dropping automatic context.

A newly started shell helper binds only to the actual foreground window; it cannot inherit Lyn's remembered window. Its bound process/window heartbeat continues every two seconds while the same process is alive, including when another application has focus. This prevents an existing shell source from expiring solely because its window stayed in the background. The broker validates the bound window and same-user process on each report. A background report refreshes that source but does not replace the remembered foreground window. Automatic resolution still requires the invocation's exact window association, and multiple shell sessions sharing that window remain ambiguous.

Linux browser integrations share `lyn-browser-v1.sock`. Heartbeats remain protocol v1 `{version, instanceId, state, url, incognito}`. A browser-originated capture uses protocol v2 `{version: 2, kind: "invoke", instanceId, requestId, url, incognito}`. Only sanitized `file:` and localhost-family URLs (`localhost`, `127.0.0.1`, `::1`) are correlation hints; query strings, fragments, titles, incognito tabs, and remote sites are omitted. Rust assigns the OS window at receive time; client payloads MUST NOT include native window or process identifiers (including a `cwd` field). The Linux Unix channel checks the peer user id, a read deadline, and a max message size before parsing; a replayed v2 `requestId` is ignored. Localhost port mapping requires exactly one same-user process working directory after skipping `docker-proxy`. IPv4 and IPv6 listen sockets that resolve to the same directory are one source. Multiple distinct cwds, a proxy/Docker listener without a unique remaining project directory, or a remote URL stay unresolved (Required), not a first-PID guess.

Linux capture entry points are distinct. Editor and terminal integrations (VS Code, Cursor, Kitty) invoke with `Alt+L` on a private socket and receive v2 generation guarantees. The browser companion suggests `Alt+L` for the focused tab; Firefox MV3 uses `background.scripts` rather than a service worker. The Settings global shortcut (default `Control+Shift+Space`) samples the pre-popup X11 window and must not use the integration combination. An existing global `Alt+L` setting is preserved on upgrade and must be changed by the user to avoid intercepting integration invocations; unrelated settings remain editable. A global invoke without a verified live source for that window resolves as required (ERR-003); it MUST NOT promote a later heartbeat to exact proof.

## Capture Session Commands

### `get_active_capture_session`

Return the one active session prepared by shortcut invocation. It MAY prepare a session when the capture window is opened through an explicit application menu action.

Until automatic context providers are connected, a newly prepared session starts with `contextResolution.state = "required"`. Repeated calls return the same session identifier and state until that session is cancelled or durably saved.

**Input:** `{}`

**Success:** `CommandResult<CaptureSession>`

**Errors:**

| Code | Trigger |
|---|---|
| `STORAGE_UNAVAILABLE` | Core storage could not initialize. |
| `INTERNAL_ERROR` | A safe session could not be prepared. |

### `list_capture_context_sources`

List currently eligible live sources and saved contexts for the active capture. Labels are safe summaries and never include terminal commands/output or agent/editor content.

**Input:**

```ts
interface ListCaptureContextSourcesInput {
  sessionId: string;
  query: string | null;
  limit: number;                 // 1..100 per group
}
```

**Success:**

```ts
CommandResult<{
  liveSources: ContextSourceOption[];
  savedContexts: ContextRef[];
}>
```

**Errors:** `STALE_SESSION`, `VALIDATION_ERROR`, `STORAGE_UNAVAILABLE`.

### `select_capture_context_source`

Replace the proposed context for this capture only. The command changes only context resolution state: entered text, staged media, and recording state MUST remain byte-for-byte/identity equivalent.

**Input:**

```ts
interface SelectCaptureContextSourceInput {
  sessionId: string;
  selection: ContextSelection;
}
```

Live sources are revalidated before selection. A saved context has `branchName: null` unless Context Resolver can safely refresh it from a currently associated live source.

Live sources are revalidated at selection and save time. A stale source returns `CONTEXT_SOURCE_STALE` without changing the session or its draft state.

Manual selection reserves a Rust-only ticket before lookup, preventing automatic completion from overtaking the user's choice. A newer selection supersedes it; a failed lookup releases its reservation without changing the context/media. Saves are refused while a selection is pending. An automatic query invalidated by that attempt is not revived after failure; retry selection or invoke again.

For text, image and audio saves, source revalidation and project lookup run outside the capture-session mutex against a snapshot. Before `save_once`, Rust checks the same active session and private context epoch; an intervening correction (including away-and-back), cancellation or replacement returns `STALE_SESSION`. Media-only changes do not invalidate the context epoch; final validation uses the current session under `save_once`, not the earlier snapshot. Capture persistence and staged-media finalization stay inside this serialized boundary.

Text, image, and audio saves use the same serialized live-selection validation before persistence or media finalization. On Linux, a native shell source must still match its registered process uid/inode and project directory; a recent heartbeat alone is insufficient. Rejection leaves staged media available for preview and context reselection.

An attested GNOME source additionally retains its original window, channel generation, native topology and kernel lifetime evidence. Selection/save continuity queries use their own bounded deadlines and must match that original evidence; they do not extend the initial invocation deadline. Rust registers one native entry only after accepting the capture-session ticket, excludes it from generic automatic snapshots and heartbeat expiry, and releases it on successful text/image/audio save, cancellation or popup close. GNOME without fresh native evidence remains unresolved; the remembered window or generic helper cannot replace that proof. Detailed evidence requirements remain owned by the [native contract](gnome_native_protocol.md).

**Success:** `CommandResult<CaptureSession>`

**Errors:** `STALE_SESSION`, `CONTEXT_SOURCE_NOT_FOUND`, `CONTEXT_SOURCE_STALE`, `CONTEXT_NOT_FOUND`, `VALIDATION_ERROR`.

### `cancel_capture_session`

Cancel the session and remove only its staged media.

**Input:** `{ sessionId: string }`

**Success:** `CommandResult<{ cancelled: true }>`

**Idempotency:** Repeating cancellation for the most recently cancelled session returns `{ cancelled: true }` and creates no side effect.

**Errors:** `VALIDATION_ERROR`, `STALE_SESSION`, `INTERNAL_ERROR` when cleanup is deferred. A deferred cleanup error MUST NOT create a capture. The implemented state machine queues cleanup by the cancelled session and staged-media identifiers; the Media Service worker that performs filesystem cleanup remains a later slice.

### `dismiss_capture_popup`

Hide the capture popup after a successful save or cancellation and attempt to restore the invocation-bound foreground window. The native correlation token remains transient and Rust-only.

**Input:** `{}`

**Success:** `CommandResult<{ dismissed: true; focusRestored: boolean }>`

`focusRestored` is false when the popup was opened directly or the first supported platform cannot safely reactivate the recorded window. Dismissal remains successful in that recoverable case.

**Errors:** `VALIDATION_ERROR`, `INTERNAL_ERROR` when the popup cannot be hidden.

### `set_capture_popup_layout`

Resize the native capture window for the current presentation state without exposing arbitrary window dimensions to the frontend.

**Input:** `{ layout: "compact" | "audio" | "error" | "chooser" | "media"; viewportWidth?: number }`

**Success:** `CommandResult<{ layout: "compact" | "audio" | "error" | "chooser" | "media" }>`

Rust maps each semantic state to a bounded CSS height and preserves the current physical width. The optional `viewportWidth` is the UI's measured `window.innerWidth` in CSS pixels, not a requested native width. It must be finite and within `[1, 16384]`; Rust divides its own physical window width by that measurement and requires the resulting ratio within `[0.5, 8]`. Refused measurements return `VALIDATION_ERROR`. Physical height is the semantic height multiplied by that ratio, rounded up and capped by the current monitor work area with a decoration reserve. Omitted measurements retain the native scale-factor behavior for existing callers. Arbitrary target dimensions are not accepted. The capture client reapplies the semantic layout when its CSS viewport width changes; height-only native resize events do not cause a resize loop. The frontend requests `chooser` while context selection is open, `media` for the screenshot-preview state, `audio` for completed-recording controls without an image preview, `error` when inline failure feedback expands an otherwise text-only capture, and `compact` otherwise. A resize failure is recoverable and MUST NOT modify or cancel the active capture session.

**Errors:** `VALIDATION_ERROR` for a refused viewport measurement; `INTERNAL_ERROR` when the native window cannot be resized.

### `save_text_capture`

Commit a titleless text capture.

**Input:**

```ts
interface SaveTextCaptureInput {
  sessionId: string;
  textBody: string;
}
```

There is intentionally no title field.

`textBody` is limited to 1,048,576 UTF-8 bytes. Blankness is checked without modifying the value; a non-blank accepted body, including leading/trailing whitespace and line breaks, is stored exactly as supplied.

**Success:** `CommandResult<SaveCaptureResult>`

**Errors:**

| Code | Trigger |
|---|---|
| `STALE_SESSION` | Session is unknown, cancelled, or superseded. |
| `CONTEXT_REQUIRED` | No valid context is assigned. |
| `CONTEXT_AMBIGUOUS` | Multiple sources remain equally plausible and no explicit selection exists. |
| `CONTEXT_SOURCE_STALE` | The selected live source disappeared or changed identity before save. |
| `EMPTY_CAPTURE` | `textBody` is blank after the blankness check; original text is never rewritten. |
| `STORAGE_WRITE_FAILED` | Atomic database commit failed. |
| `VALIDATION_ERROR` | Payload exceeds a documented implementation limit or has invalid types. |

**Idempotency:** A repeated call with the same completed `sessionId` returns `STALE_SESSION` and MUST NOT create a second capture. Concurrent duplicate deliveries are serialized by the session service, with database uniqueness as the durable backstop.

## Screenshot Commands

### `stage_clipboard_image`

Read the current clipboard through Platform Service, encode supported image data as PNG, and attach the staged image to the active session.

**Input:** `{ sessionId: string }`

**Success:** `CommandResult<StagedMedia>` where `kind = "image"`.

**Errors:** `STALE_SESSION`, `UNSUPPORTED_CLIPBOARD_CONTENT`, `MEDIA_STAGE_FAILED`, `PERMISSION_DENIED`.

The frontend does not send raw clipboard bytes or a source path through IPC.

### `discard_staged_media`

Remove the exact session-owned staged image or audio asset and return the active session to text-capture state without changing its draft or selected context.

**Input:** `{ sessionId: string; stagedMediaId: string }`

**Success:** `CommandResult<CaptureSession>` with `stagedMedia = null` and `recordingState.state = "idle"`.

**Errors:** `VALIDATION_ERROR`, `MEDIA_NOT_FOUND`, `STALE_SESSION`, `INTERNAL_ERROR`.

The command accepts only an opaque staged-media identifier owned by the active session. It cannot remove canonical media or another session's staging.

### `save_image_capture`

Finalize and commit the staged PNG.

**Input:**

```ts
interface SaveImageCaptureInput {
  sessionId: string;
  stagedMediaId: string;
  caption: string | null;
}
```

Whitespace-only captions are treated as null. A non-blank caption is stored with `captionSource = "user"` and prevents automatic replacement.

**Success:** `CommandResult<SaveCaptureResult>`

**Errors:** `STALE_SESSION`, `CONTEXT_REQUIRED`, `CONTEXT_AMBIGUOUS`, `CONTEXT_SOURCE_STALE`, `MEDIA_NOT_FOUND`, `MEDIA_FINALIZE_FAILED`, `STORAGE_WRITE_FAILED`, `VALIDATION_ERROR`.

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
  stagedMediaId: string;
  caption: string | null;
}
```

**Success:** `CommandResult<SaveCaptureResult>`

`enrichmentScheduled` is true only when the caption is null, local intelligence is enabled, and an eligible local model is available or installation policy permits a pending job. Save success never depends on the job outcome.

**Errors:** `STALE_SESSION`, `CONTEXT_REQUIRED`, `CONTEXT_AMBIGUOUS`, `CONTEXT_SOURCE_STALE`, `MEDIA_NOT_FOUND`, `MEDIA_FINALIZE_FAILED`, `STORAGE_WRITE_FAILED`, `VALIDATION_ERROR`.

## Context Commands

### `pick_project_directory`

Open the operating system's native directory picker. Rust validates the selected directory and retains its canonical path in process memory; no path crosses IPC.

**Input:** `{}`

**Success:**

```ts
CommandResult<{
  selection: {
    selectedDirectoryToken: string;
    suggestedName: string;
  } | null;
}>
```

Cancellation succeeds with `selection: null`. The opaque token expires after five minutes, is consumed by one `create_context` attempt, and becomes invalid when Lyn exits. `suggestedName` is a bounded, display-safe directory basename and is not an authority to access that directory.

**Errors:** `VALIDATION_ERROR`, `PERMISSION_DENIED`, `INTERNAL_ERROR`.

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

`selectedDirectoryToken` is issued by `pick_project_directory` and is not a raw path supplied by the WebView. Rust consumes and revalidates the token before inspecting Git metadata or writing a project context. A Git common directory is the stable project key when available; a valid non-Git directory remains a project context without a Git key.

**Success:** `CommandResult<{ context: ContextRef }>`

**Errors:** `VALIDATION_ERROR`, `PERMISSION_DENIED`, `STORAGE_WRITE_FAILED`.

Context rename and standalone-context deletion are not in the MVP contract.

### `delete_project`

Remove an empty project context from the Library. Available only to the `main`
Library window, never the capture popup.

**Input:** `{ contextId: ContextId }`, rejecting unknown fields and paths.

**Success:** `CommandResult<{ deleted: boolean }>`; `deleted: false` means the ID
was already absent, making retries safe. Standalone IDs return `VALIDATION_ERROR`.
The authoritative parameterized DELETE checks project kind and absence of **all**
captures in the same statement, independently of UI filters or pagination.
If a capture was added before confirmation, return `CONTEXT_NOT_EMPTY` and retain
both project and captures. No media or filesystem folder is deleted. A future
automatic capture can discover the project again; removal does not blacklist
a source or cancel capture sessions. Selecting an absent saved context returns
the existing `CONTEXT_NOT_FOUND` error. Capture saves retain their foreign-key
constraint and preserve the draft on persistence failure.

**Errors:** `VALIDATION_ERROR`, `CONTEXT_NOT_EMPTY`, `STORAGE_WRITE_FAILED`.

The Library offers Delete project in the selected project's toolbar, confirms
its name, and changes navigation only after a successful command. A failed
request keeps the project available and reports the reason.

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

### `get_integration_statuses`

**Input:** `{}`. **Success:** `IntegrationStatus[]` (empty on non-Linux platforms). Filesystem and native connection checks run on a blocking worker, outside the UI event loop. The command accepts no paths, peer identifiers or service actions. It never resumes installer transactions or sends a context/capture query.

```ts
type IntegrationId = "cursor" | "vscode" | "browser" | "kitty" | "shell" | "gnome";
type GnomeActivationFilesStatus = "absent" | "owned" | "changed" | "pending" | "unavailable";
type GnomeConnectionStatus = "connected" | "disconnected" | "busy" | "unavailable";
interface GnomeIntegrationEvidence {
  files: GnomeActivationFilesStatus;
  connection: GnomeConnectionStatus;
  canInstall: boolean;
  activation?: {
    phase: "absent" | "configured" | "installing" | "finishing_update" | "restoring_update" | "removing" | "awaiting_removal_confirmation" | "changed" | "unavailable";
    revision: string | null;
    actions?: GnomeManagementAction[];
  };
}
interface IntegrationStatus {
  id: IntegrationId;
  name: string;
  description: string;
  detected: boolean;
  installed: boolean;
  details: string | null;
  native?: GnomeIntegrationEvidence;
}
```

The GNOME row keeps file ownership and admitted transport observations separate. `owned` covers the three activation files only; `connected` covers an observed authenticated transport, not the focused source or a verified capture. Missing evidence must display unavailable, never Installed/Ready or an installation action. No private path, PID or source identifier is returned in native evidence. GNOME `installed` remains false: preparation/connection alone does not establish packaged installation acceptance. `canInstall` is an advisory preparation offer only when a validated build-time bundle exists, a server is detected and both file and activation observations are absent. It is not a compatibility verdict. Default builds without a bundle keep it false; pending, changed, retired and unavailable authority never offer first installation. The [native protocol contract](gnome_native_protocol.md) owns the precise observation/refusal boundaries.

The additive activation observation distinguishes recognized complete configuration, interrupted first installation, committed update cleanup, uncommitted restoration, removal and retained removal authority. Each recognized owned phase carries an opaque64-hex revision derived from its pinned authority file's identity/change time/content with a phase-specific domain; complete configuration preserves the existing rollback revision. It is a freshness marker for the same recorded configuration/intent, not a credential, a module version, an action permission or an atomic global snapshot. The same intent can retain its revision as recorded cleanup progresses. Absent/changed/unavailable observations have `revision:null`. An identical-byte replacement of the authority changes its fresh revision and invalidates a retained reader. Status creates no locks/directories, performs no manager/provider RPC or transaction recovery and never acknowledges removal. The UI uses localized phase text with legacy file-status fallback and does not display the revision. The separate `manage_gnome_activation` command below consumes an expected revision with authoritative checks; this metadata itself performs no mutation.

The optional `actions` list contains advisory offers for the observed phase. Status checks retained modules through bounded read-only ownership/content validation, without locks, writes or manager RPC. Configured activation offers synchronization only with an intact current module, update only with a different valid embedded bundle as well, and rollback only with an intact recorded previous module; removal remains available with damaged module data. Recovery offers require an intact target module. Missing offers preserve legacy display without management buttons. Offers never replace authoritative action validation.

### `manage_gnome_activation`

Library-only Linux command, registered through the command allowlist and the Library permission set, never the capture window. Runs on a blocking worker.

**Input:** `{ action, expectedRevision }`, rejecting all additional fields. `expectedRevision` must be exactly64 lowercase hexadecimal characters obtained from recognized `native.activation` evidence. `action` is one of `update`, `rollback`, `remove`, `uninstall`, `recover_install`, `finish_update`, `restore_update`, `recover_removal`, `synchronize`, `confirm_removal`. No paths, module/digest selector, process identity, first-install or terminal restart action is accepted. Update uses only the build-time embedded bundle; a build without one refuses before writes.

**Success:** `{ action, activation: GnomeActivationEvidence }` in the standard envelope. The coordinator completed the requested operation; `activation` is a subsequent read-only observation, not atomic with that effect and not proof of a running native module/capture. A retired removal may be synchronized/confirmed without an embedded bundle; a completed absent-record confirmation cannot reuse a retired revision.

The expected revision is validated before service work and again under the same installation lock before mutation; update checks before module-store writes. Snapshot/inode continuity remains checked through callbacks and postflight, which inspects the new authority rather than comparing it to the displaced request revision. Synchronization checks the lock immediately before service entry and again after the final manager fence before `Manager.Reload`; acknowledgement checks it before unlink. No automatic retry, context/capture query or terminal restart occurs.

`uninstall` composes strict removal (or pending-removal recovery), manager synchronization and retirement acknowledgement. The first transaction captures its retired revision while still holding its flock; acknowledgement consumes that exact token, never a later status revision. Already retired authority uses the supplied expected revision directly. Other pending phases refuse. The legacy `remove` action still stops at retirement. An error can leave retirement authority for a fresh explicit `confirm_removal`; modules and open terminals remain unchanged.

Settings sends one explicit action with the displayed revision, disables competing controls, retires its cached offers and refreshes status after success or error. It never retries or chains actions automatically. A failed refresh keeps old offers disabled; a receipt whose phase/revision differs from the refreshed observation loses its success message. Native management errors use localized fixed text.

**Errors:** malformed/unknown input or noncanonical revisions return `VALIDATION_ERROR`. Linux management refusals use `PERMISSION_DENIED` with one closed `details.state`: `stale_revision`, `artifact_unavailable`, `location_unavailable`, `configuration_changed`, `service_unavailable` or `configuration_unavailable`. Non-Linux use is refused. No raw OS error or private path is returned. Management failures are not automatically retryable: they may follow filesystem changes or an effective manager reload; refresh status before a new explicit action. First-install availability and packaged/installed acceptance remain separate.

### `install_integration`

**Input:** `{ id: IntegrationId }`, rejecting all additional fields. **Success:** `{ id, success, message, installed, configurationRevision? }` inside the standard result envelope. Existing integration result payloads omit the optional revision and retain their behavior. The command runs filesystem/service work on a blocking worker.

GNOME prepares only a first installation from the validated embedded bundle at internally resolved destinations. Missing bundle refuses before filesystem or bus work. Existing, pending or retired authority refuses rather than selecting recovery/update implicitly. The coordinator checks the strict stock service and exact destination membership before creation and before publication, retains its original flock and module/directory evidence through final validation, then captures `configurationRevision` and rechecks the original lock before returning. Success means configuration prepared: `success:true`, `installed:false` and the canonical revision of those complete activation files. No manager Reload or terminal action occurs; applying the configuration is a separate explicit revisioned action. Failure returns a fixed safe message, no revision and both booleans false; files or recovery authority may remain after errors.

The UI sends one preparation request, retires its cached offers and refreshes after either result or error. It shows localized preparation success only when refreshed configured authority has the exact returned revision, never claiming a running module. A failed refresh keeps old installation/management offers disabled until an accepted read. No automatic retry or apply action follows. Default builds still have no install offer; packaging, genuine Tauri IPC and installed acceptance remain separate verification boundaries.

`get_settings`, `update_settings`, and `get_app_version` are implemented. Speech-model commands and model-progress events are registered. The optional package is Parakeet v3 Q8; its [distribution decision](09_local_speech_distribution_decision.md) owns lifecycle and artifact details.

```ts
interface AppSettings {
  globalShortcut: string;
  providerTieBreakOrder: ContextProviderKind[];
  theme: "system" | "light" | "dark";
  localSpeechEnabled: boolean;
}

interface AppVersion {
  version: string;
}
```

### `get_settings`

**Input:** `{}`

**Success:** `CommandResult<AppSettings>`

**Errors:** `STORAGE_UNAVAILABLE`.

### `get_app_version`

Read the version of the running Lyn binary. This is package metadata, not a user preference.

**Input:** `{}`

**Success:** `CommandResult<{ version: string }>`

**Errors:** `VALIDATION_ERROR`.

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
  label: string;
  errorCode: ErrorCode | null;
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

### Voice caption correction and explicit transcription retry

`CaptureDetail.captionRevision: number` exposes the stored caption revision. These Library-only commands accept closed typed inputs and return `CommandResult<{ accepted: boolean }>`:

- `update_voice_caption`: `{ captureId, expectedCaptionRevision, caption }`. Audio captures only; trim and bound the caption to 500 Unicode characters. Empty text clears it. Atomically compare the revision, write user ownership, increment the revision, update FTS and retire pending/running/failed enrichment. A stale edit leaves stored data unchanged and returns `ENRICHMENT_FAILED`; the UI preserves its correction draft.
- `retry_voice_transcription`: `{ captureId, expectedCaptionRevision }`. Require installed Parakeet, enabled speech, an unchanged captionless audio capture and a failed/skipped speech job. Atomically change only that job to pending, then start the background worker. Duplicate/stale/ineligible requests refuse. Never accept media paths or a client-selected engine.

Both reject unknown fields (`VALIDATION_ERROR`) and return safe storage failures. Retry also returns `MODEL_NOT_AVAILABLE` with the route to Settings when the package or opt-in is missing. Failed/interrupted jobs require explicit retry; they are not reclaimed automatically. `enrichment://updated` refreshes settled Library state. Generated captions remain bounded to 500 characters; no separate full transcript is stored.

## Events

Events are namespaced, contain no absolute paths or capture bodies, and cannot be used to authorize a mutation.

### `capture://session-ready`

Emitted after shortcut or editor invocation prepares a **new** capture session.

Payload: `CaptureSession`.

### `capture://context-revised`

Emitted when the active session's context resolution changes while that session is already open (including a second invoke of the same session). Payload: `CaptureSession`. The event MUST NOT be treated as a new capture: the UI updates context only and MUST leave draft text, staged media, and recording state untouched. A later automatic revision MUST NOT replace a user selection for this capture (INV-15).

`contextRevision` is additive output metadata, initialized to zero and incremented by Rust on every context mutation; overflow refuses the mutation. It is not an authorization token or a media revision. Receivers register listeners before reading the initial session, retain newer context revisions received during bootstrap, and ignore equal/older same-session context events. Command responses may update media but MUST preserve a newer context already received. Revisions cannot be compared across session IDs. Linux automatic resolution freezes eligible source metadata before dispatch and runs on one worker with one coalesced pending request, outside the capture-session mutex; a Rust-only single-use ticket fences session, context and invocation changes before publication. These ordering safeguards do not establish native GNOME capture or packaged-runtime latency acceptance.

### `context://sources-changed`

Emitted when eligible live sources for the active capture may have changed. Payload: `{ sessionId: string }`. The event contains no source details; the UI re-runs `list_capture_context_sources` only while the chooser is open.

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
| ERR-017 ambiguous context evidence | `CONTEXT_AMBIGUOUS`; draft remains available and chooser opens |
| ERR-018 selected live source stale | `CONTEXT_SOURCE_STALE`; draft remains available for refresh/reselection |
| ERR-019 malformed/unavailable provider registration | Provider is ignored; resolution continues or returns `CONTEXT_REQUIRED`/`CONTEXT_AMBIGUOUS` |

## Command Summary

| Group | Command | Primary component | Mutation |
|---|---|---|---|
| Session | `get_active_capture_session` | Capture Service | Session only |
| Session | `list_capture_context_sources` | Context Resolver | No |
| Session | `select_capture_context_source` | Capture Service + Context Resolver | Session only |
| Session | `cancel_capture_session` | Capture Service | Staging cleanup |
| Window | `set_capture_popup_layout` | Platform Service | Native window only |
| Capture | `save_text_capture` | Capture Service | Yes |
| Screenshot | `stage_clipboard_image` | Media Service | Staging only |
| Media | `discard_staged_media` | Capture Service + Media Service | Staging cleanup |
| Screenshot | `save_image_capture` | Capture Service | Yes |
| Voice | `start_audio_recording` | Media Service | Staging only |
| Voice | `stop_audio_recording` | Media Service | Staging only |
| Voice | `play_staged_audio` | Media Service | No canonical mutation |
| Voice | `stop_audio_playback` | Media Service | No canonical mutation |
| Voice | `save_audio_capture` | Capture Service | Yes |
| Context | `list_contexts` | Context Resolver | No |
| Context | `create_context` | Context Resolver | Yes |
| Context | `delete_project` | Context Repository (Library only) | Yes, empty project entry only |
| Library | `list_captures` | Library Service | No |
| Library | `get_capture` | Library Service | No |
| Search | `search_captures` | Library Service | No |
| Media | `open_media_external` | Media Service | No |
| Media | `play_media` | Media Service | No |
| Settings | `get_app_version` | Command Gateway | No |
| Settings | `get_settings` | Storage Service | No |
| Settings | `update_settings` | Storage Service | Yes |
| Integrations | `get_integration_statuses` | Integration Service + Native Broker | No setup/session mutation |
| Integrations | `install_integration` | Integration Service | Provider setup; GNOME currently refuses |
| Intelligence | `get_speech_model_status` | Local Speech Adapter | No |
| Intelligence | `install_speech_model` | Local Speech Adapter | Local model state |
| Intelligence | `remove_speech_model` | Local Speech Adapter | Local model state |
| Intelligence | `retry_voice_transcription` | Enrichment Service | Explicit pending job |
| Library | `update_voice_caption` | Enrichment Service | Caption revision and FTS |

## Contract Verification

- Generate or compile TypeScript bindings from Rust contract types in CI.
- Run serialization round-trip tests for every shared type.
- Run negative tests for unknown fields, invalid enums, malformed UUIDs/timestamps, stale sessions, duplicate saves, path traversal attempts, and over-limit pages.
- Assert that no core command accepts a URL, SQL statement, shell string, absolute media path, or raw clipboard/audio payload.
- Verify every `ErrorCode` renders an actionable UI state and no error details leak sensitive content.
- Exercise concurrent VS Code windows, integrated and external terminals, multiple coding-agent working directories, and Git worktrees; the pre-popup foreground source must win over unrelated recency.
- Prove source selection and stale-source failures preserve text, staged-media identity, recording state, and the active session ID.
- Assert live-source events/options reveal no terminal/editor/agent content and that observations are absent from persisted data.
- Treat any command rename or incompatible payload change as a coordinated contract migration.
