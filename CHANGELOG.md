# Changelog

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

- Linux startup has real GUI evidence in an isolated session. Actual login-manager
  execution and installed Alt+L acceptance remain pending. Windows/macOS startup
  support has not been runtime-validated.
