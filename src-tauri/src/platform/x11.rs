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
            .get_webview_window("main")
            .ok_or(PlatformError::FocusFailed)?;
        window.show().map_err(|_| PlatformError::FocusFailed)?;
        window.set_focus().map_err(|_| PlatformError::FocusFailed)
    }

    fn hide_capture_popup(&mut self) -> Result<(), PlatformError> {
        self.app
            .get_webview_window("main")
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

fn active_window() -> Result<u32, PlatformError> {
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
