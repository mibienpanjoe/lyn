//! Private shell and terminal observation broker for Linux X11.

use std::{
    collections::HashMap,
    fs,
    io::{self, Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    process::ExitCode,
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::{
    context::{
        provider::{
            CorrelationToken, ObservationLiveness, ProviderObservation, ProviderSourceKind,
        },
        session_registry::ContextSourceRegistry,
    },
    contract::ContextProviderKind,
    platform::{WindowCorrelationToken, x11::ContextWindowKind},
};

const SOCKET_NAME: &str = "lyn-shell-v1.sock";
const MAX_MESSAGE_BYTES: u64 = 4 * 1024;
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ShellState {
    Live,
    Ended,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ShellObservationMessage {
    version: u8,
    session_id: Uuid,
    process_id: u32,
    window_id: u32,
    state: ShellState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum KittyState {
    Focused,
    Ended,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KittyObservationMessage {
    version: u8,
    terminal_session_id: u64,
    process_id: u32,
    state: KittyState,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TerminalObservationMessage {
    Shell(ShellObservationMessage),
    Kitty(KittyObservationMessage),
}

#[derive(Clone, Copy)]
struct ShellSessionBinding {
    process_id: u32,
    window_id: u32,
    window_kind: ContextWindowKind,
}

#[derive(Clone, Copy)]
struct KittyPaneBinding {
    process_id: u32,
    window_id: u32,
}

pub(crate) fn start(app: AppHandle) -> io::Result<()> {
    let (path, runtime_uid) = socket_path()?;
    prepare_socket_path(&path)?;
    let listener = UnixListener::bind(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    thread::Builder::new()
        .name("lyn-shell-provider".to_owned())
        .spawn(move || run(listener, app, runtime_uid))?;
    Ok(())
}

fn run(listener: UnixListener, app: AppHandle, runtime_uid: u32) {
    let mut sessions = HashMap::new();
    let mut kitty_panes = HashMap::new();
    let mut active_kitty_panes = HashMap::new();
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut bytes = Vec::new();
                if Read::by_ref(&mut stream)
                    .take(MAX_MESSAGE_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .is_err()
                    || bytes.len() as u64 > MAX_MESSAGE_BYTES
                {
                    continue;
                }
                let Ok(message) = serde_json::from_slice::<TerminalObservationMessage>(&bytes)
                else {
                    continue;
                };
                let registry_state = app.state::<std::sync::Mutex<ContextSourceRegistry>>();
                let Ok(mut registry) = registry_state.lock() else {
                    continue;
                };
                let changed = match message {
                    TerminalObservationMessage::Shell(message) => {
                        let active_window = (message.state == ShellState::Live)
                            .then(crate::platform::x11::active_context_window)
                            .and_then(Result::ok);
                        let directory = (message.state == ShellState::Live)
                            .then(|| process_directory(message.process_id, runtime_uid))
                            .and_then(Result::ok);
                        apply_message(
                            &mut registry,
                            &mut sessions,
                            message,
                            active_window,
                            directory,
                            Instant::now(),
                        )
                    }
                    TerminalObservationMessage::Kitty(message) => {
                        let active_window = (message.state == KittyState::Focused)
                            .then(crate::platform::x11::active_context_window)
                            .and_then(Result::ok);
                        let directory = (message.state == KittyState::Focused)
                            .then(|| process_directory(message.process_id, runtime_uid))
                            .and_then(Result::ok);
                        apply_kitty_message(
                            &mut registry,
                            &mut kitty_panes,
                            &mut active_kitty_panes,
                            message,
                            active_window,
                            directory,
                            Instant::now(),
                        )
                    }
                };
                drop(registry);
                if changed {
                    emit_sources_changed(&app);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(_) => thread::sleep(ACCEPT_POLL_INTERVAL),
        }
    }
}

fn apply_kitty_message(
    registry: &mut ContextSourceRegistry,
    panes: &mut HashMap<u64, KittyPaneBinding>,
    active_panes: &mut HashMap<u32, u64>,
    message: KittyObservationMessage,
    active_window: Option<(u32, ContextWindowKind)>,
    directory: Option<PathBuf>,
    now: Instant,
) -> bool {
    if message.version != 1 || message.terminal_session_id == 0 || message.process_id == 0 {
        return false;
    }

    match message.state {
        KittyState::Focused => {
            let Some((window_id, ContextWindowKind::Kitty)) = active_window else {
                return false;
            };
            let Some(directory) = directory else {
                return false;
            };

            if let Some(previous_terminal_id) = active_panes.get(&window_id).copied()
                && previous_terminal_id != message.terminal_session_id
                && let Some(previous) = panes.remove(&previous_terminal_id)
            {
                register_kitty_observation(
                    registry,
                    previous_terminal_id,
                    previous,
                    PathBuf::from("/"),
                    ObservationLiveness::Ended,
                    now,
                );
            }
            if let Some(previous) = panes.get(&message.terminal_session_id).copied()
                && (previous.process_id != message.process_id || previous.window_id != window_id)
            {
                register_kitty_observation(
                    registry,
                    message.terminal_session_id,
                    previous,
                    PathBuf::from("/"),
                    ObservationLiveness::Ended,
                    now,
                );
            }

            let binding = KittyPaneBinding {
                process_id: message.process_id,
                window_id,
            };
            panes.insert(message.terminal_session_id, binding);
            active_panes.insert(window_id, message.terminal_session_id);
            register_kitty_observation(
                registry,
                message.terminal_session_id,
                binding,
                directory,
                ObservationLiveness::Live,
                now,
            )
            .is_some()
        }
        KittyState::Ended => {
            let Some(binding) = panes.remove(&message.terminal_session_id) else {
                return false;
            };
            if binding.process_id != message.process_id {
                panes.insert(message.terminal_session_id, binding);
                return false;
            }
            active_panes.retain(|_, terminal_id| *terminal_id != message.terminal_session_id);
            register_kitty_observation(
                registry,
                message.terminal_session_id,
                binding,
                PathBuf::from("/"),
                ObservationLiveness::Ended,
                now,
            );
            true
        }
    }
}

fn register_kitty_observation(
    registry: &mut ContextSourceRegistry,
    terminal_session_id: u64,
    binding: KittyPaneBinding,
    directory: PathBuf,
    liveness: ObservationLiveness,
    now: Instant,
) -> Option<crate::contract::ContextSourceId> {
    registry.register(
        ProviderObservation::new(
            ContextProviderKind::Shell,
            ProviderSourceKind::ExternalTerminal,
            Some(WindowCorrelationToken::from_native(u64::from(
                binding.window_id,
            ))),
            Some(CorrelationToken::from_process_id(binding.process_id)),
            Some(CorrelationToken::from_terminal_id(terminal_session_id)),
            directory,
            now,
            liveness,
        ),
        now,
    )
}

fn emit_sources_changed(app: &AppHandle) {
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

fn process_directory(process_id: u32, expected_uid: u32) -> io::Result<PathBuf> {
    process_directory_at(std::path::Path::new("/proc"), process_id, expected_uid)
}

fn process_directory_at(
    proc_root: &std::path::Path,
    process_id: u32,
    expected_uid: u32,
) -> io::Result<PathBuf> {
    let process = proc_root.join(process_id.to_string());
    if fs::metadata(&process)?.uid() != expected_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "shell process belongs to another user",
        ));
    }
    let directory = fs::canonicalize(process.join("cwd"))?;
    if !directory.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "shell working directory is unavailable",
        ));
    }
    Ok(directory)
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

fn prepare_socket_path(path: &std::path::Path) -> io::Result<()> {
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

pub(crate) fn run_helper() -> ExitCode {
    match watch_shell(std::env::args_os().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::FAILURE,
    }
}

fn watch_shell(arguments: Vec<std::ffi::OsString>) -> Result<(), ()> {
    let [command, process_flag, process_id] = arguments.as_slice() else {
        return Err(());
    };
    if command != "watch" || process_flag != "--process" {
        return Err(());
    }
    let process_id = process_id
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value != 0)
        .ok_or(())?;
    let process_path = PathBuf::from("/proc").join(process_id.to_string());
    let process = fs::metadata(&process_path).map_err(|_| ())?;
    let process_uid = process.uid();
    let process_inode = process.ino();
    let (window_id, _) = crate::platform::x11::active_context_window().map_err(|_| ())?;
    let session_id = Uuid::new_v4();

    loop {
        let process_is_same = fs::metadata(&process_path)
            .is_ok_and(|metadata| metadata.uid() == process_uid && metadata.ino() == process_inode);
        if !process_is_same {
            let _ = send_message(&ShellObservationMessage {
                version: 1,
                session_id,
                process_id,
                window_id,
                state: ShellState::Ended,
            });
            return Ok(());
        }

        if crate::platform::x11::active_context_window()
            .is_ok_and(|(active_window, _)| active_window == window_id)
        {
            let _ = send_message(&ShellObservationMessage {
                version: 1,
                session_id,
                process_id,
                window_id,
                state: ShellState::Live,
            });
        }
        thread::sleep(HEARTBEAT_INTERVAL);
    }
}

fn send_message(message: &ShellObservationMessage) -> io::Result<()> {
    let (path, _) = socket_path()?;
    let bytes = serde_json::to_vec(message)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid provider message"))?;
    let mut stream = UnixStream::connect(path)?;
    stream.set_write_timeout(Some(Duration::from_millis(750)))?;
    stream.write_all(&bytes)?;
    stream.shutdown(std::net::Shutdown::Write)
}

fn apply_message(
    registry: &mut ContextSourceRegistry,
    sessions: &mut HashMap<Uuid, ShellSessionBinding>,
    message: ShellObservationMessage,
    active_window: Option<(u32, ContextWindowKind)>,
    directory: Option<PathBuf>,
    now: Instant,
) -> bool {
    if message.version != 1 || message.process_id == 0 || message.window_id == 0 {
        return false;
    }

    let binding = match message.state {
        ShellState::Live => {
            let Some((active_window_id, window_kind)) = active_window else {
                return false;
            };
            if active_window_id != message.window_id {
                return false;
            }
            let proposed = ShellSessionBinding {
                process_id: message.process_id,
                window_id: message.window_id,
                window_kind,
            };
            if sessions.get(&message.session_id).is_some_and(|existing| {
                existing.process_id != proposed.process_id
                    || existing.window_id != proposed.window_id
                    || existing.window_kind != proposed.window_kind
            }) {
                return false;
            }
            sessions.insert(message.session_id, proposed);
            proposed
        }
        ShellState::Ended => {
            let Some(existing) = sessions.get(&message.session_id).copied() else {
                return false;
            };
            if existing.process_id != message.process_id || existing.window_id != message.window_id
            {
                return false;
            }
            existing
        }
    };

    let (provider, source_kind) = match binding.window_kind {
        ContextWindowKind::Vscode => (
            ContextProviderKind::Vscode,
            ProviderSourceKind::VscodeIntegratedTerminal,
        ),
        ContextWindowKind::GnomeTerminal | ContextWindowKind::Kitty => (
            ContextProviderKind::Shell,
            ProviderSourceKind::ExternalTerminal,
        ),
    };
    let directory = match message.state {
        ShellState::Live => {
            let Some(directory) = directory else {
                return false;
            };
            directory
        }
        ShellState::Ended => PathBuf::from("/"),
    };
    let liveness = match message.state {
        ShellState::Live => ObservationLiveness::Live,
        ShellState::Ended => ObservationLiveness::Ended,
    };
    let changed = registry
        .register(
            ProviderObservation::new(
                provider,
                source_kind,
                Some(WindowCorrelationToken::from_native(u64::from(
                    binding.window_id,
                ))),
                Some(CorrelationToken::from_process_id(binding.process_id)),
                Some(CorrelationToken::from_session_id(message.session_id)),
                directory,
                now,
                liveness,
            ),
            now,
        )
        .is_some()
        || liveness == ObservationLiveness::Ended;
    if message.state == ShellState::Ended {
        sessions.remove(&message.session_id);
    }
    changed
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

    use super::{
        KittyObservationMessage, KittyState, ShellObservationMessage, ShellState,
        apply_kitty_message, apply_message,
    };

    fn live_message(process_id: u32, window_id: u32) -> ShellObservationMessage {
        ShellObservationMessage {
            version: 1,
            session_id: Uuid::new_v4(),
            process_id,
            window_id,
            state: ShellState::Live,
        }
    }

    #[test]
    fn external_terminal_observation_resolves_only_for_its_exact_active_window() {
        let directory = tempdir().unwrap();
        fs::create_dir(directory.path().join(".git")).unwrap();
        let now = Instant::now();
        let mut registry = ContextSourceRegistry::default();
        let mut sessions = HashMap::new();
        let message = live_message(std::process::id(), 71);

        assert!(apply_message(
            &mut registry,
            &mut sessions,
            message,
            Some((71, ContextWindowKind::GnomeTerminal)),
            Some(directory.path().to_path_buf()),
            now,
        ));

        let source = registry.live_sources(now)[0];
        let candidate = classify(
            source.source_id(),
            source.provider(),
            source.window(),
            source.process(),
            source.session(),
            &InvocationAssociations {
                foreground_window: Some(WindowCorrelationToken::from_native(71)),
                related_processes: &[],
                related_sessions: &[],
                inferred_windows: &[],
            },
        )
        .unwrap();

        assert_eq!(
            resolve(&[candidate], &[ContextProviderKind::Shell]),
            ResolutionOutcome::Resolved(source.source_id())
        );
    }

    #[test]
    fn integrated_terminal_is_kept_distinct_from_the_editor_window_provider() {
        let directory = tempdir().unwrap();
        let now = Instant::now();
        let mut registry = ContextSourceRegistry::default();
        let mut sessions = HashMap::new();

        assert!(apply_message(
            &mut registry,
            &mut sessions,
            live_message(std::process::id(), 72),
            Some((72, ContextWindowKind::Vscode)),
            Some(directory.path().to_path_buf()),
            now,
        ));

        let source = registry.live_sources(now)[0];
        assert_eq!(source.provider(), ContextProviderKind::Vscode);
        assert_eq!(source.application_name(), "VS Code");
    }

    #[test]
    fn mismatched_or_unsupported_foreground_evidence_is_rejected() {
        let directory = tempdir().unwrap();
        let now = Instant::now();
        let mut registry = ContextSourceRegistry::default();
        let mut sessions = HashMap::new();

        assert!(!apply_message(
            &mut registry,
            &mut sessions,
            live_message(std::process::id(), 73),
            Some((74, ContextWindowKind::Kitty)),
            Some(directory.path().to_path_buf()),
            now,
        ));
        assert!(registry.live_sources(now).is_empty());
    }

    #[test]
    fn two_terminal_sessions_for_one_window_resolve_as_ambiguous() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let now = Instant::now();
        let mut registry = ContextSourceRegistry::default();
        let mut sessions = HashMap::new();

        for directory in [first.path(), second.path()] {
            assert!(apply_message(
                &mut registry,
                &mut sessions,
                live_message(std::process::id(), 75),
                Some((75, ContextWindowKind::Kitty)),
                Some(directory.to_path_buf()),
                now,
            ));
        }

        let foreground = WindowCorrelationToken::from_native(75);
        let candidates: Vec<_> = registry
            .live_sources(now)
            .into_iter()
            .filter_map(|source| {
                classify(
                    source.source_id(),
                    source.provider(),
                    source.window(),
                    source.process(),
                    source.session(),
                    &InvocationAssociations {
                        foreground_window: Some(foreground),
                        related_processes: &[],
                        related_sessions: &[],
                        inferred_windows: &[],
                    },
                )
            })
            .collect();

        assert_eq!(
            resolve(&candidates, &[ContextProviderKind::Shell]),
            ResolutionOutcome::Ambiguous
        );
    }

    #[test]
    fn wire_message_has_no_path_command_output_or_environment_fields() {
        let encoded = serde_json::to_value(live_message(std::process::id(), 76)).unwrap();
        let object = encoded.as_object().unwrap();

        assert_eq!(
            object
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            ["processId", "sessionId", "state", "version", "windowId"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        );
    }

    #[test]
    fn kitty_focus_switch_replaces_the_previous_pane_for_one_os_window() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let now = Instant::now();
        let mut registry = ContextSourceRegistry::default();
        let mut panes = HashMap::new();
        let mut active_panes = HashMap::new();
        let focused = |terminal_session_id, process_id| KittyObservationMessage {
            version: 1,
            terminal_session_id,
            process_id,
            state: KittyState::Focused,
        };

        assert!(apply_kitty_message(
            &mut registry,
            &mut panes,
            &mut active_panes,
            focused(501, 1001),
            Some((81, ContextWindowKind::Kitty)),
            Some(first.path().to_path_buf()),
            now,
        ));
        assert!(apply_kitty_message(
            &mut registry,
            &mut panes,
            &mut active_panes,
            focused(502, 1002),
            Some((81, ContextWindowKind::Kitty)),
            Some(second.path().to_path_buf()),
            now,
        ));

        let sources = registry.live_sources(now);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].label(), second.path().file_name().unwrap());
    }

    #[test]
    fn kitty_message_contains_only_focus_correlation_fields() {
        let encoded = serde_json::to_value(KittyObservationMessage {
            version: 1,
            terminal_session_id: 503,
            process_id: 1003,
            state: KittyState::Focused,
        })
        .unwrap();
        let object = encoded.as_object().unwrap();

        assert_eq!(
            object
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            ["processId", "state", "terminalSessionId", "version"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        );
    }
}
