# Lyn — Transition from Requirements to Architecture

Version: v1.1, 2026-08-28

Derived from: [`03_design_contract_invariant.md`](03_design_contract_invariant.md)

## Method

Each system invariant is assigned to exactly one conceptual component. The owner is responsible for making violation structurally difficult, exposing typed failure when enforcement is threatened, and supplying the primary tests for that guarantee. Supporting components may participate, but they do not share ownership.

Component names in this document are normative and match [`05_architecture.md`](05_architecture.md).

## Component Definitions

### Frontend Shell

The Svelte UI for the quick-capture popup, Library, search, media previews, and settings. It renders state and submits user intent through typed commands. It owns presentation behavior but no canonical persistence or system invariant.

### Command Gateway

The narrow Tauri IPC boundary into the Rust core. It validates payload shape, session identity, permissions, and path-free intent before delegating to domain services. It is the architectural point that prevents the WebView from acquiring broad OS, filesystem, database, or network authority.

### Capture Service

The coordinator for one active capture session. It validates capture content, provides idempotent save semantics, commits through Storage Service and Media Service, and determines the success response that allows the popup to close.

### Context Resolver

The provider-based resolver and ephemeral live-source registry for shell, VS Code, foreground-window, Git, and manual context evidence. It binds automatic detection to the pre-popup foreground window, exposes safe selectable sources, revalidates user selection, and produces exactly one project or standalone context snapshot with a named branch when available.

### Storage Service

The sole owner of SQLite access, schema migrations, transactions, canonical entities, settings persistence, and application-data location policy. It exposes domain methods rather than SQL and contains no remote persistence client.

### Media Service

The owner of image and audio staging, validation, encoding, finalization, playback handles, safe external-open operations, and scoped cleanup within Lyn-managed media directories.

### Library Service

The read model for chronological streams, filters, detail retrieval, and FTS5 search. It treats canonical Storage Service records as truth and the search index as rebuildable derived data.

### Enrichment Service

The asynchronous policy layer for generated screenshot metadata and voice transcription-derived captions. It schedules work only after capture acceptance and applies results only when the user has not supplied authoritative metadata.

### Local Speech Adapter

The optional adapter around whisper.cpp, its installed model, transcription execution, and model lifecycle. It returns results or typed failures to Enrichment Service and does not participate in save acceptance.

### Platform Service

Narrow adapters for global shortcut registration, capturing the pre-popup foreground identity, popup focus, clipboard access, active-window evidence, microphone streams, application lifecycle, and opening files with the operating system. Platform-specific code is kept behind stable Rust traits.

## Invariant Assignments

### Command Gateway owns INV-07 and INV-13

**INV-07 — Local core privacy boundary.** Command Gateway defines the complete callable surface. Core capture and retrieval commands contain no remote destination and cannot accept one from the UI. The sole network-capable action allowed by this design is an explicit, user-initiated speech-model download routed to a constrained model manager; it is not a core capture dependency.

**INV-13 — Narrow, validated privilege boundary.** All WebView requests cross this gateway. Command input uses domain identifiers and validated values rather than SQL, shell strings, or arbitrary filesystem paths.

### Capture Service owns INV-01, INV-10, INV-11, and INV-12

**INV-01 — Durable acceptance.** Capture Service is the only component permitted to return a successful save result. It does so only after the required Storage Service transaction and media finalization have succeeded.

**INV-10 — Text fidelity and titlelessness.** Capture Service validates text without rewriting it and maps the body directly to canonical storage. It exposes no title parameter for text capture.

**INV-11 — Cancellation non-persistence.** Capture Service owns session cancellation and asks Media Service to clean only the staged assets belonging to that session.

**INV-12 — Single-session, single-save semantics.** Capture Service issues session identifiers, serializes the active popup session, and uses the session identifier as an idempotency key for save.

### Enrichment Service owns INV-02 and INV-03

**INV-02 — Enrichment independence.** Enrichment Service accepts only already-committed capture identifiers. No save path waits for its output.

**INV-03 — User-authored metadata authority.** Enrichment Service reads the current caption source and revision before applying a generated result. A user-authored or newer caption causes the generated result to be discarded.

### Context Resolver owns INV-04, INV-05, INV-14, and INV-15

**INV-04 — Exactly one context.** Context Resolver returns either one resolved context or a `context_required` outcome. Capture Service cannot accept the save without the resolver's context identifier.

**INV-05 — Capture-time context snapshot.** Context Resolver materializes project identity and the current named branch into a value object passed to save. Provider state is never retained as a live link that could rewrite old captures.

**INV-14 — Invocation-bound automatic context.** Context Resolver ranks observations by verified association with the pre-popup foreground window, process, editor instance, or terminal session. Global provider recency cannot outrank exact invocation evidence.

**INV-15 — Explicit context correction authority.** Context Resolver exposes live and saved sources through opaque selection identifiers, revalidates a user-selected live source before save, and never mutates the capture draft while selection changes.

### Library Service owns INV-06 and INV-09

**INV-06 — One project chronology.** Library Service always scopes project ownership by `context_id`; branch is an optional predicate, never a parent resource.

**INV-09 — Search is derived and bounded.** Library Service controls the FTS projection, limits indexed fields to the MVP contract, and rebuilds it from Storage Service records.

### Media Service owns INV-08

**INV-08 — Media referential integrity.** Media Service alone moves staged bytes into final Lyn-owned locations and returns validated media metadata for commit. It validates media type and containment again when opening or playing an asset.

## Invariant Coverage Table

| Invariant | Single owner | Primary enforcement point | Primary verification |
|---|---|---|---|
| INV-01 Durable acceptance | Capture Service | Save result emitted only after transaction and media finalization | Failure-injection integration tests |
| INV-02 Enrichment independence | Enrichment Service | Post-commit work queue boundary | Slow/missing engine tests |
| INV-03 User-authored metadata authority | Enrichment Service | Caption source and revision compare before update | Race and stale-result tests |
| INV-04 Exactly one context | Context Resolver | Resolved-context result type and capture foreign key | Resolution and DB-constraint tests |
| INV-05 Capture-time context snapshot | Context Resolver | Immutable save-time value object | Branch-change regression tests |
| INV-06 One project chronology | Library Service | Project query scopes by context, then optional branch | Cross-branch listing tests |
| INV-07 Local core privacy boundary | Command Gateway | No remote operation in core command surface | Offline and egress-audit tests |
| INV-08 Media referential integrity | Media Service | Staged-to-final protocol and containment validation | Interrupted-write and missing-file tests |
| INV-09 Search is derived and bounded | Library Service | FTS projection and rebuild path | Index drift/rebuild tests |
| INV-10 Text fidelity and titlelessness | Capture Service | Text command has body but no title or rewrite step | Unicode/line-break round-trip tests |
| INV-11 Cancellation non-persistence | Capture Service | Session-scoped cancel and staging cleanup | Cancel-after-each-state tests |
| INV-12 Single-session, single-save semantics | Capture Service | Active-session lock and save idempotency key | Duplicate shortcut/save tests |
| INV-13 Narrow, validated privilege boundary | Command Gateway | Typed commands, validation, Tauri capabilities | Negative IPC and path traversal tests |
| INV-14 Invocation-bound automatic context | Context Resolver | Pre-popup window capture plus evidence-quality ranking | Multi-window/session correlation tests |
| INV-15 Explicit context correction authority | Context Resolver | Opaque selection, draft-preserving update, save-time revalidation | Source-switch and stale-source tests |

Coverage: 15 of 15 invariants have one owner; none are jointly owned.

## Requirements-to-Component Allocation

| Requirement group | Primary component | Supporting components |
|---|---|---|
| FR-010 Invocation and capture lifecycle | Capture Service | Frontend Shell, Command Gateway, Platform Service |
| FR-020 Context resolution | Context Resolver | Platform Service, Storage Service, Frontend Shell |
| FR-030 Text capture | Capture Service | Storage Service, Frontend Shell |
| FR-040 Screenshot capture | Media Service | Capture Service, Platform Service, Enrichment Service, Frontend Shell |
| FR-050 Voice capture | Media Service | Platform Service, Capture Service, Local Speech Adapter, Enrichment Service, Frontend Shell |
| FR-060 Library | Library Service | Storage Service, Media Service, Frontend Shell |
| FR-070 Search | Library Service | Storage Service, Frontend Shell |
| FR-080 Settings and local intelligence | Storage Service | Command Gateway, Enrichment Service, Local Speech Adapter, Frontend Shell |
| FR-090 Persistence and data ownership | Storage Service | Command Gateway, Media Service, Library Service |
| FR-100 Concurrent work-session context | Context Resolver | Platform Service, Command Gateway, Capture Service, Frontend Shell |

## Coupling and Cohesion Decisions

### Capture coordination is separate from persistence

Capture Service owns the user-visible transaction but not SQLite or files. This keeps save semantics explicit while preserving Storage Service as the only SQL owner and Media Service as the only media-byte owner.

### Context resolution uses providers, not platform conditionals in capture logic

Shell, editor, foreground-window, and manual strategies vary by platform and reliability. Context Resolver normalizes their evidence so Capture Service receives one stable result rather than learning every integration.

Provider identity and evidence quality are separate concerns. Exact invocation-bound evidence wins first; configurable provider order only resolves a tie between equally reliable candidates. This prevents a background editor or agent session from winning merely because it reported more recently.

### Live sources are ephemeral; contexts are persistent

VS Code windows, terminal tabs, shells, and coding-agent workspaces register ephemeral local observations with Context Resolver. Storage Service persists project/standalone contexts and capture snapshots, not live process/window records. Selecting a live source resolves it to a stored context plus branch; selecting a saved context bypasses live-source correlation.

### Media lifecycle is separated from optional enrichment

Saving, opening, and playing a file are core behaviors; describing or transcribing it is optional. Keeping Media Service and Enrichment Service separate makes “save first, enrich afterward” an architectural boundary rather than a timing convention.

### Library queries own FTS behavior

Storage Service owns canonical rows and transactions. Library Service owns how those rows are projected for browsing and search. The FTS table remains rebuildable and cannot become a second source of truth.

### Local Speech Adapter is replaceable and subordinate

whisper.cpp is an optional implementation choice. Enrichment Service depends on a speech interface, not on model-specific APIs, so the engine can be absent or replaced without affecting capture contracts.

### Platform access is capability-specific

Global shortcut, clipboard, active-window, audio, and external-open operations are grouped under Platform Service at the conceptual level but implemented as narrow adapters. They share an OS boundary, not a universal “do anything” interface.

### The Frontend Shell owns no invariant

The UI provides immediate feedback and accessible interaction, but correctness cannot depend on a mutable WebView. Every invariant is enforced in Rust before or below the IPC boundary.
