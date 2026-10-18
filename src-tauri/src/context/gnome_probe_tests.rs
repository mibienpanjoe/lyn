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

#[derive(Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Control {
    #[serde(rename = "status")]
    Status,
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

fn reply(broker: &NativeBroker, control: Control) -> Value {
    match control {
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
            json!({"peers": peers})
        }
        Control::Query {
            server_pid,
            window_id,
            invoked_us,
        } => match broker.query(server_pid, window_id, invoked_us) {
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
        },
    }
}

#[test]
fn real_gnome_probe_entry() {
    // A normal unit run does not claim runtime GNOME coverage. The dedicated
    // private-Xvfb runner invokes this entry with an isolated runtime directory.
    let Some(runtime) = std::env::var_os("LYN_GNOME_PROBE_RUNTIME") else {
        return;
    };
    let runtime = std::path::PathBuf::from(runtime);
    let broker = NativeBroker::start(&runtime).unwrap();
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
    drop(broker);
    assert!(!runtime.join("lyn-gnome-v1.sock").exists());
    fs::remove_file(path).unwrap();
}
