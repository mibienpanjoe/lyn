# Native GNOME metadata protocol

**Status:** Accepted implementation contract for Linux/X11. The Rust codec and invocation-correlation rules are implemented and unit-tested; the GTK/VTE module remains a developer-only prototype. The production socket, kernel attestation, capture wiring, installer and packaged acceptance are not delivered yet. This page does not expand current provider support.

## Boundary and lifecycle

The dedicated Rust-owned Unix stream socket is `lyn-gnome-v1.sock` inside Lyn's private runtime directory. It must not accept editor/browser JSON or the prototype's version-0 datagrams. No UI IPC command exposes this channel. Only metadata crosses it: no paths, titles, commands, output, environment values, clipboard, buffers, terminal stream or file descriptors.

The broker must authenticate the same-user peer using kernel credentials, pin the GNOME Terminal server executable and process lifetime, and cross-check the frozen X11 window owner against that server. An executable basename or a payload PID is insufficient. The candidate PTY must belong to that server, and the reported foreground process group must match the kernel terminal/process identities. These checks are mandatory before registering a source; decoding a `Candidate` alone never resolves context.

Handshake order is module `Hello` → Rust `Bind` → module `Ready`. Rust creates a fresh non-nil UUID generation for each authenticated connection; `Ready` must echo it before queries are permitted. Reconnection invalidates all pending requests and source identities of the previous generation. A module source ID is nonzero, monotonic within the server lifetime, and never reused there; Rust scopes it by server identity and connection generation. No live identity is persisted.

Rust creates a distinct request UUID per invocation and freezes the pre-popup XID and monotonic invocation time. The module only answers queries; it cannot request capture, change focus or execute a command. One pending request permits one reply attempt, including refusal. Wrong kind/identity, replay, expiration, disconnect and incompatible capability/version fail closed. The broker must bound authentication, reads and writes, cap connections and pending queries, and never hold the capture-session mutex while waiting on this channel. GTK must never block on socket I/O. These transport/resource rules require broker/module integration tests before activation.

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

The Rust contract is [`gnome_protocol.rs`](../src-tauri/src/context/gnome_protocol.rs); its tests include an independent network-order query vector. A C/Rust shared-vector and real-channel test remain required before connecting the producer.

## Invocation proof and latency

Use Linux `CLOCK_MONOTONIC`, converted to microseconds in both processes, not wall time or serialized Rust `Instant`. GLib's target implementation of [`g_get_monotonic_time`](https://docs.gtk.org/glib/func.get_monotonic_time.html) uses this clock; the [GLib 2.72.4 implementation](https://github.com/GNOME/glib/blob/2.72.4/glib/gmain.c) is the reference for the current host. The dedicated Linux clock adapter still needs runtime comparison with the native producer.

For a candidate, Rust requires:

- Live connection generation, reply generation, request UUID and frozen window all match the pending invocation.
- `registration ≤ change < invocation ≤ observation ≤ receipt < invocation + 120000 µs`. Equality of change and invocation is refused because microsecond precision cannot prove their order. Deadline arithmetic must not overflow.
- The source remains an active terminal **within that frozen window**. No current global-focus check can replace it after Lyn opens.
- Every relevant topology transition updates the source revision/time on the GTK thread: map, unmap, toplevel hierarchy and PTY replacement. A→B→A after invocation is a change even if a later poll sees A again. Missing history or registration after invocation is unresolved.

The native prototype exercises these transitions in a real isolated server, including detach, TUI and loss of OS focus without a tab change. That proves native timing primitives, not production consumer wiring. A post-query topology change must also be caught by source revalidation before selection/save. A later native answer cannot override explicit context correction (INV-15) or a different capture generation; existing session authority remains unchanged.

The **120 ms native reply deadline** is an internal refusal bound, not a measured latency promise. The new native context acceptance target is warm p95 ≤ 250 ms, measured separately from the existing shortcut-to-input-ready p95 ≤ 250 ms target. Context failure must not prevent input/manual capture. Packaged measurements, including cold and absent/disconnected module cases, remain required.

## Shell and TUI policy

The module supplies a foreground **process group**, not a claimed cwd or a universally interchangeable PID. Rust must verify same-user process lifetimes, server ancestry and controlling PTY, deriving directories/Git itself. For a shell, exec TUI or child TUI, use the foreground group only when its eligible members yield one canonical cwd/project. Several distinct directories, an exited/recycled group, missing ancestry/PTY proof, SSH, tmux or container indirection without an exact local relationship remain unresolved. Do not substitute a remembered shell cwd for the foreground TUI.

Native topology timing proves the tab identity, not a historical cwd or foreground process state at nanosecond precision. Kernel process identity and project directory require consistent live reads and revalidation at selection/save. A closed source or changed project returns `CONTEXT_SOURCE_STALE` without finalizing staged media or modifying the draft. If invocation-time process provenance cannot be established, require selection rather than claim an exact automatic context.

## Verification boundary

Rust unit tests cover canonical framing, versions/capabilities, identities, timing, wrong correlation, reconnect generation mismatch, single-use replies, unavailable outcomes and same-microsecond refusal. They do not prove peer authentication, handshake transport order, PTY ownership, source registration, C encoding, installed status or successful capture. Those remain explicit integration requirements. Existing editor/browser parsers continue rejecting client-supplied native IDs; no shared permission or UI contract is widened.
