//! Private harness entrypoint. Never compiled into the application or its IPC.

use super::*;
use crate::context::{
    gnome_protocol::{Outcome, ReplyReject},
    unix_broker::peer_credentials,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
};

const MAX_CONTROL_BYTES: usize = 2048;

#[test]
fn media_probe_control_rejects_unknown_recipes_and_extra_authority() {
    let valid = json!({"kind":"mediaCaptureProbe", "windowId":1,
        "expectedDirectory":"/synthetic", "mediaKind":"image", "retireGeneration":false});
    assert!(serde_json::from_value::<Control>(valid.clone()).is_ok());
    for (field, value) in [
        ("mediaKind", json!("video")),
        ("retireGeneration", json!("command")),
        ("serverPid", json!(123)),
        ("caption", json!("arbitrary content")),
    ] {
        let mut refused = valid.clone();
        refused[field] = value;
        assert!(serde_json::from_value::<Control>(refused).is_err());
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Control {
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "mediaCaptureProbe")]
    MediaCaptureProbe {
        #[serde(rename = "windowId")]
        window_id: u32,
        #[serde(rename = "expectedDirectory")]
        expected_directory: std::path::PathBuf,
        #[serde(rename = "mediaKind")]
        media_kind: crate::contract::MediaKind,
        #[serde(rename = "retireGeneration")]
        retire_generation: bool,
    },
    #[serde(rename = "captureProbe")]
    CaptureProbe {
        #[serde(rename = "windowId")]
        window_id: u32,
        #[serde(rename = "expectedDirectory")]
        expected_directory: std::path::PathBuf,
    },
    #[serde(rename = "processQuery")]
    ProcessQuery {
        #[serde(rename = "windowId")]
        window_id: u32,
        #[serde(rename = "expectedDirectory")]
        expected_directory: std::path::PathBuf,
    },
    #[serde(rename = "ptyQuery")]
    PtyQuery {
        #[serde(rename = "windowId")]
        window_id: u32,
        #[serde(rename = "invokedUs")]
        invoked_us: u64,
    },
    #[serde(rename = "windowQuery")]
    WindowQuery {
        #[serde(rename = "windowId")]
        window_id: u32,
        #[serde(rename = "invokedUs")]
        invoked_us: u64,
    },
    #[serde(rename = "query")]
    Query {
        #[serde(rename = "serverPid")]
        server_pid: u32,
        #[serde(rename = "windowId")]
        window_id: u32,
        #[serde(rename = "invokedUs")]
        invoked_us: u64,
    },
}

fn reply(broker: &Arc<NativeBroker>, control: Control) -> Value {
    let result = match control {
        Control::MediaCaptureProbe {
            window_id,
            expected_directory,
            media_kind,
            retire_generation,
        } => {
            return crate::commands::capture::native_media_probe::run(
                broker.clone(),
                window_id,
                expected_directory,
                media_kind,
                retire_generation,
            );
        }
        Control::CaptureProbe {
            window_id,
            expected_directory,
        } => {
            return capture_probe(broker.clone(), window_id, expected_directory);
        }
        Control::ProcessQuery {
            window_id,
            expected_directory,
        } => {
            let invocation = super::super::gnome_process::ProcessInvocation::now().unwrap();
            return match broker.query_window_process(window_id, invocation) {
                Ok(evidence) => {
                    json!({"status": "candidate", "sourceId": evidence.native.candidate.source_id,
                    "stillCurrent": evidence.revalidate(broker).is_ok(),
                    "directoryMatches": std::fs::canonicalize(expected_directory).is_ok_and(|path| path == evidence.directory())})
                }
                Err(_) => json!({"status": "unavailable"}),
            };
        }
        Control::PtyQuery {
            window_id,
            invoked_us,
        } => broker.query_window_pty(window_id, invoked_us),
        Control::Status => {
            let mut peers: Vec<_> = broker
                .shared
                .channels
                .lock()
                .unwrap()
                .keys()
                .copied()
                .collect();
            peers.sort_unstable();
            return json!({"peers": peers});
        }
        Control::Query {
            server_pid,
            window_id,
            invoked_us,
        } => broker.query(server_pid, window_id, invoked_us),
        Control::WindowQuery {
            window_id,
            invoked_us,
        } => broker.query_window(window_id, invoked_us),
    };
    match result {
        Ok(evidence) => json!({
            "status": "candidate", "serverPid": evidence.server_pid,
            "generation": evidence.generation, "sourceId": evidence.candidate.source_id,
            "ptyNumber": evidence.candidate.pty_number,
            "foregroundPgid": evidence.candidate.foreground_pgid,
            "registeredAtUs": evidence.candidate.registered_us,
            "changedAtUs": evidence.candidate.changed_us,
            "revision": evidence.candidate.revision,
        }),
        Err(error) => {
            eprintln!(
                "{}",
                json!({"check": "native_probe_query_refused", "error": format!("{error:?}")})
            );
            let status = match error {
                BrokerError::Unavailable => "unavailable",
                BrokerError::Busy => "busy",
                BrokerError::Channel(ChannelError::Disconnected) => "disconnected",
                BrokerError::Channel(ChannelError::Timeout) => "timeout",
                BrokerError::Channel(ChannelError::Reply(ReplyReject::Unresolved(outcome))) => {
                    match outcome {
                        Outcome::Missing => "missing",
                        Outcome::Changed => "changed",
                        Outcome::Ambiguous => "ambiguous",
                        Outcome::Unavailable => "unavailable",
                        Outcome::Candidate(_) => panic!("candidate cannot be unresolved"),
                    }
                }
                _ => "rejected",
            };
            json!({"status": status})
        }
    }
}

fn capture_probe(broker: Arc<NativeBroker>, window: u32, directory: std::path::PathBuf) -> Value {
    use crate::{
        capture::session::CaptureSessionService,
        context::{
            gnome_capture, gnome_process::ProcessInvocation,
            session_registry::ContextSourceRegistry,
        },
        error::{CommandResult, ErrorCode},
        media::staging::MediaStore,
        storage::{Database, contexts::ContextRepository},
    };
    let invocation = ProcessInvocation::now().unwrap();
    let database = Mutex::new(Database::open_in_memory().unwrap());
    let service = Mutex::new(CaptureSessionService::default());
    let registry = Mutex::new(ContextSourceRegistry::default());
    let session = service.lock().unwrap().get_or_prepare();
    let ticket = service
        .lock()
        .unwrap()
        .begin_automatic_query(session.session_id)
        .unwrap();
    let Some((resolution, source)) = gnome_capture::prepare(broker, window, invocation, &database)
    else {
        return json!({"status": "unavailable"});
    };
    service
        .lock()
        .unwrap()
        .finish_automatic_query(ticket, resolution)
        .unwrap()
        .unwrap();
    let source_id = source.source_id();
    registry
        .lock()
        .unwrap()
        .register_native(session.session_id, source);
    registry
        .lock()
        .unwrap()
        .release_native(crate::contract::CaptureSessionId::new());
    assert!(
        registry
            .lock()
            .unwrap()
            .get(source_id, std::time::Instant::now())
            .is_some()
    );
    // A late save is a fresh continuity check, not a reset of the original
    // reply deadline. Unchanged native identity remains usable after 120 ms.
    std::thread::sleep(std::time::Duration::from_millis(150));
    let body = "synthetic native capture acceptance";
    let input = json!({"sessionId":session.session_id, "textBody":body});
    let saved = match crate::commands::capture::save_text_capture_with_registry_value(
        input.clone(),
        &database,
        &service,
        &registry,
    ) {
        CommandResult::Success { data, .. } => data,
        CommandResult::Failure { error, .. } => {
            return json!({"status": "saveRefused", "code": error.code});
        }
    };
    let duplicate_refused = matches!(crate::commands::capture::save_text_capture_with_registry_value(
        input, &database, &service, &registry), CommandResult::Failure { error, .. } if error.code == ErrorCode::StaleSession);
    let media_root = tempfile::tempdir().unwrap();
    let media = Mutex::new(MediaStore::open(media_root.path().to_path_buf()).unwrap());
    let CommandResult::Success { data: detail, .. } = crate::commands::library::get_capture_value(
        json!({"captureId":saved.capture_id}),
        &database,
        &media,
    ) else {
        panic!("Library cannot read native capture");
    };
    let expected =
        crate::context::inspect_project_directory(&std::fs::canonicalize(directory).unwrap())
            .unwrap();
    let db = database.lock().unwrap();
    let project = ContextRepository::new(db.connection())
        .find_project(expected.project_key.as_deref(), &expected.project_path)
        .unwrap();
    let count: i64 = db
        .connection()
        .query_row("SELECT COUNT(*) FROM captures", [], |row| row.get(0))
        .unwrap();
    json!({"status":"saved", "bodyMatches":detail.text_body.as_deref() == Some(body),
        "projectMatches":project.is_some_and(|context| context.id == detail.context.id),
        "branchMatches":detail.branch_name == expected.branch_name,
        "duplicateRefused":duplicate_refused, "captureCount":count,
        "leaseReleased":registry.lock().unwrap().live_sources(std::time::Instant::now()).is_empty()})
}

#[test]
fn real_gnome_probe_entry() {
    run_real_gnome_probe(false);
}

#[test]
fn real_published_gnome_probe_entry() {
    run_real_gnome_probe(true);
}

fn run_real_gnome_probe(published: bool) {
    // A normal unit run does not claim runtime GNOME coverage. The dedicated
    // private-Xvfb runner invokes this entry with an isolated runtime directory.
    let Some(runtime) = std::env::var_os("LYN_GNOME_PROBE_RUNTIME") else {
        return;
    };
    let runtime = std::path::PathBuf::from(runtime);
    let broker = Arc::new(if published {
        NativeBroker::start_published(&runtime).unwrap()
    } else {
        NativeBroker::start(&runtime).unwrap()
    });
    let path = runtime.join("probe.sock");
    let listener = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let (mut stream, _) = listener.accept().unwrap();
    assert_eq!(peer_credentials(&stream).unwrap().uid, unsafe {
        libc::geteuid()
    });
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(3)))
        .unwrap();
    loop {
        let mut length = [0; 4];
        match stream.read_exact(&mut length) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => panic!("probe control failed: {error}"),
        }
        let length = u32::from_be_bytes(length) as usize;
        assert!((1..=MAX_CONTROL_BYTES).contains(&length));
        let mut bytes = [0; MAX_CONTROL_BYTES];
        stream.read_exact(&mut bytes[..length]).unwrap();
        let request: Control = serde_json::from_slice(&bytes[..length]).unwrap();
        let response = serde_json::to_vec(&reply(&broker, request)).unwrap();
        assert!(response.len() <= MAX_CONTROL_BYTES);
        stream
            .write_all(&(response.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(&response).unwrap();
    }
    drop(stream);
    drop(listener);
    broker.shutdown();
    drop(broker);
    assert!(!runtime.join("lyn-gnome-v1.sock").exists());
    assert!(!runtime.join("lyn-gnome-v1.current").exists());
    fs::remove_file(path).unwrap();
}
