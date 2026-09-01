use tauri::{AppHandle, Manager};
use x11rb::{
    connection::Connection,
    protocol::xproto::{AtomEnum, ClientMessageData, ClientMessageEvent, ConnectionExt, EventMask},
};

use super::{
    CaptureWindowPlatform, ForegroundWindowIdentity, PlatformError, WindowCorrelationToken,
};

pub(crate) struct X11CaptureWindowPlatform {
    app: AppHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextWindowKind {
    Vscode,
    GnomeTerminal,
    Kitty,
}

impl X11CaptureWindowPlatform {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl CaptureWindowPlatform for X11CaptureWindowPlatform {
    fn capture_foreground(&mut self) -> Result<ForegroundWindowIdentity, PlatformError> {
        let window = active_window()?;
        Ok(ForegroundWindowIdentity {
            window: WindowCorrelationToken::from_native(u64::from(window)),
        })
    }

    fn show_capture_popup(&mut self) -> Result<(), PlatformError> {
        let window = self
            .app
            .get_webview_window("capture")
            .ok_or(PlatformError::FocusFailed)?;
        window.show().map_err(|_| PlatformError::FocusFailed)?;
        window.set_focus().map_err(|_| PlatformError::FocusFailed)
    }

    fn hide_capture_popup(&mut self) -> Result<(), PlatformError> {
        self.app
            .get_webview_window("capture")
            .ok_or(PlatformError::FocusFailed)?
            .hide()
            .map_err(|_| PlatformError::FocusFailed)
    }

    fn restore_foreground(
        &mut self,
        identity: ForegroundWindowIdentity,
    ) -> Result<(), PlatformError> {
        activate_window(identity.window.native() as u32)
    }
}

pub(crate) fn active_window() -> Result<u32, PlatformError> {
    let (connection, screen_number) =
        x11rb::connect(None).map_err(|_| PlatformError::Unsupported)?;
    let root = connection
        .setup()
        .roots
        .get(screen_number)
        .ok_or(PlatformError::Unsupported)?
        .root;
    let active_window_atom = connection
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")
        .map_err(|_| PlatformError::Unsupported)?
        .reply()
        .map_err(|_| PlatformError::Unsupported)?
        .atom;
    connection
        .get_property(false, root, active_window_atom, AtomEnum::WINDOW, 0, 1)
        .map_err(|_| PlatformError::Unsupported)?
        .reply()
        .map_err(|_| PlatformError::Unsupported)?
        .value32()
        .and_then(|mut values| values.next())
        .filter(|window| *window != 0)
        .ok_or(PlatformError::Unsupported)
}

pub(crate) fn active_vscode_window() -> Result<u32, PlatformError> {
    let (window, kind) = active_context_window()?;
    if kind == ContextWindowKind::Vscode {
        Ok(window)
    } else {
        Err(PlatformError::Unsupported)
    }
}

pub(crate) fn active_context_window() -> Result<(u32, ContextWindowKind), PlatformError> {
    let (connection, _) = x11rb::connect(None).map_err(|_| PlatformError::Unsupported)?;
    let window = active_window()?;
    let window_class = connection
        .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 64)
        .map_err(|_| PlatformError::Unsupported)?
        .reply()
        .map_err(|_| PlatformError::Unsupported)?
        .value;
    context_window_kind(&window_class)
        .map(|kind| (window, kind))
        .ok_or(PlatformError::Unsupported)
}

fn context_window_kind(window_class: &[u8]) -> Option<ContextWindowKind> {
    window_class
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .find_map(|part| {
            if part.eq_ignore_ascii_case(b"code")
                || part.eq_ignore_ascii_case(b"code-insiders")
                || part.eq_ignore_ascii_case(b"codium")
                || part.eq_ignore_ascii_case(b"vscodium")
            {
                Some(ContextWindowKind::Vscode)
            } else if part.eq_ignore_ascii_case(b"gnome-terminal")
                || part.eq_ignore_ascii_case(b"gnome-terminal-server")
            {
                Some(ContextWindowKind::GnomeTerminal)
            } else if part.eq_ignore_ascii_case(b"kitty") {
                Some(ContextWindowKind::Kitty)
            } else {
                None
            }
        })
}

fn activate_window(window: u32) -> Result<(), PlatformError> {
    let (connection, screen_number) =
        x11rb::connect(None).map_err(|_| PlatformError::FocusFailed)?;
    let root = connection
        .setup()
        .roots
        .get(screen_number)
        .ok_or(PlatformError::FocusFailed)?
        .root;
    let active_window_atom = connection
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")
        .map_err(|_| PlatformError::FocusFailed)?
        .reply()
        .map_err(|_| PlatformError::FocusFailed)?
        .atom;
    let event = ClientMessageEvent::new(
        32,
        window,
        active_window_atom,
        ClientMessageData::from([1, 0, 0, 0, 0]),
    );
    connection
        .send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        )
        .map_err(|_| PlatformError::FocusFailed)?
        .check()
        .map_err(|_| PlatformError::FocusFailed)?;
    connection.flush().map_err(|_| PlatformError::FocusFailed)
}

#[cfg(test)]
mod tests {
    use super::{ContextWindowKind, context_window_kind};

    #[test]
    fn accepts_supported_vscode_window_classes_only() {
        assert_eq!(
            context_window_kind(b"code\0code\0"),
            Some(ContextWindowKind::Vscode)
        );
        assert_eq!(
            context_window_kind(b"code-insiders\0Code-insiders\0"),
            Some(ContextWindowKind::Vscode)
        );
        assert_eq!(
            context_window_kind(b"codium\0VSCodium\0"),
            Some(ContextWindowKind::Vscode)
        );
        assert_ne!(
            context_window_kind(b"terminal\0kitty\0"),
            Some(ContextWindowKind::Vscode)
        );
        assert_eq!(context_window_kind(b"lyn\0Lyn\0"), None);
    }

    #[test]
    fn classifies_only_supported_context_window_classes() {
        assert_eq!(
            context_window_kind(b"code\0Code\0"),
            Some(ContextWindowKind::Vscode)
        );
        assert_eq!(
            context_window_kind(b"gnome-terminal-server\0Gnome-terminal\0"),
            Some(ContextWindowKind::GnomeTerminal)
        );
        assert_eq!(
            context_window_kind(b"kitty\0kitty\0"),
            Some(ContextWindowKind::Kitty)
        );
        assert_eq!(context_window_kind(b"lyn\0Lyn\0"), None);
        assert_eq!(context_window_kind(b"\0"), None);
    }
}
