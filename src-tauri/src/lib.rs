use std::{str::FromStr, sync::Mutex};

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
    let builder = tauri::Builder::default()
        .register_uri_scheme_protocol("lyn-media", |context, request| {
            let not_found = || {
                tauri::http::Response::builder()
                    .status(404)
                    .body(Vec::new())
                    .expect("static media response is valid")
            };
            if request.uri().host() != Some("staged") {
                return not_found();
            }
            let Ok(staged_media_id) =
                contract::StagedMediaId::from_str(request.uri().path().trim_start_matches('/'))
            else {
                return not_found();
            };
            let media_state = context
                .app_handle()
                .state::<Mutex<media::staging::MediaStore>>();
            let Ok(store) = media_state.lock() else {
                return not_found();
            };
            let Ok((bytes, mime_type)) = store.staged_preview(staged_media_id) else {
                return not_found();
            };
            let content_type = match mime_type {
                contract::MediaMimeType::ImagePng => "image/png",
                contract::MediaMimeType::AudioWav => "audio/wav",
            };
            tauri::http::Response::builder()
                .header(tauri::http::header::CONTENT_TYPE, content_type)
                .header(tauri::http::header::CACHE_CONTROL, "no-store")
                .body(bytes)
                .expect("static media response is valid")
        })
        .plugin(tauri_plugin_dialog::init());
    #[cfg(desktop)]
    let builder = builder.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|app, _shortcut, event| {
                if event.state != ShortcutState::Pressed {
                    return;
                }
                invoke_capture_popup(app);
            })
            .build(),
    );
    builder
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let database_path = app_data_dir.join("lyn.db");
            let database = storage::Database::open(database_path)?;
            let referenced_paths =
                storage::media_assets::MediaAssetRepository::new(database.connection())
                    .referenced_relative_paths()?;
            let mut media_store = media::staging::MediaStore::open(app_data_dir)?;
            media_store.reconcile(&referenced_paths)?;
            app.manage(Mutex::new(database));
            app.manage(Mutex::new(media_store));
            app.manage(Mutex::new(platform::clipboard::NativeClipboardPlatform));
            app.manage(Mutex::new(
                platform::audio::NativeAudioInputPlatform::default(),
            ));
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
            commands::capture::stage_clipboard_image,
            commands::capture::save_image_capture,
            commands::capture::start_audio_recording,
            commands::capture::stop_audio_recording,
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
        .map(|mut service| service.get_or_prepare())
        .map(|session| resolve_invocation_context(app, session, foreground));
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

#[cfg(target_os = "linux")]
fn resolve_invocation_context(
    app: &tauri::AppHandle,
    session: contract::CaptureSession,
    foreground: Option<platform::ForegroundWindowIdentity>,
) -> contract::CaptureSession {
    use context::resolver::{InvocationAssociations, ResolutionOutcome, classify, resolve};
    use contract::{ContextCandidate, ContextProviderKind, ContextResolution, ContextSelection};

    let foreground_window = foreground.map(|identity| identity.window);
    let associations = InvocationAssociations {
        foreground_window,
        related_processes: &[],
        related_sessions: &[],
        inferred_windows: &[],
    };
    let outcome = {
        let registry_state = app.state::<Mutex<context::session_registry::ContextSourceRegistry>>();
        let Ok(mut registry) = registry_state.lock() else {
            return session;
        };
        let candidates: Vec<_> = registry
            .live_sources(std::time::Instant::now())
            .into_iter()
            .filter_map(|source| {
                classify(
                    source.source_id(),
                    source.provider(),
                    source.window(),
                    source.process(),
                    source.session(),
                    &associations,
                )
            })
            .collect();
        resolve(
            &candidates,
            &[
                ContextProviderKind::Vscode,
                ContextProviderKind::Shell,
                ContextProviderKind::ForegroundWindow,
            ],
        )
    };

    let resolution = match outcome {
        ResolutionOutcome::Required => return session,
        ResolutionOutcome::Ambiguous => ContextResolution::Ambiguous {
            candidate: (),
            selection: (),
        },
        ResolutionOutcome::Resolved(source_id) => {
            let source = {
                let registry_state =
                    app.state::<Mutex<context::session_registry::ContextSourceRegistry>>();
                let Ok(mut registry) = registry_state.lock() else {
                    return session;
                };
                let Some(source) = registry.get(source_id, std::time::Instant::now()) else {
                    return session;
                };
                (
                    source.context().clone(),
                    source.identity().project_key.clone(),
                    source.identity().project_path.clone(),
                    source.identity().branch_name.clone(),
                    source.provider(),
                )
            };
            let context = {
                let database_state = app.state::<Mutex<storage::Database>>();
                let Ok(database) = database_state.lock() else {
                    return session;
                };
                let Ok(context) = storage::contexts::ContextRepository::new(database.connection())
                    .ensure_project(source.0.id, &source.0.name, source.1.as_deref(), &source.2)
                else {
                    return session;
                };
                context
            };
            ContextResolution::Resolved {
                candidate: ContextCandidate {
                    context,
                    branch_name: source.3,
                    provider: source.4,
                    requires_confirmation: false,
                },
                selection: Some(ContextSelection::LiveSource { source_id }),
            }
        }
    };
    app.state::<Mutex<capture::session::CaptureSessionService>>()
        .lock()
        .ok()
        .and_then(|mut service| {
            service
                .set_context_resolution(session.session_id, resolution)
                .ok()
        })
        .unwrap_or(session)
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
