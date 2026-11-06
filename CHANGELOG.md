# Changelog

## 1.0.0

### Changed

- Refreshed interface. Inter is bundled locally, with one type scale and one
  set of button, select, switch and segmented controls. Light and dark themes
  now follow both the system setting and an explicit choice in Settings.
- Quick capture shows the detected project, Git branch and source app as chips
  above the draft. Screenshot, Voice, keyboard hints and Save share one footer,
  and Save is the primary action.
- Library groups captures by day (Today, Yesterday, then dates) in grouped
  lists. Toolbars keep one height. Project pages filter by branch next to the
  title. The capture inspector opens only when a capture is selected and offers
  copy, open and delete as compact tools.
- Settings uses aligned rows, a real "Start Lyn when I log in" switch, one
  grouped integrations list with clear status, and plainer English and French
  copy. The section is now called Integrations.

### Added

- `Ctrl+K` opens search from anywhere in the Library, or refocuses it.

### Fixed

- Saving no longer reports "Context source stale" for a terminal that is still
  open in the same project. Lyn identified the reporting process by its
  `/proc/<pid>` inode, which the kernel reassigns to a running process after
  ordinary memory reclaim. Kitty, shell-helper and native GNOME Terminal sources
  now use the process start time; closed terminals and real directory changes
  are still refused.
- Forced Dark or Light theme now draws image outlines in the right color, and
  the context reorder arrows in Settings stay readable before hover.

### Distribution and validation limits

- Packages remain unsigned prereleases built by the release workflow.
- Frontend, Rust and GNOME native/desktop fixtures passed locally. Reclaim
  regressions run as opt-in tests inside a memory-limited user scope.
- Native GNOME compatibility and installed acceptance limits are unchanged and
  remain documented in `docs/gnome_native_protocol.md`. Local Parakeet speech
  remains Linux x86_64 only.

## 0.9.1

### Changed

- Replace Whisper base with optional local Parakeet TDT 0.6B v3 Q8 on Linux
  x86_64. Install the approximately 942 MB package in Settings, then enable
  automatic transcription. Existing Whisper files are preserved; the replacement
  requires a new download and explicit enablement.
- Save voice recordings immediately and generate captions afterward. Load one
  model on demand, reuse it for nearby notes and unload after 60 seconds idle.
  Disabling local speech cancels active inference. No cloud fallback is used.

### Added

- Editable voice captions in Library, with revision checks protecting manual
  corrections from late automatic results. Captions remain limited to 500
  characters; full transcripts are not stored separately.
- Explicit retry for failed or interrupted transcription. Failures preserve the
  recording and are never replayed automatically.

### Fixed

- Keep the experimental Windows installer buildable while local Parakeet remains
  Linux-only. The initial 0.9.0 tag failed compilation and was not published.

### Distribution and validation limits

- The optional parakeet.cpp runtime is MIT-licensed; NVIDIA's Parakeet v3 model
  and its converted Q8 artifact are CC BY 4.0. The installed speech package retains
  attribution and license text. Model weights are downloaded separately from Lyn.
- Local frontend/Rust checks, worker/native-interface fixtures and a native ABI
  probe passed. Installed French/English recognition quality, latency, peak RAM
  and microphone acceptance remain pending joint testing.
- Parakeet is currently available on Linux x86_64 only. The Windows installer
  retains core capture and Library support; local Parakeet is unavailable there.
- Packages remain unsigned prereleases. Existing GNOME native compatibility and
  installed acceptance limits remain documented in `docs/gnome_native_protocol.md`.

## 0.8.0

### Added

- Optional “Start Lyn when I log in” setting. Login launches keep Library hidden;
  opening Lyn normally shows Library while capture integrations stay available
  in the background. Enable this option in Settings after installing.

### Changed

- Integration shortcuts use Alt+L. VS Code/Cursor and browser extensions need
  updating; existing custom browser and terminal mappings may require adjustment.
  The global capture shortcut remains Ctrl+Shift+Space by default.

### Validation limits

- Linux startup registration and background launch have real GUI evidence in an
  isolated session. On installed 0.8.0, the package identity and startup entry were
  verified; the user confirmed Alt+L works while Lyn runs in the background,
  without manually opening Library. This is user-reported shortcut acceptance.
- Actual logout/login execution remains unconfirmed. The installed controller
  stopped on minimized-window reopening/focus checks before testing the shortcut;
  it does not establish automated installed reopening or shortcut acceptance.
- Windows/macOS startup support has not been runtime-validated.
