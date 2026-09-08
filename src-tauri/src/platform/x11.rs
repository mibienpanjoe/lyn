use std::sync::Mutex;

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
    Cursor,
    Browser,
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
        if is_lyn_window(window) {
            return Err(PlatformError::Unsupported);
        }
        Ok(ForegroundWindowIdentity {
            window: WindowCorrelationToken::from_native(u64::from(window)),
        })
    }

    fn show_capture_popup(&mut self) -> Result<(), PlatformError> {
        let window = match self.app.get_webview_window("capture") {
            Some(w) => w,
            None => tauri::WebviewWindowBuilder::new(
                &self.app,
                "capture",
                tauri::WebviewUrl::App("index.html?surface=capture".into()),
            )
            .title("Lyn")
            .inner_size(640.0, 210.0)
            .min_inner_size(520.0, 200.0)
            .resizable(true)
            .visible(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .build()
            .map_err(|_| PlatformError::FocusFailed)?,
        };
        let _ = window.unminimize();
        window.show().map_err(|_| PlatformError::FocusFailed)?;
        let _ = window.set_focus();
        Ok(())
    }

    fn hide_capture_popup(&mut self) -> Result<(), PlatformError> {
        if let Some(window) = self.app.get_webview_window("capture") {
            window.hide().map_err(|_| PlatformError::FocusFailed)?;
        }
        Ok(())
    }

    fn restore_foreground(
        &mut self,
        identity: ForegroundWindowIdentity,
    ) -> Result<(), PlatformError> {
        activate_window(identity.window.native() as u32)
    }
}

pub(crate) fn is_lyn_window(window: u32) -> bool {
    window_class(window).is_some_and(|class| is_lyn_class(&class))
}

fn window_class(window: u32) -> Option<Vec<u8>> {
    let (connection, _) = x11rb::connect(None).ok()?;
    Some(
        connection
            .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 64)
            .ok()?
            .reply()
            .ok()?
            .value,
    )
}

fn is_lyn_class(window_class: &[u8]) -> bool {
    window_class
        .split(|byte| *byte == 0)
        .any(|part| part.eq_ignore_ascii_case(b"lyn"))
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

pub(crate) fn active_editor_window() -> Result<(u32, ContextWindowKind), PlatformError> {
    let (window, kind) = active_context_window()?;
    if kind == ContextWindowKind::Vscode || kind == ContextWindowKind::Cursor {
        Ok((window, kind))
    } else {
        Err(PlatformError::Unsupported)
    }
}

pub(crate) fn active_browser_window() -> Result<(u32, ContextWindowKind), PlatformError> {
    let (window, kind) = active_context_window()?;
    if kind == ContextWindowKind::Browser {
        Ok((window, kind))
    } else {
        Err(PlatformError::Unsupported)
    }
}

static LAST_ACTIVE_CONTEXT_WINDOW: Mutex<Option<(u32, ContextWindowKind)>> = Mutex::new(None);

pub(crate) fn active_context_window() -> Result<(u32, ContextWindowKind), PlatformError> {
    let (connection, _) = x11rb::connect(None).map_err(|_| PlatformError::Unsupported)?;
    let window = active_window()?;
    let window_class = connection
        .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 64)
        .map_err(|_| PlatformError::Unsupported)?
        .reply()
        .map_err(|_| PlatformError::Unsupported)?
        .value;
    remember_active_context_window(window, &window_class)
}

// Showing Lyn necessarily moves OS focus away from the observed window, so a
// Lyn window keeps the last observed context window authoritative instead of
// silently expiring the bounded observation the capture was invoked from.
fn remember_active_context_window(
    window: u32,
    window_class: &[u8],
) -> Result<(u32, ContextWindowKind), PlatformError> {
    if is_lyn_class(window_class) {
        return LAST_ACTIVE_CONTEXT_WINDOW
            .lock()
            .ok()
            .and_then(|last| *last)
            .ok_or(PlatformError::Unsupported);
    }
    let active = context_window_kind(window_class).map(|kind| (window, kind));
    if let Ok(mut last) = LAST_ACTIVE_CONTEXT_WINDOW.lock() {
        *last = active;
    }
    active.ok_or(PlatformError::Unsupported)
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
            } else if part.eq_ignore_ascii_case(b"cursor") {
                Some(ContextWindowKind::Cursor)
            } else if part.eq_ignore_ascii_case(b"gnome-terminal")
                || part.eq_ignore_ascii_case(b"gnome-terminal-server")
            {
                Some(ContextWindowKind::GnomeTerminal)
            } else if part.eq_ignore_ascii_case(b"kitty") {
                Some(ContextWindowKind::Kitty)
            } else if part.eq_ignore_ascii_case(b"google-chrome")
                || part.eq_ignore_ascii_case(b"chromium")
                || part.eq_ignore_ascii_case(b"chromium-browser")
                || part.eq_ignore_ascii_case(b"brave-browser")
                || part.eq_ignore_ascii_case(b"microsoft-edge")
                || part.eq_ignore_ascii_case(b"microsoft-edge-dev")
                || part.eq_ignore_ascii_case(b"firefox")
                || part.eq_ignore_ascii_case(b"navigator")
            {
                Some(ContextWindowKind::Browser)
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
    use std::sync::Mutex;

    use super::{
        ContextWindowKind, LAST_ACTIVE_CONTEXT_WINDOW, PlatformError, context_window_kind,
        remember_active_context_window,
    };

    static ACTIVE_WINDOW_MEMO_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn accepts_supported_vscode_and_cursor_window_classes() {
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
        assert_eq!(
            context_window_kind(b"cursor\0Cursor\0"),
            Some(ContextWindowKind::Cursor)
        );
        assert_eq!(
            context_window_kind(b"google-chrome\0Google-chrome\0"),
            Some(ContextWindowKind::Browser)
        );
        assert_eq!(
            context_window_kind(b"chromium\0Chromium\0"),
            Some(ContextWindowKind::Browser)
        );
        assert_eq!(
            context_window_kind(b"brave-browser\0Brave-browser\0"),
            Some(ContextWindowKind::Browser)
        );
        assert_eq!(
            context_window_kind(b"firefox\0Firefox\0"),
            Some(ContextWindowKind::Browser)
        );
        assert_ne!(
            context_window_kind(b"terminal\0kitty\0"),
            Some(ContextWindowKind::Vscode)
        );
        assert_ne!(
            context_window_kind(b"terminal\0kitty\0"),
            Some(ContextWindowKind::Cursor)
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
            context_window_kind(b"cursor\0Cursor\0"),
            Some(ContextWindowKind::Cursor)
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

    #[test]
    fn lyn_focus_keeps_the_pre_popup_context_window_authoritative() {
        let _guard = ACTIVE_WINDOW_MEMO_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *LAST_ACTIVE_CONTEXT_WINDOW.lock().unwrap() = None;

        assert_eq!(
            remember_active_context_window(42, b"kitty\0kitty\0"),
            Ok((42, ContextWindowKind::Kitty))
        );
        assert_eq!(
            remember_active_context_window(7, b"lyn\0Lyn\0"),
            Ok((42, ContextWindowKind::Kitty))
        );
        assert_eq!(
            remember_active_context_window(9, b"cursor\0Cursor\0"),
            Ok((9, ContextWindowKind::Cursor))
        );
        assert_eq!(
            remember_active_context_window(7, b"lyn\0Lyn\0"),
            Ok((9, ContextWindowKind::Cursor))
        );
    }

    #[test]
    fn unsupported_focus_clears_the_remembered_context_window() {
        let _guard = ACTIVE_WINDOW_MEMO_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *LAST_ACTIVE_CONTEXT_WINDOW.lock().unwrap() = None;

        assert!(remember_active_context_window(42, b"kitty\0kitty\0").is_ok());
        assert_eq!(
            remember_active_context_window(11, b"nautilus\0Nautilus\0"),
            Err(PlatformError::Unsupported)
        );
        assert_eq!(
            remember_active_context_window(7, b"lyn\0Lyn\0"),
            Err(PlatformError::Unsupported)
        );
    }
}
