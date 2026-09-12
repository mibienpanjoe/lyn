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
    platform::{WindowCorrelationToken, x11::ContextWindowKind},
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
                    .then(crate::platform::x11::focused_editor_window)
                    .flatten();
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
    windows: &mut HashMap<
        Uuid,
        (
            WindowCorrelationToken,
            ContextProviderKind,
            ProviderSourceKind,
        ),
    >,
    message: VscodeObservationMessage,
    active_window: Option<(u32, ContextWindowKind)>,
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
    let (window, provider, source_kind) = match message.state {
        WindowState::Focused => {
            match active_window {
                Some((active_window, window_kind)) => {
                    let (provider, source_kind) = match window_kind {
                        ContextWindowKind::Cursor => (
                            ContextProviderKind::Cursor,
                            ProviderSourceKind::CursorWindow,
                        ),
                        _ => (
                            ContextProviderKind::Vscode,
                            ProviderSourceKind::VscodeWindow,
                        ),
                    };
                    let window = WindowCorrelationToken::from_native(u64::from(active_window));
                    claim_window(
                        windows,
                        registry,
                        message.instance_id,
                        window,
                        provider,
                        source_kind,
                        now,
                        &mut removed_previous_window,
                    );
                    (window, provider, source_kind)
                }
                // Opening Lyn takes OS focus. Keep this instance on its last
                // correlated window instead of dropping the observation or
                // inheriting another Cursor workspace's window.
                None => match windows.get(&message.instance_id).copied() {
                    Some(existing) => existing,
                    None => return false,
                },
            }
        }
        WindowState::Unfocused | WindowState::Ended => {
            let Some((window, provider, source_kind)) = windows.get(&message.instance_id).copied()
            else {
                return false;
            };
            (window, provider, source_kind)
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
            provider,
            source_kind,
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

fn claim_window(
    windows: &mut HashMap<
        Uuid,
        (
            WindowCorrelationToken,
            ContextProviderKind,
            ProviderSourceKind,
        ),
    >,
    registry: &mut ContextSourceRegistry,
    instance_id: Uuid,
    window: WindowCorrelationToken,
    provider: ContextProviderKind,
    source_kind: ProviderSourceKind,
    now: Instant,
    removed_previous_window: &mut bool,
) {
    let displaced: Vec<_> = windows
        .iter()
        .filter(|(id, (claimed, _, _))| **id != instance_id && *claimed == window)
        .map(|(id, value)| (*id, *value))
        .collect();
    for (other_id, (claimed, other_provider, other_kind)) in displaced {
        windows.remove(&other_id);
        registry.register(
            ProviderObservation::new(
                other_provider,
                other_kind,
                Some(claimed),
                None,
                None,
                PathBuf::from("/"),
                now,
                ObservationLiveness::Ended,
            ),
            now,
        );
        *removed_previous_window = true;
    }

    if let Some((previous, prev_provider, prev_source)) =
        windows.insert(instance_id, (window, provider, source_kind))
        && previous != window
    {
        registry.register(
            ProviderObservation::new(
                prev_provider,
                prev_source,
                Some(previous),
                None,
                None,
                PathBuf::from("/"),
                now,
                ObservationLiveness::Ended,
            ),
            now,
        );
        *removed_previous_window = true;
    }
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
        platform::{WindowCorrelationToken, x11::ContextWindowKind},
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
            Some((101, ContextWindowKind::Vscode)),
            now,
        ));
        assert!(apply_message(
            &mut registry,
            &mut windows,
            message(second.path(), WindowState::Focused),
            Some((202, ContextWindowKind::Cursor)),
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
            Some((303, ContextWindowKind::Vscode)),
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
    fn cursor_focused_workspace_resolves_only_for_its_invocation_window() {
        let directory = tempdir().unwrap();
        let mut registry = ContextSourceRegistry::default();
        let mut windows = HashMap::new();
        let now = Instant::now();
        let window = WindowCorrelationToken::from_native(404);

        assert!(apply_message(
            &mut registry,
            &mut windows,
            message(directory.path(), WindowState::Focused),
            Some((404, ContextWindowKind::Cursor)),
            now,
        ));

        let sources = registry.live_sources(now);
        let source = sources[0];
        assert_eq!(source.provider(), ContextProviderKind::Cursor);
        assert_eq!(source.application_name(), "Cursor");
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

        assert_eq!(
            resolve(&[exact], &[ContextProviderKind::Cursor]),
            ResolutionOutcome::Resolved(source.source_id())
        );
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

        apply_message(
            &mut registry,
            &mut windows,
            focused(),
            Some((11, ContextWindowKind::Vscode)),
            now,
        );
        apply_message(
            &mut registry,
            &mut windows,
            focused(),
            Some((22, ContextWindowKind::Vscode)),
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
    fn focused_without_os_window_keeps_this_instance_not_another_cursor_window() {
        let lyn = tempdir().unwrap();
        let other = tempdir().unwrap();
        fs::create_dir(lyn.path().join(".git")).unwrap();
        fs::create_dir(other.path().join(".git")).unwrap();
        let lyn_id = Uuid::new_v4();
        let other_id = Uuid::new_v4();
        let mut registry = ContextSourceRegistry::default();
        let mut windows = HashMap::new();
        let now = Instant::now();
        let focused = |instance_id, directory: &std::path::Path| VscodeObservationMessage {
            version: 1,
            instance_id,
            state: WindowState::Focused,
            workspace_folders: vec![directory.display().to_string()],
        };

        apply_message(
            &mut registry,
            &mut windows,
            focused(other_id, other.path()),
            Some((101, ContextWindowKind::Cursor)),
            now,
        );
        apply_message(
            &mut registry,
            &mut windows,
            focused(lyn_id, lyn.path()),
            Some((202, ContextWindowKind::Cursor)),
            now,
        );
        apply_message(
            &mut registry,
            &mut windows,
            focused(lyn_id, lyn.path()),
            None,
            now + std::time::Duration::from_secs(1),
        );

        let sources = registry.live_sources(now + std::time::Duration::from_secs(1));
        assert_eq!(sources.len(), 2);
        let lyn_source = sources
            .iter()
            .find(|source| source.window() == Some(WindowCorrelationToken::from_native(202)))
            .expect("lyn window stays correlated");
        assert_eq!(
            lyn_source.identity().project_path,
            lyn.path().canonicalize().unwrap().to_string_lossy()
        );
        assert!(
            sources.iter().any(|source| {
                source.window() == Some(WindowCorrelationToken::from_native(101))
            })
        );
    }

    #[test]
    fn claiming_a_cursor_window_evicts_the_other_instance() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        fs::create_dir(first.path().join(".git")).unwrap();
        fs::create_dir(second.path().join(".git")).unwrap();
        let first_id = Uuid::new_v4();
        let second_id = Uuid::new_v4();
        let mut registry = ContextSourceRegistry::default();
        let mut windows = HashMap::new();
        let now = Instant::now();
        let focused = |instance_id, directory: &std::path::Path| VscodeObservationMessage {
            version: 1,
            instance_id,
            state: WindowState::Focused,
            workspace_folders: vec![directory.display().to_string()],
        };

        apply_message(
            &mut registry,
            &mut windows,
            focused(first_id, first.path()),
            Some((101, ContextWindowKind::Cursor)),
            now,
        );
        apply_message(
            &mut registry,
            &mut windows,
            focused(second_id, second.path()),
            Some((101, ContextWindowKind::Cursor)),
            now,
        );

        let sources = registry.live_sources(now);
        assert_eq!(sources.len(), 1);
        assert_eq!(
            sources[0].window(),
            Some(WindowCorrelationToken::from_native(101))
        );
        assert_eq!(
            sources[0].identity().project_path,
            second.path().canonicalize().unwrap().to_string_lossy()
        );
        assert!(!windows.contains_key(&first_id));
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
        apply_message(
            &mut registry,
            &mut windows,
            live,
            Some((42, ContextWindowKind::Vscode)),
            now,
        );
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
    fn focused_heartbeat_without_a_window_mapping_is_ignored() {
        let directory = tempdir().unwrap();
        let mut registry = ContextSourceRegistry::default();
        let mut windows = HashMap::new();
        let now = Instant::now();

        assert!(!apply_message(
            &mut registry,
            &mut windows,
            message(directory.path(), WindowState::Focused),
            None,
            now,
        ));
        assert!(registry.live_sources(now).is_empty());
        assert!(windows.is_empty());
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
            Some((42, ContextWindowKind::Vscode)),
            now,
        ));
        assert!(registry.live_sources(now).is_empty());
    }
}
