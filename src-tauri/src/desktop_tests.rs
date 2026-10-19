//! Real desktop fixture only. No production command, mode or auth exception.

use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt},
        net::UnixStream,
    },
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::Duration,
};

static QUERY_GATE: OnceLock<Mutex<UnixStream>> = OnceLock::new();

/// Test-only scheduling barrier. It cannot choose a source or alter a deadline.
pub(crate) fn before_native_query(window: u32, invoked_us: u64) {
    let Some(gate) = QUERY_GATE.get() else { return };
    let mut stream = gate.lock().unwrap();
    let mut message = [0u8; 13];
    message[0] = b'Q';
    message[1..5].copy_from_slice(&window.to_be_bytes());
    message[5..13].copy_from_slice(&invoked_us.to_be_bytes());
    stream.write_all(&message).unwrap();
    let mut release = [0];
    stream.read_exact(&mut release).unwrap();
    assert_eq!(release, [b'R'], "Invalid fixture query release");
}

pub(crate) fn native_query_finished(error: Option<&crate::context::gnome_broker::BrokerError>) {
    use crate::context::{
        gnome_broker::BrokerError,
        gnome_channel::ChannelError,
        gnome_protocol::{Outcome, ReplyReject},
    };
    let Some(gate) = QUERY_GATE.get() else { return };
    let status = match error {
        None => 0,
        Some(BrokerError::Channel(ChannelError::Reply(
            ReplyReject::Changed | ReplyReject::Unresolved(Outcome::Changed),
        ))) => 1,
        Some(BrokerError::Channel(ChannelError::Timeout)) => 2,
        _ => 3,
    };
    gate.lock().unwrap().write_all(&[b'O', status]).unwrap();
}

fn private_xdg_root(paths: &[PathBuf; 4]) -> Option<PathBuf> {
    let root = paths[0].parent()?;
    if !root.file_name()?.to_str()?.starts_with("lyn-gnome-run-") {
        return None;
    }
    for path in paths
        .iter()
        .map(PathBuf::as_path)
        .chain(std::iter::once(root))
    {
        let metadata = fs::symlink_metadata(path).ok()?;
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
        {
            return None;
        }
    }
    paths
        .iter()
        .all(|path| path.is_absolute() && path.parent() == Some(root))
        .then(|| root.to_path_buf())
}

fn parent_control_stream(runtime: &std::path::Path, name: &str) -> UnixStream {
    let path = runtime.join(name);
    let metadata = fs::symlink_metadata(&path).unwrap();
    assert!(metadata.file_type().is_socket());
    assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
    assert_eq!(metadata.mode() & 0o777, 0o600);
    let stream = UnixStream::connect(path).unwrap();
    let peer = crate::context::unix_broker::peer_credentials(&stream).unwrap();
    assert_eq!(peer.uid, unsafe { libc::geteuid() });
    assert_eq!(peer.pid, unsafe { libc::getppid() });
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    stream
}

#[test]
fn desktop_fixture_requires_one_private_owned_xdg_root() {
    let directory = tempfile::Builder::new()
        .prefix("lyn-gnome-run-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let paths = ["runtime", "config", "data", "cache"].map(|name| directory.path().join(name));
    for path in &paths {
        fs::DirBuilder::new().mode(0o700).create(path).unwrap();
    }
    assert_eq!(
        private_xdg_root(&paths),
        Some(directory.path().to_path_buf())
    );
    let mut mixed = paths.clone();
    mixed[1] = PathBuf::from("/tmp");
    assert!(private_xdg_root(&mixed).is_none());
    fs::set_permissions(&paths[1], fs::Permissions::from_mode(0o755)).unwrap();
    assert!(private_xdg_root(&paths).is_none());
    fs::set_permissions(&paths[1], fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(private_xdg_root(&paths).is_none());
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    fs::remove_dir(&paths[1]).unwrap();
    std::os::unix::fs::symlink(&paths[2], &paths[1]).unwrap();
    assert!(private_xdg_root(&paths).is_none());
}

#[test]
fn real_gnome_desktop_entry() {
    if std::env::var_os("LYN_GNOME_UI_FIXTURE").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let paths = [
        "XDG_RUNTIME_DIR",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
    ]
    .map(|name| PathBuf::from(std::env::var_os(name).expect("private XDG required")));
    let root = private_xdg_root(&paths)
        .expect("desktop fixture must run on isolated_run.py's private XDG root");
    if std::env::var_os("LYN_GNOME_UI_QUERY_GATE").as_deref() == Some(std::ffi::OsStr::new("1")) {
        let stream = parent_control_stream(&paths[0], "native-query-gate.sock");
        assert!(QUERY_GATE.set(Mutex::new(stream)).is_ok());
    }
    let helper_root = tempfile::Builder::new()
        .prefix("fixture-helper-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(root)
        .unwrap();
    let port: u16 = std::env::var("LYN_GNOME_UI_PORT").unwrap().parse().unwrap();
    assert_ne!(port, 0);
    let mut context = tauri::generate_context!();
    // Tauri 2.11.5 Context::config_mut / Builder::any_thread, checked against
    // the pinned crate's source; this localhost URL exists only in cfg(test).
    context.config_mut().build.dev_url = Some(format!("http://127.0.0.1:{port}").parse().unwrap());
    let app = super::desktop_builder(Some(helper_root.path().to_path_buf()))
        .any_thread()
        .build(context)
        .unwrap();
    if std::env::var_os("LYN_GNOME_UI_RESTART").as_deref() == Some(std::ffi::OsStr::new("1")) {
        let mut stream = parent_control_stream(&paths[0], "desktop-lifecycle.sock");
        let handle = app.handle().clone();
        std::thread::spawn(move || {
            // The enclosing fixture owns a 60-second lifetime. No commands,
            // source IDs or filesystem paths can enter through this socket.
            stream
                .set_read_timeout(Some(Duration::from_secs(45)))
                .unwrap();
            // At most one controlled reconnect followed by one clean exit.
            let mut reconnected = false;
            for _ in 0..2 {
                let mut request = [0];
                if stream.read_exact(&mut request).is_err() {
                    break;
                }
                match request[0] {
                    b'X' => {
                        handle.exit(0);
                        break;
                    }
                    b'R' if !reconnected => {
                        reconnected = true;
                        use tauri::Manager;
                        let broker = handle
                            .state::<std::sync::Arc<crate::context::gnome_broker::NativeBroker>>();
                        let (old, new) = broker
                            .reconnect_fixture_peer()
                            .expect("real native reconnect required");
                        stream.write_all(b"G").unwrap();
                        stream.write_all(old.as_bytes()).unwrap();
                        stream.write_all(new.as_bytes()).unwrap();
                    }
                    _ => break,
                }
            }
        });
    }
    app.run(super::desktop_runtime_event);
}
