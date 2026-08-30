use std::sync::Mutex;

use tauri::{Emitter, Manager};

#[cfg(desktop)]
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

mod capture;
mod commands;
mod context;
pub mod contract;
mod enrichment;
pub mod error;
mod intelligence;
mod library;
mod media;
mod platform;
mod storage;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    invoke_capture_popup(app);
                })
                .build(),
        )
        .setup(|app| {
            let database_path = app.path().app_data_dir()?.join("lyn.db");
            let database = storage::Database::open(database_path)?;
            app.manage(Mutex::new(database));
            app.manage(Mutex::new(context::DirectorySelectionRegistry::default()));
            app.manage(Mutex::new(
                context::session_registry::ContextSourceRegistry::default(),
            ));
            app.manage(Mutex::new(
                capture::session::CaptureSessionService::default(),
            ));
            app.manage(Mutex::new(platform::InvocationContext::default()));
            #[cfg(desktop)]
            {
                // Settings persistence replaces this initial default in T27.
                let _ = app.global_shortcut().register("Control+Shift+Space");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::capture::get_active_capture_session,
            commands::capture::cancel_capture_session,
            commands::capture::list_capture_context_sources,
            commands::capture::select_capture_context_source,
            commands::capture::save_text_capture,
            commands::context::pick_project_directory,
            commands::context::create_context,
            commands::context::list_contexts,
            commands::platform::dismiss_capture_popup,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Lyn");
}

#[cfg(target_os = "linux")]
fn invoke_capture_popup(app: &tauri::AppHandle) {
    use crate::platform::CaptureWindowPlatform;

    let mut platform = platform::x11::X11CaptureWindowPlatform::new(app.clone());
    let foreground = platform.capture_foreground().ok();
    if let Ok(mut invocation) = app.state::<Mutex<platform::InvocationContext>>().lock() {
        invocation.record_foreground(foreground);
    }
    let session = app
        .state::<Mutex<capture::session::CaptureSessionService>>()
        .lock()
        .ok()
        .map(|mut service| service.get_or_prepare());
    if platform.show_capture_popup().is_err() {
        return;
    }
    if let Some(session) = session {
        let _ = app.emit("capture://session-ready", &session);
        let _ = app.emit(
            "context://sources-changed",
            serde_json::json!({ "sessionId": session.session_id }),
        );
    }
}

#[cfg(all(desktop, not(target_os = "linux")))]
fn invoke_capture_popup(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rust_test_harness_is_available() {
        assert_eq!(2 + 2, 4);
    }
}
