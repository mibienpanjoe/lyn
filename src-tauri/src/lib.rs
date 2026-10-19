use std::sync::Mutex;

use tauri::{Emitter, Manager};

#[cfg(desktop)]
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

mod capture;
mod commands;
mod context;
pub mod contract;
#[cfg(all(test, target_os = "linux"))]
mod desktop_tests;
mod enrichment;
pub mod error;
pub mod integrations;
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
    desktop_builder(None)
        .run(tauri::generate_context!())
        .expect("failed to run Lyn");
}

// The private override is used only by the cfg(test) desktop fixture. The
// production entry point always refreshes the user's normal helper location.
fn desktop_builder(helper_root: Option<std::path::PathBuf>) -> tauri::Builder<tauri::Wry> {
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
            let resolved =
                match media::resource_from_request(request.uri().host(), request.uri().path()) {
                    Some(media::LynMediaResource::Staged(staged_media_id)) => media_state
                        .lock()
                        .ok()
                        .and_then(|store| store.staged_preview(staged_media_id).ok()),
                    Some(media::LynMediaResource::Capture(media_id)) => {
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
                    None => None,
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
                .header(tauri::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
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
    let builder = builder.on_window_event(|window, event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            if window.label() == "capture" {
                api.prevent_close();
                dismiss_and_cancel_capture(window.app_handle());
            }
        }
    });
    builder
        .setup(move |app| {
            #[cfg(not(target_os = "linux"))]
            let _ = &helper_root;
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
            if let Ok(worker) = context::resolution_worker::ResolutionWorker::start() {
                app.manage(worker);
            }
            #[cfg(target_os = "linux")]
            if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR")
                && let Ok(broker) =
                    context::gnome_broker::NativeBroker::start(std::path::Path::new(&runtime))
            {
                app.manage(std::sync::Arc::new(broker));
            }
            #[cfg(target_os = "linux")]
            let _ = context::browser_provider::start(app.handle().clone());
            #[cfg(target_os = "linux")]
            let _ = context::shell_provider::start(app.handle().clone());
            #[cfg(target_os = "linux")]
            {
                if let Ok(current_exe) = std::env::current_exe() {
                    let home = helper_root
                        .clone()
                        .unwrap_or_else(integrations::user_home_dir);
                    let _ = integrations::refresh_shell_helper(&home, &current_exe);
                    let _ = integrations::refresh_browser_host(&home, &current_exe);
                }
            }
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
            commands::library::delete_capture,
            commands::library::search_captures,
            commands::library::play_media,
            commands::library::open_media_external,
            commands::platform::dismiss_capture_popup,
            commands::platform::set_capture_popup_layout,
            commands::settings::get_app_version,
            commands::settings::get_settings,
            commands::settings::update_settings,
            #[cfg(target_os = "linux")]
            commands::integrations::get_integration_statuses,
            #[cfg(target_os = "linux")]
            commands::integrations::install_integration,
            commands::model::get_speech_model_status,
            commands::model::install_speech_model,
            commands::model::cancel_speech_model_install,
            commands::model::remove_speech_model,
        ])
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
pub fn run_browser_host_helper() -> std::process::ExitCode {
    context::browser_provider::run_browser_host_helper()
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxHelperMode {
    ShellWatch,
    BrowserHost,
}

#[cfg(target_os = "linux")]
pub fn linux_helper_mode(
    argv0: Option<&std::ffi::OsStr>,
    argv1: Option<&std::ffi::OsStr>,
) -> Option<LinuxHelperMode> {
    use std::path::Path;

    let invoked_as_browser_host = argv0
        .map(Path::new)
        .and_then(Path::file_name)
        .is_some_and(|name| name == "lyn-browser-host");
    if invoked_as_browser_host || argv1.is_some_and(|argument| argument == "browser-host") {
        return Some(LinuxHelperMode::BrowserHost);
    }
    if argv1.is_some_and(|argument| argument == "watch") {
        return Some(LinuxHelperMode::ShellWatch);
    }
    None
}

fn dismiss_and_cancel_capture(app: &tauri::AppHandle) {
    use crate::platform::CaptureWindowPlatform;

    if let Ok(mut service) = app
        .state::<Mutex<capture::session::CaptureSessionService>>()
        .lock()
    {
        if let Some(session) = service.active_session() {
            let _ = service.cancel(session.session_id);
            if let Ok(mut registry) = app
                .state::<Mutex<context::session_registry::ContextSourceRegistry>>()
                .lock()
            {
                registry.release_native(session.session_id);
            }
            if let Ok(mut media_store) = app.state::<Mutex<media::staging::MediaStore>>().lock() {
                while let Some(cleanup) = service.take_cleanup_request() {
                    let _ = media_store.discard_staged(cleanup.session_id, cleanup.staged_media_id);
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    let mut platform = platform::x11::X11CaptureWindowPlatform::new(app.clone());
    #[cfg(not(target_os = "linux"))]
    let mut platform = platform::UnsupportedCaptureWindowPlatform::new(app.clone());

    if let Ok(mut invocation) = app.state::<Mutex<platform::InvocationContext>>().lock() {
        let _ = invocation.dismiss(&mut platform);
    } else {
        let _ = platform.hide_capture_popup();
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn invoke_capture_popup(app: &tauri::AppHandle) {
    // Global shortcut: no v2 request generation and no v2 exclusive-claim.
    invoke_capture_popup_with_request(app, None);
}

#[cfg(target_os = "linux")]
enum NativeInvocationRoute {
    Other,
    Refused,
    Query(u32, context::gnome_process::ProcessInvocation),
}

#[cfg(target_os = "linux")]
pub(crate) fn invoke_capture_popup_with_request(
    app: &tauri::AppHandle,
    request_id: Option<uuid::Uuid>,
) {
    use crate::platform::CaptureWindowPlatform;

    let mut platform = platform::x11::X11CaptureWindowPlatform::new(app.clone());
    let native_time = context::gnome_process::ProcessInvocation::now().ok();
    let sample = platform::x11::sample_foreground().ok();
    let foreground = sample.map(|(identity, _)| identity);
    let native_route = sample
        .map(|(identity, actual)| {
            let window = identity.window.native() as u32;
            match platform::x11::classify_native_window(window).map(|(_, kind)| kind) {
                Some(platform::x11::ContextWindowKind::GnomeTerminal) => {
                    match native_time.filter(|_| actual) {
                        Some(time) => NativeInvocationRoute::Query(window, time),
                        None => NativeInvocationRoute::Refused,
                    }
                }
                Some(_) => NativeInvocationRoute::Other,
                None => NativeInvocationRoute::Refused,
            }
        })
        .unwrap_or(NativeInvocationRoute::Refused);
    // Freeze eligibility before queuing: a later heartbeat is not invocation
    // evidence, even when it refers to the same native window.
    let sources = app
        .state::<Mutex<context::session_registry::ContextSourceRegistry>>()
        .lock()
        .ok()
        .map(|mut registry| registry.snapshot_live_sources(std::time::Instant::now()))
        .unwrap_or_default();
    if let Ok(mut invocation) = app.state::<Mutex<platform::InvocationContext>>().lock() {
        if foreground.is_some() || !invocation.has_foreground() {
            invocation.record_foreground(foreground);
        }
    }
    let prepared = app
        .state::<Mutex<capture::session::CaptureSessionService>>()
        .lock()
        .ok()
        .map(|mut service| {
            let prepared = service.prepare_invocation(request_id);
            let is_new = prepared.is_new();
            let apply_automatic = service.should_apply_automatic_foreground(
                is_new,
                sample.is_some_and(|(_, actual)| actual),
            );
            let ticket = apply_automatic
                .then(|| service.begin_automatic_query(prepared.session().session_id))
                .flatten();
            (
                service
                    .active_session()
                    .unwrap_or_else(|| prepared.session()),
                is_new,
                ticket,
            )
        });
    let is_new = prepared.as_ref().is_some_and(|(_, is_new, _)| *is_new);
    let ticket = prepared.as_ref().and_then(|(_, _, ticket)| *ticket);
    let prepared_session = prepared.map(|(session, _, _)| session);
    let (popup_ready, wait_popup_ready) = std::sync::mpsc::channel();
    if let Some(ticket) = ticket {
        queue_invocation_resolution(
            app,
            foreground,
            sources,
            native_route,
            ticket,
            wait_popup_ready,
        );
    }
    if platform.show_capture_popup().is_err() {
        return;
    }
    if let Some(session) = prepared_session {
        if is_new {
            let _ = app.emit("capture://session-ready", &session);
        } else {
            let _ = app.emit("capture://context-revised", &session);
        }
        let _ = app.emit(
            "context://sources-changed",
            serde_json::json!({ "sessionId": session.session_id }),
        );
    }
    // The worker can compute during popup construction, but never publishes a
    // result before the initial session event. Failure to show drops this gate.
    let _ = popup_ready.send(());
}

#[cfg(target_os = "linux")]
fn queue_invocation_resolution(
    app: &tauri::AppHandle,
    foreground: Option<platform::ForegroundWindowIdentity>,
    sources: Vec<context::session_registry::LiveContextSource>,
    native_route: NativeInvocationRoute,
    ticket: capture::session::AutomaticContextTicket,
    popup_ready: std::sync::mpsc::Receiver<()>,
) {
    let Some(worker) = app.try_state::<context::resolution_worker::ResolutionWorker>() else {
        return;
    };
    let app = app.clone();
    worker.submit(Box::new(move || {
        let Some((resolution, native_source)) =
            compute_invocation_context(&app, foreground, sources, native_route)
        else {
            return;
        };
        if popup_ready
            .recv_timeout(std::time::Duration::from_secs(5))
            .is_err()
        {
            return;
        }
        let revised = app
            .state::<Mutex<capture::session::CaptureSessionService>>()
            .lock()
            .ok()
            .and_then(|mut service| {
                // Native source preparation did I/O before either guard. Retire
                // the previous lease only after this exact ticket is accepted.
                let registry_state =
                    app.state::<Mutex<context::session_registry::ContextSourceRegistry>>();
                let mut registry = registry_state.lock().ok()?;
                let session = service
                    .finish_automatic_query(ticket, resolution)
                    .ok()
                    .flatten()?;
                if let Some(source) = native_source {
                    registry.register_native(session.session_id, source);
                }
                Some(session)
            });
        if let Some(session) = revised {
            let _ = app.emit("capture://context-revised", &session);
            let _ = app.emit(
                "context://sources-changed",
                serde_json::json!({"sessionId":session.session_id}),
            );
        }
    }));
}

#[cfg(target_os = "linux")]
fn compute_invocation_context(
    app: &tauri::AppHandle,
    foreground: Option<platform::ForegroundWindowIdentity>,
    sources: Vec<context::session_registry::LiveContextSource>,
    native_route: NativeInvocationRoute,
) -> Option<(
    contract::ContextResolution,
    Option<context::session_registry::LiveContextSource>,
)> {
    // Sources were frozen before dispatch. No registry/session guard spans I/O.
    use context::resolver::{InvocationAssociations, ResolutionOutcome, classify, resolve};
    use contract::{ContextCandidate, ContextProviderKind, ContextResolution, ContextSelection};

    if !matches!(native_route, NativeInvocationRoute::Other) {
        let resolved = (|| {
            let NativeInvocationRoute::Query(window, invocation) = native_route else {
                return None;
            };
            let broker = app.try_state::<std::sync::Arc<context::gnome_broker::NativeBroker>>()?;
            let database = app.state::<Mutex<storage::Database>>();
            let (resolution, source) = context::gnome_capture::prepare(
                broker.inner().clone(),
                window,
                invocation,
                database.inner(),
            )?;
            Some((resolution, Some(source)))
        })();
        return Some(resolved.unwrap_or((
            ContextResolution::Required {
                candidate: (),
                selection: (),
            },
            None,
        )));
    }

    let foreground_window = foreground.map(|identity| identity.window);
    let associations = InvocationAssociations {
        foreground_window,
        related_processes: &[],
        related_sessions: &[],
        inferred_windows: &[],
    };
    let outcome = {
        let candidates: Vec<_> = sources
            .iter()
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
                    ContextProviderKind::Cursor,
                    ContextProviderKind::Browser,
                    ContextProviderKind::Shell,
                    ContextProviderKind::ForegroundWindow,
                ]
            });
        resolve(&candidates, &provider_order)
    };

    let resolution = match outcome {
        ResolutionOutcome::Required => ContextResolution::Required {
            candidate: (),
            selection: (),
        },
        ResolutionOutcome::Ambiguous => ContextResolution::Ambiguous {
            candidate: (),
            selection: (),
        },
        ResolutionOutcome::Resolved(source_id) => {
            let source = {
                let source = sources
                    .iter()
                    .find(|source| source.source_id() == source_id)?;
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
                    return None;
                };
                let Ok(context) = storage::contexts::ContextRepository::new(database.connection())
                    .ensure_project(source.0.id, &source.0.name, source.1.as_deref(), &source.2)
                else {
                    return None;
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
    Some((resolution, None))
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

    #[cfg(target_os = "linux")]
    #[test]
    fn native_messaging_invocation_runs_the_browser_host_not_the_gui() {
        use std::ffi::OsStr;

        use super::{LinuxHelperMode, linux_helper_mode};

        assert_eq!(
            linux_helper_mode(
                Some(OsStr::new("/home/me/.local/share/lyn/bin/lyn-browser-host")),
                Some(OsStr::new("chrome-extension://abc/"))
            ),
            Some(LinuxHelperMode::BrowserHost)
        );
        assert_eq!(
            linux_helper_mode(Some(OsStr::new("lyn")), Some(OsStr::new("browser-host"))),
            Some(LinuxHelperMode::BrowserHost)
        );
        assert_eq!(
            linux_helper_mode(Some(OsStr::new("lyn-context")), Some(OsStr::new("watch"))),
            Some(LinuxHelperMode::ShellWatch)
        );
        assert_eq!(
            linux_helper_mode(Some(OsStr::new("/usr/bin/lyn")), None),
            None
        );
    }
}
