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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KittyInvokeMessage {
    version: u8,
    kind: String,
    request_id: Uuid,
    terminal_session_id: u64,
    process_id: u32,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TerminalObservationMessage {
    KittyInvoke(KittyInvokeMessage),
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

const KITTY_OWNER_TTL: Duration = Duration::from_secs(30);
const MAX_KITTY_OWNERS: usize = 64;

/// Bounded cache of X11 Kitty window -> owning client PID (XRes). An entry is
/// re-queried after its TTL so a reused window id cannot keep a stale owner.
#[derive(Default)]
struct KittyWindowOwners {
    entries: HashMap<u32, (Option<u32>, Instant)>,
}

impl KittyWindowOwners {
    fn owner(
        &mut self,
        window: u32,
        now: Instant,
        lookup: impl FnOnce(u32) -> Option<u32>,
    ) -> Option<u32> {
        if let Some((owner, checked)) = self.entries.get(&window)
            && now.duration_since(*checked) < KITTY_OWNER_TTL
        {
            return *owner;
        }
        if self.entries.len() >= MAX_KITTY_OWNERS {
            self.entries.clear();
        }
        let owner = lookup(window);
        self.entries.insert(window, (owner, now));
        owner
    }
}

/// Every Kitty process runs its own watcher and keeps reporting its last
/// focused pane, even while another Kitty process owns the foreground. A pane
/// may bind only to a Kitty window owned by its own Kitty process (the pane's
/// direct parent); otherwise two processes alternately replace each other's
/// source and an open draft's selection goes stale within one heartbeat.
/// Without XRes the previous single-process behaviour is kept.
fn attest_kitty_window(
    active: Option<(u32, ContextWindowKind)>,
    process_id: u32,
    parent_of: impl FnOnce(u32) -> Option<u32>,
    owners: &mut KittyWindowOwners,
    owner_of: impl FnOnce(u32) -> Option<u32>,
    now: Instant,
) -> Option<(u32, ContextWindowKind)> {
    let (window, kind) = active?;
    if kind != ContextWindowKind::Kitty {
        return Some((window, kind));
    }
    match owners.owner(window, now, owner_of) {
        None => Some((window, kind)),
        Some(owner) => (parent_of(process_id) == Some(owner)).then_some((window, kind)),
    }
}

/// Kitty numbers panes per process, so two Kitty processes both report pane 1.
/// Pane bindings are keyed by (Kitty process, pane id); the key is opaque.
fn process_pane_key(kitty_process: Option<u32>, terminal_session_id: u64) -> u64 {
    match (kitty_process, u32::try_from(terminal_session_id)) {
        (Some(process), Ok(pane)) => (u64::from(process) << 32) | u64::from(pane),
        _ => terminal_session_id,
    }
}

fn kitty_pane_key(
    panes: &HashMap<u64, KittyPaneBinding>,
    message: &KittyObservationMessage,
    parent_of: impl FnOnce(u32) -> Option<u32>,
) -> u64 {
    if message.state == KittyState::Ended {
        // The closing pane's process may already be gone; find its binding.
        if let Some(key) = panes.iter().find_map(|(key, binding)| {
            (binding.process_id == message.process_id
                && (*key == message.terminal_session_id
                    || *key & u64::from(u32::MAX) == message.terminal_session_id))
                .then_some(*key)
        }) {
            return key;
        }
    }
    process_pane_key(parent_of(message.process_id), message.terminal_session_id)
}

/// A new binding needs the active (or remembered) Kitty window to belong to
/// the pane's Kitty process. An existing binding keeps refreshing its own
/// window while another window has focus, so clicking a second Kitty window
/// with a draft open does not expire the invoked pane.
fn kitty_heartbeat_window(
    active: Option<(u32, ContextWindowKind)>,
    panes: &HashMap<u64, KittyPaneBinding>,
    message: &KittyObservationMessage,
    parent_of: impl Fn(u32) -> Option<u32>,
    owners: &mut KittyWindowOwners,
    owner_of: impl Fn(u32) -> Option<u32>,
    now: Instant,
) -> Option<(u32, ContextWindowKind)> {
    if let Some(window) = attest_kitty_window(
        active,
        message.process_id,
        &parent_of,
        owners,
        &owner_of,
        now,
    ) {
        return Some(window);
    }
    let binding = panes
        .get(&message.terminal_session_id)
        .filter(|binding| binding.process_id == message.process_id)?;
    attest_kitty_window(
        Some((binding.window_id, ContextWindowKind::Kitty)),
        message.process_id,
        &parent_of,
        owners,
        &owner_of,
        now,
    )
}

fn parent_process(process_id: u32) -> Option<u32> {
    let mut text = String::new();
    fs::File::open(format!("/proc/{process_id}/stat"))
        .ok()?
        .take(4097)
        .read_to_string(&mut text)
        .ok()?;
    if text.len() > 4096 {
        return None;
    }
    // comm (field 2) may contain spaces or parentheses; fields follow the last ')'.
    let (_, fields) = text.rsplit_once(')')?;
    fields
        .split_whitespace()
        .nth(1)?
        .parse::<u32>()
        .ok()
        .filter(|parent| *parent > 1)
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
    let mut kitty_owners = KittyWindowOwners::default();
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
                // X11/XRes and procfs lookups run before the registry lock.
                let message = match message {
                    TerminalObservationMessage::Kitty(message) => {
                        TerminalObservationMessage::Kitty(KittyObservationMessage {
                            terminal_session_id: kitty_pane_key(
                                &kitty_panes,
                                &message,
                                parent_process,
                            ),
                            ..message
                        })
                    }
                    other => other,
                };
                let kitty_window = match &message {
                    TerminalObservationMessage::KittyInvoke(message)
                        if message.version == 2 && message.kind == "invoke" =>
                    {
                        attest_kitty_window(
                            crate::platform::x11::active_context_window().ok(),
                            message.process_id,
                            parent_process,
                            &mut kitty_owners,
                            crate::platform::x11_owner::local_owner,
                            Instant::now(),
                        )
                    }
                    TerminalObservationMessage::Kitty(message)
                        if message.state == KittyState::Focused =>
                    {
                        kitty_heartbeat_window(
                            crate::platform::x11::active_context_window().ok(),
                            &kitty_panes,
                            message,
                            parent_process,
                            &mut kitty_owners,
                            crate::platform::x11_owner::local_owner,
                            Instant::now(),
                        )
                    }
                    _ => None,
                };
                let registry_state = app.state::<std::sync::Mutex<ContextSourceRegistry>>();
                let Ok(mut registry) = registry_state.lock() else {
                    continue;
                };
                let changed = match message {
                    TerminalObservationMessage::KittyInvoke(message) => {
                        if message.version != 2 || message.kind != "invoke" {
                            false
                        } else {
                            let active_window = kitty_window;
                            let directory = process_directory(message.process_id, runtime_uid).ok();
                            let focused = KittyObservationMessage {
                                version: 1,
                                terminal_session_id: process_pane_key(
                                    parent_process(message.process_id),
                                    message.terminal_session_id,
                                ),
                                process_id: message.process_id,
                                state: KittyState::Focused,
                            };
                            let changed = apply_kitty_message(
                                &mut registry,
                                &mut kitty_panes,
                                &mut active_kitty_panes,
                                focused,
                                active_window,
                                directory,
                                Instant::now(),
                            );
                            drop(registry);
                            if changed {
                                emit_sources_changed(&app);
                            }
                            let request_id = message.request_id;
                            let handle = app.clone();
                            let _ = app.run_on_main_thread(move || {
                                crate::invoke_capture_popup_with_request(&handle, Some(request_id));
                            });
                            continue;
                        }
                    }
                    TerminalObservationMessage::Shell(message) => {
                        let live = message.state == ShellState::Live;
                        let window_id = message.window_id;
                        let current = live
                            .then(crate::platform::x11::active_context_window)
                            .and_then(Result::ok);
                        let attested = live
                            .then(|| crate::platform::x11::classify_native_window(window_id))
                            .flatten();
                        let active_window = observation_window(live, window_id, current, attested);
                        if active_window.is_some_and(|(_, kind)| {
                            shell_observation_superseded_by_kitty(
                                kind,
                                message.window_id,
                                &active_kitty_panes,
                            )
                        }) {
                            false
                        } else {
                            let directory = live
                                .then(|| process_directory(message.process_id, runtime_uid))
                                .and_then(Result::ok);
                            let changed = apply_message(
                                &mut registry,
                                &mut sessions,
                                message,
                                active_window,
                                directory,
                                Instant::now(),
                            );
                            if changed && live {
                                if let Some((id, kind)) =
                                    foreground_shell_window(current, active_window)
                                {
                                    crate::platform::x11::remember_context_window(id, kind);
                                }
                            }
                            changed
                        }
                    }
                    TerminalObservationMessage::Kitty(message) => {
                        let active_window = kitty_window;
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

fn observation_window(
    live: bool,
    message_window_id: u32,
    current: Option<(u32, ContextWindowKind)>,
    attested: Option<(u32, ContextWindowKind)>,
) -> Option<(u32, ContextWindowKind)> {
    if !live {
        return None;
    }
    current
        .filter(|(id, _)| *id == message_window_id)
        .or_else(|| attested.filter(|(id, _)| *id == message_window_id))
}

fn shell_observation_superseded_by_kitty(
    window_kind: ContextWindowKind,
    window_id: u32,
    active_kitty_panes: &HashMap<u32, u64>,
) -> bool {
    window_kind == ContextWindowKind::Kitty && active_kitty_panes.contains_key(&window_id)
}

fn foreground_shell_window(
    current: Option<(u32, ContextWindowKind)>,
    observed: Option<(u32, ContextWindowKind)>,
) -> Option<(u32, ContextWindowKind)> {
    current.filter(|window| Some(*window) == observed)
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
    // A new helper has no binding yet. Lyn's remembered foreground window
    // must not attach a newly started shell to a different terminal.
    let (window_id, _) = crate::platform::x11::current_context_window().map_err(|_| ())?;
    let session_id = Uuid::new_v4();

    watch_shell_session(
        session_id,
        process_id,
        window_id,
        || {
            fs::metadata(&process_path).is_ok_and(|metadata| {
                metadata.uid() == process_uid && metadata.ino() == process_inode
            })
        },
        |message| {
            let _ = send_message(message);
        },
        || thread::sleep(HEARTBEAT_INTERVAL),
    );
    Ok(())
}

fn watch_shell_session(
    session_id: Uuid,
    process_id: u32,
    window_id: u32,
    mut process_is_same: impl FnMut() -> bool,
    mut publish: impl FnMut(&ShellObservationMessage),
    mut wait: impl FnMut(),
) {
    loop {
        if !process_is_same() {
            publish(&ShellObservationMessage {
                version: 1,
                session_id,
                process_id,
                window_id,
                state: ShellState::Ended,
            });
            return;
        }

        // Liveness belongs to the bound shell, not to OS focus. Otherwise an
        // idle terminal expires and a fast invocation races the next heartbeat.
        publish(&ShellObservationMessage {
            version: 1,
            session_id,
            process_id,
            window_id,
            state: ShellState::Live,
        });
        wait();
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
            // Kitty's pane watcher owns this window even if an inherited
            // GNOME marker caused the generic shell bootstrap to start.
            if active_window_id != message.window_id || window_kind == ContextWindowKind::Kitty {
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
        ContextWindowKind::Cursor => (
            ContextProviderKind::Cursor,
            ProviderSourceKind::CursorIntegratedTerminal,
        ),
        ContextWindowKind::GnomeTerminal => (
            ContextProviderKind::Shell,
            ProviderSourceKind::ExternalTerminal,
        ),
        ContextWindowKind::Kitty | ContextWindowKind::Browser => return false,
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
    use std::{
        cell::{Cell, RefCell},
        collections::HashMap,
        fs,
        os::unix::fs::{MetadataExt, symlink},
        time::{Duration, Instant},
    };

    use tempfile::tempdir;
    use uuid::Uuid;

    use crate::{
        context::{
            resolver::{InvocationAssociations, ResolutionOutcome, classify, resolve},
            session_registry::{ContextSourceRegistry, LIVE_SOURCE_TTL},
        },
        contract::ContextProviderKind,
        platform::{WindowCorrelationToken, x11::ContextWindowKind},
    };

    use super::{
        KITTY_OWNER_TTL, KittyObservationMessage, KittyPaneBinding, KittyState, KittyWindowOwners,
        ShellObservationMessage, ShellState, TerminalObservationMessage, apply_kitty_message,
        apply_message, attest_kitty_window, foreground_shell_window, kitty_heartbeat_window,
        kitty_pane_key, observation_window, parent_process, process_directory_at,
        shell_observation_superseded_by_kitty, watch_shell_session,
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
    fn background_shell_heartbeats_keep_the_bound_source_live_past_the_ttl() {
        let directory = tempdir().unwrap();
        let initial = Instant::now();
        let clock = Cell::new(initial);
        let registry = RefCell::new(ContextSourceRegistry::default());
        let mut sessions = HashMap::new();
        let message = live_message(std::process::id(), u32::MAX);
        let session_id = message.session_id;
        let process_id = message.process_id;
        let window_id = message.window_id;
        let bound_window = Some((window_id, ContextWindowKind::GnomeTerminal));
        assert!(apply_message(
            &mut registry.borrow_mut(),
            &mut sessions,
            message,
            bound_window,
            Some(directory.path().to_path_buf()),
            initial,
        ));

        // The synthetic bound window can never be the actual X11 foreground.
        // Run the production producer with fake time, keeping the process alive
        // longer than one TTL before ending it.
        watch_shell_session(
            session_id,
            process_id,
            window_id,
            || clock.get() < initial + LIVE_SOURCE_TTL + Duration::from_secs(12),
            |message| {
                let message =
                    serde_json::from_slice(&serde_json::to_vec(message).unwrap()).unwrap();
                apply_message(
                    &mut registry.borrow_mut(),
                    &mut sessions,
                    message,
                    bound_window,
                    Some(directory.path().to_path_buf()),
                    clock.get(),
                );
            },
            || {
                clock.set(clock.get() + super::HEARTBEAT_INTERVAL);
                if clock.get() == initial + LIVE_SOURCE_TTL + Duration::from_secs(10) {
                    let mut registry = registry.borrow_mut();
                    let sources = registry.live_sources(clock.get());
                    assert_eq!(
                        sources.len(),
                        1,
                        "live background shell expired before invocation"
                    );
                    assert_eq!(
                        sources[0].window(),
                        Some(WindowCorrelationToken::from_native(u64::from(window_id)))
                    );
                }
            },
        );
        assert!(registry.borrow_mut().live_sources(clock.get()).is_empty());
    }

    #[test]
    fn background_shell_cannot_replace_the_remembered_foreground_window() {
        let foreground = Some((71, ContextWindowKind::GnomeTerminal));
        let background = Some((72, ContextWindowKind::GnomeTerminal));
        assert_eq!(foreground_shell_window(foreground, background), None);
        assert_eq!(foreground_shell_window(None, background), None);
        assert_eq!(foreground_shell_window(foreground, foreground), foreground);
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
    fn cursor_integrated_terminal_is_attributed_to_cursor() {
        let directory = tempdir().unwrap();
        let now = Instant::now();
        let mut registry = ContextSourceRegistry::default();
        let mut sessions = HashMap::new();

        assert!(apply_message(
            &mut registry,
            &mut sessions,
            live_message(std::process::id(), 73),
            Some((73, ContextWindowKind::Cursor)),
            Some(directory.path().to_path_buf()),
            now,
        ));

        let source = registry.live_sources(now)[0];
        assert_eq!(source.provider(), ContextProviderKind::Cursor);
        assert_eq!(source.application_name(), "Cursor");
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
    fn generic_shell_cannot_claim_a_kitty_window_even_with_inherited_gnome_marker() {
        let directory = tempdir().unwrap();
        let now = Instant::now();
        let mut registry = ContextSourceRegistry::default();
        let mut sessions = HashMap::new();

        assert!(!apply_message(
            &mut registry,
            &mut sessions,
            live_message(std::process::id(), 74),
            Some((74, ContextWindowKind::Kitty)),
            Some(directory.path().to_path_buf()),
            now,
        ));
        assert!(registry.live_sources(now).is_empty());
    }

    #[test]
    fn attested_terminal_window_is_used_when_lyn_has_already_taken_focus() {
        assert_eq!(
            observation_window(
                true,
                91,
                Some((7, ContextWindowKind::Cursor)),
                Some((91, ContextWindowKind::GnomeTerminal)),
            ),
            Some((91, ContextWindowKind::GnomeTerminal))
        );
        assert_eq!(
            observation_window(
                true,
                91,
                Some((91, ContextWindowKind::GnomeTerminal)),
                Some((91, ContextWindowKind::GnomeTerminal)),
            ),
            Some((91, ContextWindowKind::GnomeTerminal))
        );
        assert_eq!(
            observation_window(
                false,
                91,
                Some((91, ContextWindowKind::GnomeTerminal)),
                Some((91, ContextWindowKind::GnomeTerminal)),
            ),
            None
        );
        assert_eq!(
            observation_window(true, 91, Some((7, ContextWindowKind::Cursor)), None),
            None
        );
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
                Some((75, ContextWindowKind::GnomeTerminal)),
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

    /// Drives the production key + window path for two Kitty processes
    /// (100: stipen, 200: lyn) that both number their single pane 1.
    struct TwoKittys {
        registry: ContextSourceRegistry,
        panes: HashMap<u64, KittyPaneBinding>,
        active_panes: HashMap<u32, u64>,
        owners: KittyWindowOwners,
        stipen: tempfile::TempDir,
        lyn: tempfile::TempDir,
    }

    impl TwoKittys {
        fn new() -> Self {
            Self {
                registry: ContextSourceRegistry::default(),
                panes: HashMap::new(),
                active_panes: HashMap::new(),
                owners: KittyWindowOwners::default(),
                stipen: tempdir().unwrap(),
                lyn: tempdir().unwrap(),
            }
        }

        /// Window 81 belongs to Kitty 100, window 82 to Kitty 200.
        fn heartbeat(&mut self, process_id: u32, active_window: u32, at: Instant) {
            let parent = |process_id: u32| match process_id {
                1001 => Some(100),
                2001 => Some(200),
                _ => None,
            };
            let owner = |window: u32| match window {
                81 => Some(100),
                82 => Some(200),
                _ => None,
            };
            let raw = KittyObservationMessage {
                version: 1,
                terminal_session_id: 1,
                process_id,
                state: KittyState::Focused,
            };
            let message = KittyObservationMessage {
                terminal_session_id: kitty_pane_key(&self.panes, &raw, parent),
                ..raw
            };
            let window = kitty_heartbeat_window(
                Some((active_window, ContextWindowKind::Kitty)),
                &self.panes,
                &message,
                parent,
                &mut self.owners,
                owner,
                at,
            );
            let directory = if process_id == 1001 {
                self.stipen.path().to_path_buf()
            } else {
                self.lyn.path().to_path_buf()
            };
            apply_kitty_message(
                &mut self.registry,
                &mut self.panes,
                &mut self.active_panes,
                message,
                window,
                Some(directory),
                at,
            );
        }

        fn source_for_window(&mut self, window: u64, at: Instant) -> Option<(String, String)> {
            self.registry
                .live_sources(at)
                .into_iter()
                .find(|source| source.window() == Some(WindowCorrelationToken::from_native(window)))
                .map(|source| (source.source_id().to_string(), source.label().to_owned()))
        }
    }

    #[test]
    fn a_second_kitty_process_cannot_steal_the_remembered_window() {
        // Popup open: window 81 (stipen) stays remembered while both watchers
        // keep heartbeating their last pane every two seconds.
        let now = Instant::now();
        let mut kittys = TwoKittys::new();
        kittys.heartbeat(1001, 81, now);
        let invoked = kittys.source_for_window(81, now).unwrap();
        for second in (2..=60).step_by(2) {
            let at = now + Duration::from_secs(second);
            kittys.heartbeat(2001, 81, at);
            kittys.heartbeat(1001, 81, at);
            assert_eq!(
                kittys.source_for_window(81, at),
                Some(invoked.clone()),
                "the selected stipen source changed at {second}s"
            );
        }
        let stipen = kittys
            .stipen
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        assert_eq!(invoked.1, stipen);
    }

    #[test]
    fn clicking_the_other_kitty_window_keeps_the_invoked_pane_alive() {
        let now = Instant::now();
        let mut kittys = TwoKittys::new();
        kittys.heartbeat(1001, 81, now);
        let invoked = kittys.source_for_window(81, now).unwrap();
        // The user clicks the lyn Kitty window (82) with the draft still open.
        kittys.heartbeat(2001, 82, now + Duration::from_secs(1));
        for second in (2..=60).step_by(2) {
            let at = now + Duration::from_secs(second);
            kittys.heartbeat(1001, 82, at);
            kittys.heartbeat(2001, 82, at);
            assert_eq!(kittys.source_for_window(81, at), Some(invoked.clone()));
        }
        let at = now + Duration::from_secs(60);
        let other = kittys.source_for_window(82, at).unwrap();
        let lyn = kittys
            .lyn
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        assert_eq!(other.1, lyn);
        assert_ne!(other.0, invoked.0);
    }

    #[test]
    fn kitty_pane_keys_separate_processes_and_find_ended_bindings() {
        let parent = |process_id: u32| Some(process_id / 10);
        let focused = |process_id| KittyObservationMessage {
            version: 1,
            terminal_session_id: 1,
            process_id,
            state: KittyState::Focused,
        };
        let panes = HashMap::new();
        let first = kitty_pane_key(&panes, &focused(1001), parent);
        let second = kitty_pane_key(&panes, &focused(2001), parent);
        assert_ne!(first, second);
        let panes = HashMap::from([(
            first,
            KittyPaneBinding {
                process_id: 1001,
                window_id: 81,
            },
        )]);
        let ended = KittyObservationMessage {
            state: KittyState::Ended,
            ..focused(1001)
        };
        // An exited pane's parent is unreadable; its binding is still found.
        assert_eq!(kitty_pane_key(&panes, &ended, |_| None), first);
    }

    #[test]
    fn kitty_window_attestation_requires_the_owning_process() {
        let now = Instant::now();
        let kitty = Some((81, ContextWindowKind::Kitty));
        let parent = |process_id: u32| Some(process_id / 10);
        let mut owners = KittyWindowOwners::default();
        assert_eq!(
            attest_kitty_window(kitty, 1000, parent, &mut owners, |_| Some(100), now),
            kitty
        );
        assert_eq!(
            attest_kitty_window(kitty, 2000, parent, &mut owners, |_| Some(100), now),
            None
        );
        assert_eq!(
            attest_kitty_window(kitty, 1000, |_| None, &mut owners, |_| Some(100), now),
            None,
            "an unreadable parent cannot bind"
        );
        // Without XRes the single-process behaviour is unchanged.
        let mut unknown = KittyWindowOwners::default();
        assert_eq!(
            attest_kitty_window(kitty, 2000, parent, &mut unknown, |_| None, now),
            kitty
        );
        let editor = Some((82, ContextWindowKind::Vscode));
        assert_eq!(
            attest_kitty_window(editor, 2000, parent, &mut owners, |_| Some(1), now),
            editor
        );
        assert_eq!(
            attest_kitty_window(None, 1000, parent, &mut owners, |_| Some(100), now),
            None
        );
    }

    #[test]
    fn kitty_window_owners_are_cached_then_requeried_after_the_ttl() {
        let now = Instant::now();
        let mut owners = KittyWindowOwners::default();
        let mut lookups = 0;
        let mut owner_at = |at, value| {
            owners.owner(81, at, |_| {
                lookups += 1;
                Some(value)
            })
        };
        assert_eq!(owner_at(now, 100), Some(100));
        assert_eq!(owner_at(now + Duration::from_secs(29), 200), Some(100));
        assert_eq!(owner_at(now + KITTY_OWNER_TTL, 200), Some(200));
        assert_eq!(lookups, 2);
    }

    #[test]
    fn parent_process_reads_the_real_parent_of_a_child() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        assert_eq!(parent_process(child.id()), Some(std::process::id()));
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(parent_process(child.id()), None);
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

    #[test]
    fn kitty_invoke_is_not_parsed_as_a_generic_shell_session() {
        let encoded = serde_json::json!({
            "version": 2,
            "kind": "invoke",
            "requestId": "c3b1a2d0-1111-4aaa-8bbb-0123456789ab",
            "terminalSessionId": 77,
            "processId": 4242
        });
        let parsed: TerminalObservationMessage = serde_json::from_value(encoded).unwrap();
        match parsed {
            TerminalObservationMessage::KittyInvoke(message) => {
                assert_eq!(message.kind, "invoke");
                assert_eq!(message.terminal_session_id, 77);
                assert_eq!(message.process_id, 4242);
            }
            TerminalObservationMessage::Shell(_) | TerminalObservationMessage::Kitty(_) => {
                panic!("expected kitty invoke")
            }
        }
    }

    #[test]
    fn gnome_terminal_live_observation_parses_as_a_shell_session() {
        let encoded = serde_json::json!({
            "version": 1,
            "sessionId": "c3b1a2d0-2222-4aaa-8bbb-0123456789ab",
            "processId": 1742125,
            "windowId": 76630076,
            "state": "live"
        });
        let parsed: TerminalObservationMessage = serde_json::from_value(encoded).unwrap();
        match parsed {
            TerminalObservationMessage::Shell(message) => {
                assert_eq!(message.process_id, 1742125);
                assert_eq!(message.window_id, 76630076);
                assert_eq!(message.state, ShellState::Live);
            }
            TerminalObservationMessage::KittyInvoke(_) | TerminalObservationMessage::Kitty(_) => {
                panic!("expected generic shell observation")
            }
        }
    }

    #[test]
    fn generic_shell_observation_is_ignored_when_a_kitty_pane_owns_the_window() {
        let mut panes = HashMap::new();
        panes.insert(81, 501);
        assert!(shell_observation_superseded_by_kitty(
            ContextWindowKind::Kitty,
            81,
            &panes,
        ));
        assert!(!shell_observation_superseded_by_kitty(
            ContextWindowKind::GnomeTerminal,
            81,
            &panes,
        ));
        assert!(!shell_observation_superseded_by_kitty(
            ContextWindowKind::Kitty,
            82,
            &panes,
        ));
    }

    #[test]
    fn one_shell_session_cannot_rebind_to_another_process_or_window() {
        let directory = tempdir().unwrap();
        let now = Instant::now();
        let mut registry = ContextSourceRegistry::default();
        let mut sessions = HashMap::new();
        let mut message = live_message(1201, 91);
        let session_id = message.session_id;

        assert!(apply_message(
            &mut registry,
            &mut sessions,
            message,
            Some((91, ContextWindowKind::GnomeTerminal)),
            Some(directory.path().to_path_buf()),
            now,
        ));
        message = ShellObservationMessage {
            version: 1,
            session_id,
            process_id: 1202,
            window_id: 92,
            state: ShellState::Live,
        };
        assert!(!apply_message(
            &mut registry,
            &mut sessions,
            message,
            Some((92, ContextWindowKind::GnomeTerminal)),
            Some(directory.path().to_path_buf()),
            now,
        ));
        assert_eq!(registry.live_sources(now).len(), 1);
        assert_eq!(
            registry.live_sources(now)[0].window(),
            Some(WindowCorrelationToken::from_native(91))
        );
    }

    #[test]
    fn process_directory_requires_the_runtime_user_and_returns_no_client_path() {
        let fixture = tempdir().unwrap();
        let project = fixture.path().join("project");
        let process = fixture.path().join("proc").join("1301");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&process).unwrap();
        symlink(&project, process.join("cwd")).unwrap();
        let uid = fs::metadata(&process).unwrap().uid();

        assert_eq!(
            process_directory_at(&fixture.path().join("proc"), 1301, uid).unwrap(),
            project
        );
        assert_eq!(
            process_directory_at(&fixture.path().join("proc"), 1301, uid.saturating_add(1))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }
}
