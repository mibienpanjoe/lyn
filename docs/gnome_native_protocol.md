# Native GNOME metadata protocol

**Status:** Accepted implementation contract for Linux/X11. The Rust/C codecs and invocation-correlation rules are tested; the developer-only GTK/VTE module also answers v1 queries through the actual Rust broker in an isolated real GNOME server. Native peer admission, X-Resource window ownership and live PTY/foreground-group directory checks use the production policy. Application startup, source registration/revalidation, capture wiring, installer and packaged acceptance remain pending. This page does not expand current provider support.

## Boundary and lifecycle

The dedicated Rust-owned Unix stream socket is `lyn-gnome-v1.sock` inside Lyn's private runtime directory. It must not accept editor/browser JSON or the prototype's version-0 datagrams. No UI IPC command exposes this channel. Only metadata crosses it: no paths, titles, commands, output, environment values, clipboard, buffers, terminal stream or file descriptors.

The broker must authenticate the same-user peer using kernel credentials, pin the GNOME Terminal server executable and process lifetime, and cross-check the frozen X11 window owner against that server. An executable basename or a payload PID is insufficient. The candidate PTY must belong to that server, and the reported foreground process group must match the kernel terminal/process identities. These checks are mandatory before registering a source; decoding a `Candidate` alone never resolves context.

The Rust [native peer primitive](../src-tauri/src/context/gnome_peer.rs) implements the first admission boundary: kernel `SO_PEERCRED`, same effective uid, and executable device/inode equality with a root-owned, non-group/world-writable GNOME server at an allowlisted system path. It pins a `pidfd` plus proc inode, uid, executable inode and starttime, and rejects exit or identity change on revalidation. `pidfd_open` support (Linux 5.3+) is required; permission/kernel failure leaves the native provider unavailable, with no weaker PID-only fallback. The pidfd is only polled, never read, signalled, or used to duplicate a process FD. Peer admission alone does not prove the PTY/process associations checked by the broker paths below.

Handshake order is module `Hello` → Rust `Bind` → module `Ready`. Rust creates a fresh non-nil UUID generation for each authenticated connection; `Ready` must echo it before queries are permitted. Reconnection invalidates all pending requests and source identities of the previous generation. A module source ID is nonzero, monotonic within the server lifetime, and never reused there; Rust scopes it by server identity and connection generation. No live identity is persisted.

The C [channel](../integrations/gnome/native_channel.c) uses nonblocking GLib FD watches, one 112-byte input and output frame, a 500 ms initial handshake limit and 120 ms partial-read/output limits. It checks the broker peer uid, closes on wrong generation/kind/framing, and refuses output backlog rather than blocking GTK. The prototype enables it only with a private harness endpoint; automatic production endpoint discovery and reconnection are not enabled. These C-side checks do not replace Rust-side native peer/window/PTY attestation. Query traversal is capped at 2048 widgets and depth 32; reaching a limit returns unavailable, never a partial exact candidate.

Rust creates a distinct request UUID per invocation and freezes the pre-popup XID and monotonic invocation time. The module only answers queries; it cannot request capture, change focus or execute a command. One pending request permits one reply attempt, including refusal. Wrong kind/identity, replay, expiration, disconnect and incompatible capability/version fail closed. The broker must bound authentication, reads and writes, cap connections and pending queries, and never hold the capture-session mutex while waiting on this channel. GTK must never block on socket I/O. These transport/resource rules require broker/module integration tests before activation.

The Rust [stream channel](../src-tauri/src/context/gnome_channel.rs) implements the 500 ms absolute handshake and invocation-relative 120 ms exchange deadlines with nonblocking sockets and `poll`. Partial input, interruptions and byte-by-byte delivery do not reset the deadline; a future invocation timestamp is rejected before any request is sent. It revalidates the peer around exchanges, checks unsolicited trailing input, and rejects ancillary data with no FD receive buffer. Protocol, transport or correlation errors close/drop the socket and invalidate the generation. A correlated explicit missing/changed/ambiguous/unavailable response permits a new request UUID on the same connection. Exclusive mutable ownership allows only one exchange at a time. This component has socket-pair/fixture-peer tests; it does not yet listen, manage multiple server connections, attest a PTY or register a source.

The Rust [listener primitive](../src-tauri/src/context/gnome_listener.rs) binds a mode-0600 socket in an absolute same-user private directory, rejecting a symlink final component even with trailing separators. It never removes an occupied endpoint; shutdown removes only its recorded socket inode and preserves a replacement. Its pending listen backlog is 16 (Linux may admit one additional queued connection), separate from the active-channel limit. Idle accept blocks on readiness and an explicit stop/handle-drop wakeup, with no periodic timer. Accepted streams remain unauthenticated until passed to the native peer/channel boundary. Listener tests cover permissions, occupied/replaced paths, cancellation, close-on-exec and queue saturation.

The Rust [connection manager](../src-tauri/src/context/gnome_broker.rs) composes these primitives on one admission worker, with one bounded handshake and at most eight admitted server PIDs. Its public Rust query path derives the server from the frozen window's XRes owner as described below; there is no fallback to another peer. It never holds the registry mutex across peer checks or stream exchanges, and a busy channel immediately refuses a second query. Idle reconnect replaces and closes the old generation; a busy reconnect is refused. Dead idle channels are reaped on admission, while fatal query failures retire only the matching slot. Returned evidence includes the authenticated PID and connection generation, but still requires PTY/process attestation and later source revalidation. Shutdown marks the manager unavailable, wakes accept and joins the worker; a pending handshake may consume its remaining 500 ms budget. Unit tests use real sockets and same-executable fixture peers, including distinct subprocess PIDs for capacity. The separate native harness exercises real GNOME admission/exchanges, not application startup/capture wiring.

## X11 window ownership

The Rust [window-owner adapter](../src-tauri/src/platform/x11_owner.rs) uses [X-Resource 1.2](https://sources.debian.org/src/xorgproto/2025.1-1/resproto.txt), not `_NET_WM_PID`. It verifies a live Window, maps it to exactly one server-advertised client resource range, and requires one canonical positive `LOCAL_CLIENT_PID` for that base. The client base is not necessarily the window XID: the current Xorg/Xvfb returns the base even when queried with a window. Duplicate/overlapping ranges, unsupported negotiated versions, malformed identities, absent extensions and remote/TCP transports stay unresolved. A second live-window check refuses an unused XID that merely falls inside a client's range.

Connections use local Unix sockets, kernel peer uid root/current user before sending authentication, nonblocking I/O and one absolute 100 ms wire deadline covering setup and requests. Authentication uses x11rb's existing trusted local Xauthority configuration; its filesystem lookup is not a hostile-network input or a hard filesystem-latency guarantee. No arbitrary path enters UI IPC. The adapter rejects ancillary descriptors and never inspects terminal streams.

`NativeBroker::query_window` derives the server PID from XRes, exchanges metadata with that admitted peer, then repeats ownership verification and fences the current connection slot/generation and peer identity. The raw-PID exchange is private; only test controls exercise it separately. Reconnection during the second owner check must not return evidence from the retired generation. These checks establish window/server association, not PTY ownership or an authoritative project directory. Runtime tests cover spoofed PID properties, foreign and closed windows with a still-live GNOME client, and timeout/recovery while only the private Xvfb is stopped. They are not packaged latency measurements or installed capture acceptance.

## Kernel master-PTY observation

`NativeBroker::query_window_pty` adds a metadata-only cross-check through
[`gnome_pty.rs`](../src-tauri/src/context/gnome_pty.rs). It requires a matching
`tty-index` in the authenticated server's bounded `/proc/PID/fdinfo` scan,
stats the descriptor target without opening it, and checks the PTMX device,
inode and supported devpts route. At most 4096 descriptors and 16 KiB per
metadata file are inspected; work is refused after the original invocation's
120 ms deadline. Local filesystem operations are not preemptible hard deadlines.

Both `/dev/pts/ptmx` and the root-owned legacy `/dev/ptmx` node are supported.
The latter uses the kernel's documented [sibling `pts` lookup](https://docs.kernel.org/filesystems/devpts.html),
not equality of master/slave filesystem devices. Mount/user/PID namespaces and
process roots must match the broker; other paths, chroots and duplicate masters
remain unresolved. Root-managed device nodes/mount topology are trusted;
privileged remount attacks are outside this metadata-only check's threat model.

Inode/index/descriptor values can recycle: they are observations, not terminal
lifetime identities. After the kernel reads, another query of the same frozen
window and **original** invocation must confirm the same source/revision/PTY,
followed by owner and current-channel fences. Later selection/save still needs
fresh source validation. Real isolated GNOME tests exercise this Rust path for
three concurrent windows including a TUI; unit tests cover ownership, closure,
duplicates, metadata limits and confirmation ordering. This does **not** yet
itself prove foreground-group ancestry or cwd/Git, register a source or wire capture.

## Foreground group and local directory

[`gnome_process.rs`](../src-tauri/src/context/gnome_process.rs) treats the native
PGID as a group, not an interchangeable PID. It scans bounded kernel stat
metadata, refusing foreign-uid live members in that group rather than selecting
only the readable same-user subset. Every member must have the owned slave's
controlling TTY, the reported foreground group, one session and canonical cwd,
and a same-user ancestry reaching the admitted server. Process roots and
mount/user/PID namespaces must match the broker. Standard system SSH/multiplexer/
container proxy executable inodes are refused; this is not exhaustive detection
of copied/custom remote clients or a claim to resolve their remote directory.

`NativeBroker::query_window_process` compares two complete snapshots (including
proc/starttime, executable and directory identities) before final native
source/revision confirmation and owner/generation checks. It retains pidfd pins
for members/ancestors and polls them again before returning, then checks the
original deadline again. Bounds are 8192 proc entries, 64 members, 64 ancestry
steps per member, 256 retained pins and 4096 bytes per stat file, under the
original 120 ms refusal budget. Local filesystem calls are not preemptible.

`ProcessInvocation::now` must run at the Rust invocation boundary **before**
popup or owner/socket work. It freezes a BOOTTIME lower bound before sampling
MONOTONIC; it never reconstructs an earlier invocation using a later suspend
offset. The [proc starttime](https://man7.org/linux/man-pages/man5/proc_pid_stat.5.html)
tick must entirely precede that frozen bound. The private test control mints this
clock on Rust receipt, not at the earlier Python dispatch. Production shortcut
wiring still needs this clock captured alongside the frozen window. Physical
suspend/resume remains a separate manual check.

The directory and pinned snapshot stay Rust-owned and ephemeral. These are
consistent live observations, not a historical cwd or a permanently valid
capture context. Git enrichment, source registration, selection/save validation
and the first saved capture remain integration work. Real process fixtures cover
wrong TTY/ancestry, exited lifetimes, coherent two-member groups and distinct-cwd
refusal; the real GNOME harness checks the directory derived by Rust without
returning raw paths through its cfg(test)-only control endpoint.

## Fixed frame layout

Each frame is exactly **112 bytes**, integer fields in network byte order. UUIDs use their 16 RFC bytes. There is no string, variable-length body, native C struct serialization, ancillary FD, or extensible field bag. Stream readers assemble exactly one frame under a deadline; a truncated frame or invalid next frame terminates the connection rather than resynchronizing by scanning content.

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 4 | ASCII `LYGN` |
| 4 | 2 | Version `1` |
| 6 | 2 | Kind: Hello `1`, Bind `2`, Ready `3`, Query `4`, Reply `5` |
| 8 | 16 | Connection generation UUID |
| 24 | 16 | Request UUID |
| 40 | 4 | Frozen X11 window ID |
| 44 | 4 | PTY number (`0` is valid) |
| 48 | 4 | Foreground PGID, `1..INT32_MAX` |
| 52 | 8 | Module terminal source ID |
| 60 | 8 | Query invocation / reply observation time, monotonic microseconds |
| 68 | 8 | Last topology-change time, monotonic microseconds |
| 76 | 8 | Topology revision, nonzero |
| 84 | 8 | First native source registration time, monotonic microseconds |
| 92 | 2 | Handshake capabilities, exactly `3` (snapshot `1` + topology timing `2`) |
| 94 | 2 | Reply status |
| 96 | 16 | Reserved, zero |

All fields not used by a kind MUST be zero. Hello uses only capabilities; Bind/Ready add generation. Query uses generation, request, window and invocation time, all nonzero. Reply uses generation, request, window, observation time and status; status `1` (candidate) additionally uses the six source fields at offsets 44–60 and 68–92. Status `2` missing, `3` changed, `4` ambiguous and `5` unavailable carry no source fields. Unknown versions, kinds, capability bits, status, reserved values and noncanonical unused fields are rejected, not silently ignored. Source registration ≤ change ≤ observation, with positive times, is a frame-level requirement.

The codecs are [`gnome_protocol.rs`](../src-tauri/src/context/gnome_protocol.rs) and [`wire.c`](../integrations/gnome/wire.c). Both exercise the same ten valid and 22 refused [wire vectors](../integrations/gnome/wire_vectors.txt), including all kinds/statuses, 64-bit values, invalid identities/timing, capabilities and reserved fields. C decoding also checks bounded input and unchanged output on refusal. The isolated native test exercises the stream handshake, frozen-window queries, active TUI, topology-change refusals and generation disconnection. Its Python fixture broker is not the production Rust authority; sharing a wire format does not authenticate a peer.

## Invocation proof and latency

Use Linux `CLOCK_MONOTONIC`, converted to microseconds in both processes, not wall time or serialized Rust `Instant`. GLib's target implementation of [`g_get_monotonic_time`](https://docs.gtk.org/glib/func.get_monotonic_time.html) uses this clock; the [GLib 2.72.4 implementation](https://github.com/GNOME/glib/blob/2.72.4/glib/gmain.c) is the reference for the current host. The Rust channel implements the same clock conversion, but still needs runtime comparison with the real native producer.

For a candidate, Rust requires:

- Live connection generation, reply generation, request UUID and frozen window all match the pending invocation.
- `registration ≤ change < invocation ≤ observation ≤ receipt < invocation + 120000 µs`. Equality of change and invocation is refused because microsecond precision cannot prove their order. Deadline arithmetic must not overflow.
- The source remains an active terminal **within that frozen window**. No current global-focus check can replace it after Lyn opens.
- Every relevant topology transition updates the source revision/time on the GTK thread: map, unmap, toplevel hierarchy and PTY replacement. A→B→A after invocation is a change even if a later poll sees A again. Missing history or registration after invocation is unresolved.

The native prototype exercises these transitions and v1 query responses in a real isolated server, including detach, TUI and loss of OS focus without a tab change. That proves producer-side timing/correlation, not production consumer wiring. A closing VTE may temporarily be unavailable before its widget/window disappears; both unavailable and missing must remain refusals with zero source fields, never a replacement candidate. A post-query topology change must also be caught by source revalidation before selection/save. A later native answer cannot override explicit context correction (INV-15) or a different capture generation; existing session authority remains unchanged.

The **120 ms native reply deadline** is an internal refusal bound, not a measured latency promise. The new native context acceptance target is warm p95 ≤ 250 ms, measured separately from the existing shortcut-to-input-ready p95 ≤ 250 ms target. Context failure must not prevent input/manual capture. Packaged measurements, including cold and absent/disconnected module cases, remain required.

## Shell and TUI policy

The module supplies a foreground **process group**, not a claimed cwd or a universally interchangeable PID. Rust must verify same-user process lifetimes, server ancestry and controlling PTY, deriving directories/Git itself. For a shell, exec TUI or child TUI, use the foreground group only when its eligible members yield one canonical cwd/project. Several distinct directories, an exited/recycled group, missing ancestry/PTY proof, SSH, tmux or container indirection without an exact local relationship remain unresolved. Do not substitute a remembered shell cwd for the foreground TUI.

Native topology timing proves the tab identity, not a historical cwd or foreground process state at nanosecond precision. Kernel process identity and project directory require consistent live reads and revalidation at selection/save. A closed source or changed project returns `CONTEXT_SOURCE_STALE` without finalizing staged media or modifying the draft. If invocation-time process provenance cannot be established, require selection rather than claim an exact automatic context.

## Verification boundary

Rust tests cover canonical framing, versions/capabilities, identities, timing, wrong correlation, generation mismatch, single-use replies, unavailable outcomes, same-microsecond refusal, bounded stream exchange, extra input, ancillary FD refusal and socket invalidation. Shared C/Rust vectors prove encoding/decoding compatibility. The native harness covers both producer exchanges with a Python fixture and real GNOME → Rust peer/channel/listener/manager exchanges, including XRes, master-PTY and foreground-group directory checks. Its cfg(test)-only control endpoint uses the production broker constructor, not fixture authentication. It verifies metadata for concurrent windows and a TUI, no fallback for an unknown server, missing-window reuse, frozen-window retention after OS focus changes, same-microsecond refusal and revocation on actual server exit. Source registration, application startup, installed status and successful capture remain explicit integration requirements. Ordinary library tests alone do not run the real-GNOME probe. Existing editor/browser parsers continue rejecting client-supplied native IDs; no shared permission or UI contract is widened.
