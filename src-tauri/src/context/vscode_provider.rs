//! Local, bounded VS Code workspace observations over a user-only Unix socket.

use std::{
    collections::HashMap,
    fs,
    io::{self, Read},
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
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
        provider::{ObservationLiveness, ProviderObservation, ProviderSourceKind},
        session_registry::ContextSourceRegistry,
    },
    contract::ContextProviderKind,
    platform::WindowCorrelationToken,
};

const SOCKET_NAME: &str = "lyn-context-v1.sock";
const MAX_MESSAGE_BYTES: u64 = 16 * 1024;
const MAX_WORKSPACE_PATH_BYTES: usize = 4 * 1024;
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WindowState {
    Focused,
    Unfocused,
    Ended,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VscodeObservationMessage {
    version: u8,
    instance_id: Uuid,
    state: WindowState,
    workspace_folders: Vec<String>,
}

pub(crate) fn start(app: AppHandle) -> io::Result<()> {
    let path = socket_path()?;
    prepare_socket_path(&path)?;
    let listener = UnixListener::bind(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    thread::Builder::new()
        .name("lyn-vscode-provider".to_owned())
        .spawn(move || run(listener, app))?;
    Ok(())
}

fn run(listener: UnixListener, app: AppHandle) {
    let mut windows = HashMap::new();
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut bytes = Vec::new();
                if stream
                    .by_ref()
                    .take(MAX_MESSAGE_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .is_err()
                    || bytes.len() as u64 > MAX_MESSAGE_BYTES
                {
                    continue;
                }
                let Ok(message) = serde_json::from_slice::<VscodeObservationMessage>(&bytes) else {
                    continue;
                };
                let active_window = (message.state == WindowState::Focused)
                    .then(crate::platform::x11::active_vscode_window)
                    .and_then(Result::ok);
                let registry_state = app.state::<std::sync::Mutex<ContextSourceRegistry>>();
                let Ok(mut registry) = registry_state.lock() else {
                    continue;
                };
                let changed = apply_message(
                    &mut registry,
                    &mut windows,
                    message,
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
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(_) => thread::sleep(ACCEPT_POLL_INTERVAL),
        }
    }
}

fn apply_message(
    registry: &mut ContextSourceRegistry,
    windows: &mut HashMap<Uuid, WindowCorrelationToken>,
    message: VscodeObservationMessage,
    active_window: Option<u32>,
    now: Instant,
) -> bool {
    if message.version != 1
        || message.workspace_folders.len() > 1
        || message
            .workspace_folders
            .iter()
            .any(|path| path.is_empty() || path.len() > MAX_WORKSPACE_PATH_BYTES)
    {
        return false;
    }

    let mut removed_previous_window = false;
    let window = match message.state {
        WindowState::Focused => {
            let Some(active_window) = active_window else {
                return false;
            };
            let window = WindowCorrelationToken::from_native(u64::from(active_window));
            if let Some(previous) = windows.insert(message.instance_id, window)
                && previous != window
            {
                registry.register(
                    ProviderObservation::new(
                        ContextProviderKind::Vscode,
                        ProviderSourceKind::VscodeWindow,
                        Some(previous),
                        None,
                        None,
                        PathBuf::from("/"),
                        now,
                        ObservationLiveness::Ended,
                    ),
                    now,
                );
                removed_previous_window = true;
            }
            window
        }
        WindowState::Unfocused | WindowState::Ended => {
            let Some(window) = windows.get(&message.instance_id).copied() else {
                return false;
            };
            window
        }
    };

    let liveness = if message.state == WindowState::Ended || message.workspace_folders.is_empty() {
        ObservationLiveness::Ended
    } else {
        ObservationLiveness::Live
    };
    let directory = message
        .workspace_folders
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    let registered = registry.register(
        ProviderObservation::new(
            ContextProviderKind::Vscode,
            ProviderSourceKind::VscodeWindow,
            Some(window),
            None,
            None,
            directory,
            now,
            liveness,
        ),
        now,
    );
    let changed =
        removed_previous_window || registered.is_some() || liveness == ObservationLiveness::Ended;
    if message.state == WindowState::Ended {
        windows.remove(&message.instance_id);
    }
    changed
}

fn socket_path() -> io::Result<PathBuf> {
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
    Ok(runtime.join(SOCKET_NAME))
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

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, fs, time::Instant};

    use tempfile::tempdir;
    use uuid::Uuid;

    use crate::{
        context::{
            resolver::{InvocationAssociations, ResolutionOutcome, classify, resolve},
            session_registry::ContextSourceRegistry,
        },
        contract::ContextProviderKind,
        platform::WindowCorrelationToken,
    };

    use super::{VscodeObservationMessage, WindowState, apply_message};

    fn message(directory: &std::path::Path, state: WindowState) -> VscodeObservationMessage {
        VscodeObservationMessage {
            version: 1,
            instance_id: Uuid::new_v4(),
            state,
            workspace_folders: vec![directory.display().to_string()],
        }
    }

    #[test]
    fn focused_windows_register_exact_distinct_x11_sources() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        fs::create_dir(first.path().join(".git")).unwrap();
        fs::create_dir(second.path().join(".git")).unwrap();
        let mut registry = ContextSourceRegistry::default();
        let mut windows = HashMap::new();
        let now = Instant::now();

        assert!(apply_message(
            &mut registry,
            &mut windows,
            message(first.path(), WindowState::Focused),
            Some(101),
            now,
        ));
        assert!(apply_message(
            &mut registry,
            &mut windows,
            message(second.path(), WindowState::Focused),
            Some(202),
            now,
        ));

        let sources = registry.live_sources(now);
        assert_eq!(sources.len(), 2);
        assert!(
            sources.iter().any(|source| {
                source.window() == Some(WindowCorrelationToken::from_native(101))
            })
        );
        assert!(
            sources.iter().any(|source| {
                source.window() == Some(WindowCorrelationToken::from_native(202))
            })
        );
    }

    #[test]
    fn focused_workspace_resolves_only_for_its_invocation_window() {
        let directory = tempdir().unwrap();
        let mut registry = ContextSourceRegistry::default();
        let mut windows = HashMap::new();
        let now = Instant::now();
        let window = WindowCorrelationToken::from_native(303);

        assert!(apply_message(
            &mut registry,
            &mut windows,
            message(directory.path(), WindowState::Focused),
            Some(303),
            now,
        ));

        let sources = registry.live_sources(now);
        let source = sources[0];
        let exact = classify(
            source.source_id(),
            source.provider(),
            source.window(),
            source.process(),
            source.session(),
            &InvocationAssociations {
                foreground_window: Some(window),
                related_processes: &[],
                related_sessions: &[],
                inferred_windows: &[],
            },
        )
        .unwrap();
        let unrelated = classify(
            source.source_id(),
            source.provider(),
            source.window(),
            source.process(),
            source.session(),
            &InvocationAssociations {
                foreground_window: Some(WindowCorrelationToken::from_native(404)),
                related_processes: &[],
                related_sessions: &[],
                inferred_windows: &[],
            },
        );

        assert_eq!(
            resolve(&[exact], &[ContextProviderKind::Vscode]),
            ResolutionOutcome::Resolved(source.source_id())
        );
        assert!(unrelated.is_none());
    }

    #[test]
    fn remapping_one_extension_window_removes_its_previous_correlation() {
        let directory = tempdir().unwrap();
        let instance_id = Uuid::new_v4();
        let mut registry = ContextSourceRegistry::default();
        let mut windows = HashMap::new();
        let now = Instant::now();
        let focused = || VscodeObservationMessage {
            version: 1,
            instance_id,
            state: WindowState::Focused,
            workspace_folders: vec![directory.path().display().to_string()],
        };

        apply_message(&mut registry, &mut windows, focused(), Some(11), now);
        apply_message(
            &mut registry,
            &mut windows,
            focused(),
            Some(22),
            now + std::time::Duration::from_secs(1),
        );

        let sources = registry.live_sources(now + std::time::Duration::from_secs(1));
        assert_eq!(sources.len(), 1);
        assert_eq!(
            sources[0].window(),
            Some(WindowCorrelationToken::from_native(22))
        );
    }

    #[test]
    fn heartbeat_refreshes_one_source_and_ended_removes_it() {
        let directory = tempdir().unwrap();
        let instance_id = Uuid::new_v4();
        let mut registry = ContextSourceRegistry::default();
        let mut windows = HashMap::new();
        let now = Instant::now();
        let live = VscodeObservationMessage {
            version: 1,
            instance_id,
            state: WindowState::Focused,
            workspace_folders: vec![directory.path().display().to_string()],
        };
        apply_message(&mut registry, &mut windows, live, Some(42), now);
        let source_id = registry.live_sources(now)[0].source_id();

        apply_message(
            &mut registry,
            &mut windows,
            VscodeObservationMessage {
                version: 1,
                instance_id,
                state: WindowState::Unfocused,
                workspace_folders: vec![directory.path().display().to_string()],
            },
            None,
            now + std::time::Duration::from_secs(10),
        );
        assert_eq!(
            registry.live_sources(now + std::time::Duration::from_secs(10))[0].source_id(),
            source_id
        );

        apply_message(
            &mut registry,
            &mut windows,
            VscodeObservationMessage {
                version: 1,
                instance_id,
                state: WindowState::Ended,
                workspace_folders: vec![],
            },
            None,
            now + std::time::Duration::from_secs(11),
        );
        assert!(
            registry
                .live_sources(now + std::time::Duration::from_secs(11))
                .is_empty()
        );
    }

    #[test]
    fn multi_root_and_oversized_messages_are_rejected_without_sources() {
        let directory = tempdir().unwrap();
        let mut registry = ContextSourceRegistry::default();
        let mut windows = HashMap::new();
        let now = Instant::now();
        let rejected = VscodeObservationMessage {
            version: 1,
            instance_id: Uuid::new_v4(),
            state: WindowState::Focused,
            workspace_folders: vec![
                directory.path().display().to_string(),
                directory.path().display().to_string(),
            ],
        };

        assert!(!apply_message(
            &mut registry,
            &mut windows,
            rejected,
            Some(42),
            now,
        ));
        assert!(registry.live_sources(now).is_empty());
    }
}
