# Context Provider Feasibility & Design — Browser Context Provider (G1-Ext)

**Status:** Implemented on Linux X11. Desktop Chromium and Firefox two-tab localhost proof confirmed 2026-09-18.

**Date:** 2026-09-18.

**Target:** Linux (X11 & Wayland-ready), Windows, macOS. Primary reference desktop: Pop!_OS 22.04 LTS, GNOME on X11.

## Summary

Lyn extends its local context provider architecture to web browsers (Google Chrome, Chromium, Brave, Microsoft Edge, and Mozilla Firefox). When a user captures a note or screenshot while viewing a web application, Lyn binds the capture to the relevant working context without exposing private browsing data, search history, or cloud accounts.

In particular, for developers testing local web services (`http://localhost:<port>` or `http://127.0.0.1:<port>`), Lyn correlates the focused tab's localhost listening port back to the same-user operating system process only when that port maps to exactly one working directory. IPv4 and IPv6 sockets of the same directory count as one source. `docker-proxy` and multiple distinct cwds are not project evidence. A remote site without that local proof stays unresolved.

## Architecture

```
┌────────────────────────────────────────────────────────┐
│                   Browser (Chrome / Firefox)           │
│                                                        │
│  ┌────────────────────────┐                            │
│  │ Lyn Companion Extension│                            │
│  │ (Manifest V3)          │                            │
│  └───────────┬────────────┘                            │
│              │ stdin / stdout (Native Messaging JSON)  │
└──────────────┼─────────────────────────────────────────┘
               ▼
┌────────────────────────────────────────────────────────┐
│  Lyn Native Host Helper (`lyn-browser-host`)           │
│  ~/.config/google-chrome/NativeMessagingHosts/...      │
└──────────────┬─────────────────────────────────────────┘
               │ Private Unix Domain Socket (mode 0600)
               │ (`$XDG_RUNTIME_DIR/lyn-browser-v1.sock`)
               ▼
┌────────────────────────────────────────────────────────┐
│  Lyn Desktop Core (Rust)                               │
│  - Receives v1 observe / v2 invoke                     │
│  - Unique localhost cwd (/proc/net/tcp{,6} -> cwd)     │
│  - Correlates pre-popup foreground window (X11)        │
│  - Ephemeral in-memory registry (never logs URLs)      │
└────────────────────────────────────────────────────────┘
```

## Invariants & Privacy Boundaries

1. **INV-14 — Invocation-Bound Automatic Context:**
   Observations are registered ephemerally in memory with an expiration lease. An observation only becomes an active candidate if the invoking window matches the browser process or window correlation token.
2. **Zero URL Logging or SQLite Persistence:**
   Browser URLs and page titles are never stored in `lyn.db`, full-text search indexes, or diagnostic logs. Once resolved to a Project Context (or Standalone Context), the URL is discarded.
3. **Query Parameter & Credential Stripping:**
   Before any URL leaves the browser extension, authentication tokens, session IDs, query strings (`?token=...`), titles, and sensitive fragments are removed. Only `file:` and localhost-family URLs are transmitted, as scheme, host, port, and sanitized path. Remote sites emit no URL.
4. **Strict Incognito / Private Window Exclusion:**
   Tabs opened in incognito or private browsing modes emit no URLs, titles, or workspace observations. An invoke from a private tab may still open capture as Required.
5. **Localhost Process Verification:**
   For local ports (`localhost`, `127.0.0.1`, `::1`), Lyn queries `/proc/net/tcp` and `/proc/net/tcp6`, maps listen inodes to same-user processes, skips `docker-proxy`, and accepts a directory only when every remaining listener shares one cwd. The first matching PID is never chosen by itself.
6. **Source-tab invoke:**
   `lyn.capture` sends protocol v2 `{kind: invoke, requestId}` over Native Messaging while the tab still has focus. Chromium cannot register `Ctrl+Alt+Shift+L` as a suggested command, so the companion suggests `Alt+Shift+L` (rebindable in the browser). Firefox MV3 loads `background.scripts` rather than a service worker. Window identity is assigned by Rust. Replayed `requestId` values are ignored. The Native Messaging helper shuts down its Unix write side after the JSON payload so the broker can bound the read.

## Evidence Ranking & Tie-Breaking

The default provider tie-break order is updated non-destructively:
1. `vscode` (VS Code)
2. `cursor` (Cursor)
3. `browser` (Browser)
4. `shell` (Terminal)
5. `foreground_window` (Foreground window)

Existing user settings in SQLite are automatically migrated without discarding custom preferences.
