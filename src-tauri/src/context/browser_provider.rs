//! Local, bounded browser context observations over a user-only Unix socket.

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, Read, Write},
    net::Shutdown,
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::UnixListener,
    },
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::{
    context::{
        invocation_protocol::MAX_PROVIDER_MESSAGE_BYTES,
        localhost_resolver::{find_listening_port_cwd, parse_file_url_path, parse_localhost_port},
        provider::{
            CorrelationToken, ObservationLiveness, ProviderObservation, ProviderSourceKind,
        },
        session_registry::ContextSourceRegistry,
        unix_broker::{ProviderByteChannel, UnixSocketChannel},
    },
    contract::ContextProviderKind,
    platform::{WindowCorrelationToken, x11::ContextWindowKind},
};

const SOCKET_NAME: &str = "lyn-browser-v1.sock";
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WindowState {
    Focused,
    Unfocused,
    Ended,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserObservationMessage {
    pub(crate) version: u8,
    pub(crate) instance_id: Uuid,
    pub(crate) state: WindowState,
    pub(crate) url: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) incognito: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserInvokeMessage {
    pub(crate) version: u8,
    pub(crate) kind: String,
    pub(crate) request_id: Uuid,
    pub(crate) instance_id: Uuid,
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) incognito: bool,
}

#[derive(Debug)]
pub(crate) enum BrowserHostMessage {
    Observe(BrowserObservationMessage),
    Invoke(BrowserInvokeMessage),
}

pub(crate) fn start(app: AppHandle) -> io::Result<()> {
    let (path, expected_uid) = socket_path()?;
    prepare_socket_path(&path)?;
    let listener = UnixListener::bind(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    thread::Builder::new()
        .name("lyn-browser-provider".to_owned())
        .spawn(move || run(listener, app, expected_uid))?;
    Ok(())
}

fn run(listener: UnixListener, app: AppHandle, expected_uid: u32) {
    let mut tabs = HashMap::new();
    let mut seen_invokes = HashSet::new();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let mut channel = UnixSocketChannel::new(stream, expected_uid);
                let Ok(bytes) = channel.recv(MAX_PROVIDER_MESSAGE_BYTES) else {
                    continue;
                };
                drop(channel);

                let Some(message) = parse_browser_message(&bytes) else {
                    continue;
                };

                let (observation, invoke_request) = match message {
                    BrowserHostMessage::Observe(observation) => (observation, None),
                    BrowserHostMessage::Invoke(invoke) => {
                        if invoke.version != 2 || invoke.kind != "invoke" {
                            continue;
                        }
                        let request_id = invoke.request_id;
                        if !admit_invoke(&mut seen_invokes, request_id) {
                            continue;
                        }
                        (observation_from_invoke(invoke), Some(request_id))
                    }
                };

                let sample_focus =
                    observation.state == WindowState::Focused && !observation.incognito;
                let active_window = sample_focus
                    .then(crate::platform::x11::active_browser_window)
                    .and_then(Result::ok);

                let registry_state = app.state::<std::sync::Mutex<ContextSourceRegistry>>();
                let Ok(mut registry) = registry_state.lock() else {
                    continue;
                };

                let changed = apply_message(
                    &mut registry,
                    &mut tabs,
                    observation,
                    active_window,
                    Instant::now(),
                );
                drop(registry);

                if changed {
                    let session_id = app
                        .state::<std::sync::Mutex<crate::capture::session::CaptureSessionService>>()
                        .lock()
                        .ok()
                        .and_then(|service| service.active_session())
                        .map(|session| session.session_id);
                    if let Some(session_id) = session_id {
                        let _ = app.emit(
                            "context://sources-changed",
                            serde_json::json!({ "sessionId": session_id }),
                        );
                    }
                }

                if let Some(request_id) = invoke_request {
                    let handle = app.clone();
                    let _ = app.run_on_main_thread(move || {
                        crate::invoke_capture_popup_with_request(&handle, Some(request_id));
                    });
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(_) => thread::sleep(ACCEPT_POLL_INTERVAL),
        }
    }
}

pub(crate) fn parse_browser_message(bytes: &[u8]) -> Option<BrowserHostMessage> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    if value.get("windowId").is_some()
        || value.get("processId").is_some()
        || value.get("cwd").is_some()
    {
        return None;
    }
    if value.get("kind").and_then(serde_json::Value::as_str) == Some("invoke") {
        let invoke: BrowserInvokeMessage = serde_json::from_value(value).ok()?;
        if invoke.version != 2 || invoke.kind != "invoke" {
            return None;
        }
        return Some(BrowserHostMessage::Invoke(invoke));
    }
    let observation: BrowserObservationMessage = serde_json::from_value(value).ok()?;
    if observation.version != 1 {
        return None;
    }
    Some(BrowserHostMessage::Observe(observation))
}

fn observation_from_invoke(invoke: BrowserInvokeMessage) -> BrowserObservationMessage {
    BrowserObservationMessage {
        version: 1,
        instance_id: invoke.instance_id,
        state: WindowState::Focused,
        url: invoke.url,
        title: None,
        incognito: invoke.incognito,
    }
}

fn admit_invoke(seen: &mut HashSet<Uuid>, request_id: Uuid) -> bool {
    seen.insert(request_id)
}

pub(crate) fn apply_message(
    registry: &mut ContextSourceRegistry,
    tabs: &mut HashMap<Uuid, (WindowCorrelationToken, PathBuf)>,
    message: BrowserObservationMessage,
    active_window: Option<(u32, ContextWindowKind)>,
    now: Instant,
) -> bool {
    if message.version != 1 || message.incognito {
        return false;
    }

    let mut removed_previous = false;

    match message.state {
        WindowState::Focused => {
            let Some((window_id, ContextWindowKind::Browser)) = active_window else {
                return false;
            };

            let Some(url) = message.url.as_deref() else {
                return false;
            };

            // Resolve directory from localhost port or file:// URL
            let directory = if let Some(port) = parse_localhost_port(url) {
                find_listening_port_cwd(port)
            } else {
                parse_file_url_path(url)
            };

            let Some(directory) = directory else {
                return false;
            };

            let window_token = WindowCorrelationToken::from_native(u64::from(window_id));
            let observation = ProviderObservation::new(
                ContextProviderKind::Browser,
                ProviderSourceKind::BrowserTab,
                Some(window_token),
                None,
                Some(CorrelationToken::from_session_id(message.instance_id)),
                directory.clone(),
                now,
                ObservationLiveness::Live,
            );

            if let Some((previous_window, previous_dir)) =
                tabs.insert(message.instance_id, (window_token, directory.clone()))
            {
                if previous_window != window_token || previous_dir != directory {
                    removed_previous = true;
                }
            }

            registry.register(observation, now).is_some() || removed_previous
        }
        WindowState::Unfocused => {
            if let Some((window, directory)) = tabs.get(&message.instance_id).cloned() {
                let observation = ProviderObservation::new(
                    ContextProviderKind::Browser,
                    ProviderSourceKind::BrowserTab,
                    Some(window),
                    None,
                    Some(CorrelationToken::from_session_id(message.instance_id)),
                    directory,
                    now,
                    ObservationLiveness::Live,
                );
                registry.register(observation, now).is_some()
            } else {
                false
            }
        }
        WindowState::Ended => {
            if let Some((window, directory)) = tabs.remove(&message.instance_id) {
                let observation = ProviderObservation::new(
                    ContextProviderKind::Browser,
                    ProviderSourceKind::BrowserTab,
                    Some(window),
                    None,
                    Some(CorrelationToken::from_session_id(message.instance_id)),
                    directory,
                    now,
                    ObservationLiveness::Ended,
                );
                registry.register(observation, now);
                true
            } else {
                false
            }
        }
    }
}

fn socket_path() -> io::Result<(PathBuf, u32)> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is unavailable"))?;
    let metadata = fs::metadata(&runtime)?;
    if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "XDG_RUNTIME_DIR is not user-private",
        ));
    }
    Ok((runtime.join(SOCKET_NAME), metadata.uid()))
}

fn prepare_socket_path(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => fs::remove_file(path),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "provider socket path is occupied",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn run_browser_host_helper() -> std::process::ExitCode {
    let mut stdin = io::stdin().lock();
    let mut length_buf = [0u8; 4];
    if stdin.read_exact(&mut length_buf).is_err() {
        return std::process::ExitCode::FAILURE;
    }
    let length = u32::from_ne_bytes(length_buf) as usize;
    if length == 0 || length > 1024 * 1024 {
        return std::process::ExitCode::FAILURE;
    }
    let mut msg_buf = vec![0u8; length];
    if stdin.read_exact(&mut msg_buf).is_err() {
        return std::process::ExitCode::FAILURE;
    }

    let Ok((socket_path, _)) = socket_path() else {
        return std::process::ExitCode::FAILURE;
    };
    let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&socket_path) else {
        send_native_response(b"{\"status\":\"offline\"}");
        return std::process::ExitCode::SUCCESS;
    };

    let _ = stream.write_all(&msg_buf);
    let _ = stream.shutdown(Shutdown::Write);
    let mut reply = Vec::new();
    let _ = stream.read_to_end(&mut reply);

    send_native_response(b"{\"status\":\"ok\"}");
    std::process::ExitCode::SUCCESS
}

fn send_native_response(response: &[u8]) {
    let mut stdout = io::stdout().lock();
    let length = response.len() as u32;
    let _ = stdout.write_all(&length.to_ne_bytes());
    let _ = stdout.write_all(response);
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, HashSet},
        fs,
        time::Instant,
    };

    use tempfile::tempdir;
    use uuid::Uuid;

    use crate::{
        context::{
            resolver::{InvocationAssociations, ResolutionOutcome, classify, resolve},
            session_registry::ContextSourceRegistry,
        },
        contract::ContextProviderKind,
        platform::{WindowCorrelationToken, x11::ContextWindowKind},
    };

    use super::{
        BrowserHostMessage, BrowserObservationMessage, WindowState, admit_invoke, apply_message,
        observation_from_invoke, parse_browser_message,
    };

    const INSTANCE: &str = "7af0a690-8948-4f0a-b9f0-51e43c583efa";
    const REQUEST: &str = "c3b1a2d0-1111-4aaa-8bbb-0123456789ab";

    #[test]
    fn registers_focused_browser_tab_for_file_or_localhost() {
        let repo = tempdir().unwrap();
        fs::create_dir(repo.path().join(".git")).unwrap();

        let mut registry = ContextSourceRegistry::default();
        let mut tabs = HashMap::new();
        let now = Instant::now();

        let message = BrowserObservationMessage {
            version: 1,
            instance_id: Uuid::new_v4(),
            state: WindowState::Focused,
            url: Some(format!("file://{}", repo.path().display())),
            title: Some("My App".to_string()),
            incognito: false,
        };

        let registered = apply_message(
            &mut registry,
            &mut tabs,
            message,
            Some((303, ContextWindowKind::Browser)),
            now,
        );
        assert!(registered);

        let sources = registry.live_sources(now);
        assert_eq!(sources.len(), 1);
        let source = &sources[0];
        assert_eq!(source.provider(), ContextProviderKind::Browser);
        assert_eq!(source.application_name(), "Browser");

        // Verify resolver can select it by exact foreground window
        let foreground = Some(WindowCorrelationToken::from_native(303));
        let associations = InvocationAssociations {
            foreground_window: foreground,
            related_processes: &[],
            related_sessions: &[],
            inferred_windows: &[],
        };
        let candidate = classify(
            source.source_id(),
            source.provider(),
            source.window(),
            source.process(),
            source.session(),
            &associations,
        )
        .expect("browser candidate classified");

        let resolution = resolve(&[candidate], &[ContextProviderKind::Browser]);
        assert_eq!(resolution, ResolutionOutcome::Resolved(source.source_id()));
    }

    #[test]
    fn ignores_incognito_tabs() {
        let repo = tempdir().unwrap();
        let mut registry = ContextSourceRegistry::default();
        let mut tabs = HashMap::new();
        let now = Instant::now();

        let message = BrowserObservationMessage {
            version: 1,
            instance_id: Uuid::new_v4(),
            state: WindowState::Focused,
            url: Some(format!("file://{}", repo.path().display())),
            title: Some("Incognito".to_string()),
            incognito: true,
        };

        let registered = apply_message(
            &mut registry,
            &mut tabs,
            message,
            Some((303, ContextWindowKind::Browser)),
            now,
        );
        assert!(!registered);
        assert!(registry.live_sources(now).is_empty());
    }

    #[test]
    fn ended_state_removes_tab() {
        let repo = tempdir().unwrap();
        fs::create_dir(repo.path().join(".git")).unwrap();

        let mut registry = ContextSourceRegistry::default();
        let mut tabs = HashMap::new();
        let now = Instant::now();
        let instance_id = Uuid::new_v4();

        let focus_msg = BrowserObservationMessage {
            version: 1,
            instance_id,
            state: WindowState::Focused,
            url: Some(format!("file://{}", repo.path().display())),
            title: Some("App".to_string()),
            incognito: false,
        };

        apply_message(
            &mut registry,
            &mut tabs,
            focus_msg,
            Some((303, ContextWindowKind::Browser)),
            now,
        );
        assert_eq!(registry.live_sources(now).len(), 1);

        let end_msg = BrowserObservationMessage {
            version: 1,
            instance_id,
            state: WindowState::Ended,
            url: None,
            title: None,
            incognito: false,
        };

        apply_message(
            &mut registry,
            &mut tabs,
            end_msg,
            Some((303, ContextWindowKind::Browser)),
            now,
        );
        assert!(registry.live_sources(now).is_empty());
    }

    #[test]
    fn remote_https_is_not_a_local_source() {
        let mut registry = ContextSourceRegistry::default();
        let mut tabs = HashMap::new();
        let message = BrowserObservationMessage {
            version: 1,
            instance_id: Uuid::new_v4(),
            state: WindowState::Focused,
            url: Some("https://example.com/app".to_string()),
            title: None,
            incognito: false,
        };
        assert!(!apply_message(
            &mut registry,
            &mut tabs,
            message,
            Some((303, ContextWindowKind::Browser)),
            Instant::now(),
        ));
        assert!(registry.live_sources(Instant::now()).is_empty());
    }

    #[test]
    fn parses_v1_observe_and_v2_invoke() {
        let observe = parse_browser_message(
            format!(
                r#"{{"version":1,"instanceId":"{INSTANCE}","state":"focused","url":"http://localhost:5173/"}}"#
            )
            .as_bytes(),
        );
        match observe {
            Some(BrowserHostMessage::Observe(message)) => {
                assert_eq!(message.version, 1);
                assert_eq!(message.url.as_deref(), Some("http://localhost:5173/"));
            }
            other => panic!("expected observe, got {other:?}"),
        }

        let invoke = parse_browser_message(
            format!(
                r#"{{"version":2,"kind":"invoke","instanceId":"{INSTANCE}","requestId":"{REQUEST}","url":"file:///tmp/app"}}"#
            )
            .as_bytes(),
        );
        match invoke {
            Some(BrowserHostMessage::Invoke(message)) => {
                assert_eq!(message.kind, "invoke");
                assert_eq!(message.url.as_deref(), Some("file:///tmp/app"));
                let observation = observation_from_invoke(message);
                assert_eq!(observation.state, WindowState::Focused);
                assert_eq!(observation.url.as_deref(), Some("file:///tmp/app"));
            }
            other => panic!("expected invoke, got {other:?}"),
        }
    }

    #[test]
    fn rejects_client_supplied_native_ids() {
        assert!(
            parse_browser_message(
                format!(
                    r#"{{"version":2,"kind":"invoke","instanceId":"{INSTANCE}","requestId":"{REQUEST}","url":"http://localhost:5173/","windowId":42}}"#
                )
                .as_bytes()
            )
            .is_none()
        );
        assert!(
            parse_browser_message(
                format!(
                    r#"{{"version":2,"kind":"invoke","instanceId":"{INSTANCE}","requestId":"{REQUEST}","cwd":"/tmp/app"}}"#
                )
                .as_bytes()
            )
            .is_none()
        );
        assert!(
            parse_browser_message(
                format!(
                    r#"{{"version":1,"instanceId":"{INSTANCE}","state":"focused","url":"http://localhost:5173/","processId":9}}"#
                )
                .as_bytes()
            )
            .is_none()
        );
    }

    #[test]
    fn replayed_invoke_generation_is_ignored() {
        let request = Uuid::parse_str(REQUEST).unwrap();
        let mut seen = HashSet::new();
        assert!(admit_invoke(&mut seen, request));
        assert!(!admit_invoke(&mut seen, request));
        assert!(admit_invoke(&mut seen, Uuid::new_v4()));
    }
}
