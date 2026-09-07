# Context Provider Feasibility & Design — Browser Context Provider (G1-Ext)

**Status:** Accepted for implementation.

**Date:** 2026-09-07.

**Target:** Linux (X11 & Wayland-ready), Windows, macOS. Primary reference desktop: Pop!_OS 22.04 LTS, GNOME on X11.

## Summary

Lyn extends its local context provider architecture to web browsers (Google Chrome, Chromium, Brave, Microsoft Edge, and Mozilla Firefox). When a user captures a note or screenshot while viewing a web application, Lyn binds the capture to the relevant working context without exposing private browsing data, search history, or cloud accounts.

In particular, for developers testing local web services (`http://localhost:<port>` or `http://127.0.0.1:<port>`), Lyn correlates the active localhost listening port back to the underlying operating system process, resolves its working directory on disk, and automatically binds the capture to the active local Git project and branch.

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
│  - Receives `BrowserObservationMessage`                │
│  - Localhost port mapper (/proc/net/tcp -> /proc/cwd)  │
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
   Before any URL leaves the browser extension, authentication tokens, session IDs, query strings (`?token=...`), and sensitive fragments are removed. Only scheme, host, port, and sanitized path are transmitted.
4. **Strict Incognito / Private Window Exclusion:**
   Tabs opened in incognito or private browsing modes emit no URLs, titles, or workspace observations.
5. **Localhost Process Verification:**
   For local ports (`localhost`, `127.0.0.1`), Lyn queries the kernel process table (`/proc/net/tcp` and `/proc/<pid>/cwd`) strictly within the same user's permission boundary.

## Evidence Ranking & Tie-Breaking

The default provider tie-break order is updated non-destructively:
1. `vscode` (VS Code)
2. `cursor` (Cursor)
3. `browser` (Browser)
4. `shell` (Terminal)
5. `foreground_window` (Foreground window)

Existing user settings in SQLite are automatically migrated without discarding custom preferences.
