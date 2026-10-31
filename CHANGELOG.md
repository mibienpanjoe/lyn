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

- Linux startup registration and background launch have real GUI evidence in an
  isolated session. On installed 0.8.0, the package identity and startup entry were
  verified; the user confirmed Alt+L works while Lyn runs in the background,
  without manually opening Library. This is user-reported shortcut acceptance.
- Actual logout/login execution remains unconfirmed. The installed controller
  stopped on minimized-window reopening/focus checks before testing the shortcut;
  it does not establish automated installed reopening or shortcut acceptance.
- Windows/macOS startup support has not been runtime-validated.
