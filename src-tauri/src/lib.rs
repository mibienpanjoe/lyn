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
mod perf;
mod platform;
mod security;
mod settings;
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
            let app = context.app_handle();
            let media_state = app.state::<Mutex<media::staging::MediaStore>>();
            let resolved = match request.uri().host() {
                Some("staged") => {
                    let Ok(staged_media_id) = contract::StagedMediaId::from_str(
                        request.uri().path().trim_start_matches('/'),
                    ) else {
                        return not_found();
                    };
                    media_state
                        .lock()
                        .ok()
                        .and_then(|store| store.staged_preview(staged_media_id).ok())
                }
                Some("capture") => {
                    let Ok(media_id) =
                        contract::MediaId::from_str(request.uri().path().trim_start_matches('/'))
                    else {
                        return not_found();
                    };
                    let database_state = app.state::<Mutex<storage::Database>>();
                    let asset = database_state.lock().ok().and_then(|database| {
                        storage::media_assets::MediaAssetRepository::new(database.connection())
                            .find(media_id)
                            .ok()
                            .flatten()
                    });
                    asset.and_then(|asset| {
                        media_state.lock().ok().and_then(|store| {
                            store
                                .read_final(&asset.relative_path)
                                .ok()
                                .map(|bytes| (bytes, asset.mime_type))
                        })
                    })
                }
                _ => None,
            };
            let Some((bytes, mime_type)) = resolved else {
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
            let speech_manager = intelligence::model::SpeechModelManager::new(&app_data_dir);
            let database_path = app_data_dir.join("lyn.db");
            let mut database = storage::Database::open(database_path)?;
            let settings = storage::settings::SettingsRepository::new(database.connection())
                .get()
                .unwrap_or_default();
            let referenced_paths =
                storage::media_assets::MediaAssetRepository::new(database.connection())
                    .referenced_relative_paths()?;
            let enrichment = enrichment::EnrichmentQueue::new(database.connection_mut());
            let _ = enrichment.recover_interrupted();
            let mut media_store = media::staging::MediaStore::open(app_data_dir)?;
            media_store.reconcile(&referenced_paths)?;
            app.manage(Mutex::new(database));
            app.manage(speech_manager);
            app.manage(Mutex::new(media_store));
            app.manage(Mutex::new(platform::clipboard::NativeClipboardPlatform));
            app.manage(Mutex::new(
                platform::audio::NativeAudioInputPlatform::default(),
            ));
            app.manage(Mutex::new(
                platform::playback::NativeAudioPlaybackPlatform::default(),
            ));
            app.manage(platform::media_open::NativeMediaOpenPlatform);
            app.manage(Mutex::new(context::DirectorySelectionRegistry::default()));
            app.manage(Mutex::new(
                context::session_registry::ContextSourceRegistry::default(),
            ));
            app.manage(Mutex::new(
                capture::session::CaptureSessionService::default(),
            ));
            app.manage(Mutex::new(platform::InvocationContext::default()));
            if settings.local_speech_enabled
                && app
                    .state::<intelligence::model::SpeechModelManager>()
                    .installed()
            {
                spawn_enrichment_worker(app.handle().clone());
            }
            #[cfg(target_os = "linux")]
            // Context providers enrich capture, but must never make core capture unavailable.
            // A missing or unusable runtime socket therefore disables only this provider.
            let _ = context::vscode_provider::start(app.handle().clone());
            #[cfg(target_os = "linux")]
            let _ = context::shell_provider::start(app.handle().clone());
            #[cfg(desktop)]
            {
                let _ = app
                    .global_shortcut()
                    .register(settings.global_shortcut.as_str());
            }
            let theme = match settings.theme {
                contract::ThemeSetting::System => None,
                contract::ThemeSetting::Light => Some(tauri::Theme::Light),
                contract::ThemeSetting::Dark => Some(tauri::Theme::Dark),
            };
            app.set_theme(theme);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::capture::get_active_capture_session,
            commands::capture::cancel_capture_session,
            commands::capture::list_capture_context_sources,
            commands::capture::select_capture_context_source,
            commands::capture::save_text_capture,
            commands::capture::stage_clipboard_image,
            commands::capture::discard_staged_media,
            commands::capture::save_image_capture,
            commands::capture::start_audio_recording,
            commands::capture::stop_audio_recording,
            commands::capture::play_staged_audio,
            commands::capture::stop_audio_playback,
            commands::capture::save_audio_capture,
            commands::context::pick_project_directory,
            commands::context::create_context,
            commands::context::list_contexts,
            commands::library::list_captures,
            commands::library::get_capture,
            commands::library::search_captures,
            commands::library::play_media,
            commands::library::open_media_external,
            commands::platform::dismiss_capture_popup,
            commands::platform::set_capture_popup_layout,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::model::get_speech_model_status,
            commands::model::install_speech_model,
            commands::model::cancel_speech_model_install,
            commands::model::remove_speech_model,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Lyn");
}

pub(crate) fn spawn_enrichment_worker(app: tauri::AppHandle) {
    let manager = app
        .state::<intelligence::model::SpeechModelManager>()
        .inner()
        .clone();
    if !manager.begin_worker() {
        return;
    }
    let worker_manager = manager.clone();
    if std::thread::Builder::new()
        .name("lyn-speech-enrichment".to_owned())
        .spawn(move || {
            let enabled = app
                .state::<Mutex<storage::Database>>()
                .lock()
                .ok()
                .and_then(|database| {
                    storage::settings::SettingsRepository::new(database.connection())
                        .get()
                        .ok()
                })
                .is_some_and(|settings| settings.local_speech_enabled);
            if enabled {
                let database = app.state::<Mutex<storage::Database>>();
                let mut processor = worker_manager.processor();
                loop {
                    let Ok((processed, event)) =
                        enrichment::process_one(database.inner(), true, &mut processor)
                    else {
                        break;
                    };
                    if let Some(event) = event {
                        let _ = app.emit("enrichment://updated", event);
                    }
                    if !processed {
                        break;
                    }
                }
            }
            worker_manager.finish_worker();
        })
        .is_err()
    {
        manager.finish_worker();
    }
}

#[cfg(target_os = "linux")]
pub fn run_shell_context_helper() -> std::process::ExitCode {
    context::shell_provider::run_helper()
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
        let provider_order = app
            .state::<Mutex<storage::Database>>()
            .lock()
            .ok()
            .and_then(|database| {
                storage::settings::SettingsRepository::new(database.connection())
                    .get()
                    .ok()
            })
            .map(|settings| settings.provider_tie_break_order)
            .unwrap_or_else(|| {
                vec![
                    ContextProviderKind::Vscode,
                    ContextProviderKind::Shell,
                    ContextProviderKind::ForegroundWindow,
                ]
            });
        resolve(&candidates, &provider_order)
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
    if let Some(window) = app.get_webview_window("capture") {
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
