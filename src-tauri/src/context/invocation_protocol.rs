//! Versioned editor-provider frames on the private Unix socket.
//!
//! v1 heartbeats remain observe-only. v2 `kind: invoke` is the capture request
//! contract: Rust assigns window/process identity at receive time, and the
//! client `requestId` is the invoke generation. A v1 payload with
//! `intent: "invoke"` is a compatibility shim; it MUST NOT receive v2
//! generation guarantees.
//!
//! Workspace folders are correlation hints for local directories. They never
//! become UI filesystem paths. An integrated-terminal invoke may send that
//! terminal's live working directory as the hint when `surface` is `terminal`;
//! the `cwd` field remains forbidden. External/shell cwd is still derived by
//! Rust from a validated process. Client-supplied native window or process
//! identifiers are rejected.

use std::time::Duration;

use serde_json::{Map, Value};
use uuid::Uuid;

pub(crate) const MAX_PROVIDER_MESSAGE_BYTES: u64 = 16 * 1024;
pub(crate) const MAX_WORKSPACE_PATH_BYTES: usize = 4 * 1024;
pub(crate) const INVOKE_REPLY_BUDGET: Duration = Duration::from_millis(300);

const FORBIDDEN_NATIVE_KEYS: &[&str] = &[
    "windowId",
    "window",
    "processId",
    "pid",
    "cwd",
    "hwnd",
    "wid",
];
const V1_ALLOWED_KEYS: &[&str] = &[
    "version",
    "instanceId",
    "state",
    "workspaceFolders",
    "intent",
];
const V2_ALLOWED_KEYS: &[&str] = &[
    "version",
    "kind",
    "instanceId",
    "requestId",
    "workspaceFolders",
    "surface",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProtocolVersion {
    V1,
    V2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObserveState {
    Focused,
    Unfocused,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderCapability {
    ObserveFocus,
    InvokeCapture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InvokeSurface {
    Editor,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProviderFrame {
    Observe {
        protocol: ProtocolVersion,
        instance_id: Uuid,
        state: ObserveState,
        workspace_folders: Vec<String>,
    },
    Invoke {
        protocol: ProtocolVersion,
        instance_id: Uuid,
        request_id: Option<Uuid>,
        workspace_folders: Vec<String>,
        surface: InvokeSurface,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameReject {
    TooLarge,
    InvalidJson,
    UnknownVersion,
    UnknownKind,
    UnknownField,
    ClientSuppliedNativeId,
    EmptyPath,
    PathTooLong,
}

impl ProviderFrame {
    pub(crate) fn capabilities(&self) -> &'static [ProviderCapability] {
        match self {
            Self::Observe { .. } => &[ProviderCapability::ObserveFocus],
            Self::Invoke {
                protocol: ProtocolVersion::V2,
                request_id: Some(_),
                ..
            } => &[
                ProviderCapability::ObserveFocus,
                ProviderCapability::InvokeCapture,
            ],
            Self::Invoke { .. } => &[ProviderCapability::InvokeCapture],
        }
    }

    pub(crate) fn has_v2_invoke_guarantees(&self) -> bool {
        matches!(
            self,
            Self::Invoke {
                protocol: ProtocolVersion::V2,
                request_id: Some(_),
                ..
            }
        )
    }
}

/// A late frame may complete an in-flight invoke only for the same v2
/// generation. v1 observe/compat invoke, other request ids, and expired
/// captures must not finish another invocation's resolution.
pub(crate) fn late_frame_may_complete_invoke(
    frame: &ProviderFrame,
    expected_request_id: Uuid,
) -> bool {
    match frame {
        ProviderFrame::Invoke {
            protocol: ProtocolVersion::V2,
            request_id: Some(request_id),
            ..
        } => *request_id == expected_request_id,
        _ => false,
    }
}

pub(crate) fn parse_provider_frame(bytes: &[u8]) -> Result<ProviderFrame, FrameReject> {
    if bytes.len() as u64 > MAX_PROVIDER_MESSAGE_BYTES {
        return Err(FrameReject::TooLarge);
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| FrameReject::InvalidJson)?;
    let object = value.as_object().ok_or(FrameReject::InvalidJson)?;
    match version_of(object)? {
        ProtocolVersion::V1 => parse_v1(object),
        ProtocolVersion::V2 => parse_v2(object),
    }
}

fn parse_v1(object: &Map<String, Value>) -> Result<ProviderFrame, FrameReject> {
    reject_unknown_or_native(object, V1_ALLOWED_KEYS)?;
    let instance_id = uuid_field(object, "instanceId")?;
    let workspace_folders = workspace_folders(object)?;
    match intent_of(object)? {
        V1Intent::Observe => Ok(ProviderFrame::Observe {
            protocol: ProtocolVersion::V1,
            instance_id,
            state: observe_state(object)?,
            workspace_folders,
        }),
        V1Intent::Invoke => Ok(ProviderFrame::Invoke {
            protocol: ProtocolVersion::V1,
            instance_id,
            request_id: None,
            workspace_folders,
            surface: InvokeSurface::Editor,
        }),
    }
}

fn parse_v2(object: &Map<String, Value>) -> Result<ProviderFrame, FrameReject> {
    reject_unknown_or_native(object, V2_ALLOWED_KEYS)?;
    match object.get("kind").and_then(Value::as_str) {
        Some("invoke") => Ok(ProviderFrame::Invoke {
            protocol: ProtocolVersion::V2,
            instance_id: uuid_field(object, "instanceId")?,
            request_id: Some(uuid_field(object, "requestId")?),
            workspace_folders: workspace_folders(object)?,
            surface: invoke_surface(object)?,
        }),
        Some(_) => Err(FrameReject::UnknownKind),
        None => Err(FrameReject::InvalidJson),
    }
}

fn reject_unknown_or_native(
    object: &Map<String, Value>,
    allowed: &[&str],
) -> Result<(), FrameReject> {
    for key in object.keys() {
        if FORBIDDEN_NATIVE_KEYS.contains(&key.as_str()) {
            return Err(FrameReject::ClientSuppliedNativeId);
        }
        if !allowed.contains(&key.as_str()) {
            return Err(FrameReject::UnknownField);
        }
    }
    Ok(())
}

fn version_of(object: &Map<String, Value>) -> Result<ProtocolVersion, FrameReject> {
    match object.get("version").and_then(Value::as_u64) {
        Some(1) => Ok(ProtocolVersion::V1),
        Some(2) => Ok(ProtocolVersion::V2),
        Some(_) => Err(FrameReject::UnknownVersion),
        None => Err(FrameReject::InvalidJson),
    }
}

fn uuid_field(object: &Map<String, Value>, name: &str) -> Result<Uuid, FrameReject> {
    object
        .get(name)
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or(FrameReject::InvalidJson)
}

fn workspace_folders(object: &Map<String, Value>) -> Result<Vec<String>, FrameReject> {
    let Some(Value::Array(items)) = object.get("workspaceFolders") else {
        return Err(FrameReject::InvalidJson);
    };
    let mut folders = Vec::with_capacity(items.len());
    for item in items {
        let Some(path) = item.as_str() else {
            return Err(FrameReject::InvalidJson);
        };
        if path.is_empty() {
            return Err(FrameReject::EmptyPath);
        }
        if path.len() > MAX_WORKSPACE_PATH_BYTES {
            return Err(FrameReject::PathTooLong);
        }
        folders.push(path.to_owned());
    }
    Ok(folders)
}

fn observe_state(object: &Map<String, Value>) -> Result<ObserveState, FrameReject> {
    match object.get("state").and_then(Value::as_str) {
        Some("focused") => Ok(ObserveState::Focused),
        Some("unfocused") => Ok(ObserveState::Unfocused),
        Some("ended") => Ok(ObserveState::Ended),
        _ => Err(FrameReject::InvalidJson),
    }
}

fn invoke_surface(object: &Map<String, Value>) -> Result<InvokeSurface, FrameReject> {
    match object.get("surface") {
        None => Ok(InvokeSurface::Editor),
        Some(Value::String(value)) if value == "editor" => Ok(InvokeSurface::Editor),
        Some(Value::String(value)) if value == "terminal" => Ok(InvokeSurface::Terminal),
        Some(_) => Err(FrameReject::InvalidJson),
    }
}

#[derive(Clone, Copy)]
enum V1Intent {
    Observe,
    Invoke,
}

fn intent_of(object: &Map<String, Value>) -> Result<V1Intent, FrameReject> {
    match object.get("intent") {
        None => Ok(V1Intent::Observe),
        Some(Value::String(value)) if value == "observe" => Ok(V1Intent::Observe),
        Some(Value::String(value)) if value == "invoke" => Ok(V1Intent::Invoke),
        Some(_) => Err(FrameReject::InvalidJson),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSTANCE: &str = "7af0a690-8948-4f0a-b9f0-51e43c583efa";
    const REQUEST: &str = "c3b1a2d0-1111-4aaa-8bbb-0123456789ab";

    fn parse(json: &str) -> Result<ProviderFrame, FrameReject> {
        parse_provider_frame(json.as_bytes())
    }

    #[test]
    fn v1_observe_round_trips_without_becoming_invoke() {
        let frame = parse(&format!(
            r#"{{"version":1,"instanceId":"{INSTANCE}","state":"focused","workspaceFolders":["/tmp/lyn-cl01-alpha"]}}"#
        ))
        .unwrap();
        assert_eq!(
            frame,
            ProviderFrame::Observe {
                protocol: ProtocolVersion::V1,
                instance_id: Uuid::parse_str(INSTANCE).unwrap(),
                state: ObserveState::Focused,
                workspace_folders: vec!["/tmp/lyn-cl01-alpha".into()],
            }
        );
        assert_eq!(frame.capabilities(), &[ProviderCapability::ObserveFocus]);
        assert!(!frame.has_v2_invoke_guarantees());
    }

    #[test]
    fn v1_intent_invoke_is_compatibility_without_generation() {
        let frame = parse(&format!(
            r#"{{"version":1,"instanceId":"{INSTANCE}","state":"focused","workspaceFolders":["/tmp/lyn-cl01-alpha"],"intent":"invoke"}}"#
        ))
        .unwrap();
        assert_eq!(
            frame,
            ProviderFrame::Invoke {
                protocol: ProtocolVersion::V1,
                instance_id: Uuid::parse_str(INSTANCE).unwrap(),
                request_id: None,
                workspace_folders: vec!["/tmp/lyn-cl01-alpha".into()],
                surface: InvokeSurface::Editor,
            }
        );
        assert!(!frame.has_v2_invoke_guarantees());
        assert!(!late_frame_may_complete_invoke(
            &frame,
            Uuid::parse_str(REQUEST).unwrap()
        ));
    }

    #[test]
    fn v2_invoke_carries_rust_owned_generation() {
        let frame = parse(&format!(
            r#"{{"version":2,"kind":"invoke","instanceId":"{INSTANCE}","requestId":"{REQUEST}","workspaceFolders":["/tmp/lyn-cl01-beta"]}}"#
        ))
        .unwrap();
        assert_eq!(
            frame,
            ProviderFrame::Invoke {
                protocol: ProtocolVersion::V2,
                instance_id: Uuid::parse_str(INSTANCE).unwrap(),
                request_id: Some(Uuid::parse_str(REQUEST).unwrap()),
                workspace_folders: vec!["/tmp/lyn-cl01-beta".into()],
                surface: InvokeSurface::Editor,
            }
        );
        assert!(frame.has_v2_invoke_guarantees());
        assert!(late_frame_may_complete_invoke(
            &frame,
            Uuid::parse_str(REQUEST).unwrap()
        ));
        assert!(!late_frame_may_complete_invoke(&frame, Uuid::new_v4()));
    }

    #[test]
    fn v1_observe_cannot_complete_a_v2_invoke() {
        let observe = parse(&format!(
            r#"{{"version":1,"instanceId":"{INSTANCE}","state":"focused","workspaceFolders":["/tmp/lyn-cl01-alpha"]}}"#
        ))
        .unwrap();
        assert!(!late_frame_may_complete_invoke(
            &observe,
            Uuid::parse_str(REQUEST).unwrap()
        ));
    }

    #[test]
    fn unknown_version_and_kind_are_rejected() {
        assert_eq!(
            parse(&format!(
                r#"{{"version":3,"instanceId":"{INSTANCE}","state":"focused","workspaceFolders":[]}}"#
            )),
            Err(FrameReject::UnknownVersion)
        );
        assert_eq!(
            parse(&format!(
                r#"{{"version":2,"kind":"observe","instanceId":"{INSTANCE}","requestId":"{REQUEST}","workspaceFolders":[]}}"#
            )),
            Err(FrameReject::UnknownKind)
        );
    }

    #[test]
    fn client_supplied_native_ids_are_rejected() {
        assert_eq!(
            parse(&format!(
                r#"{{"version":2,"kind":"invoke","instanceId":"{INSTANCE}","requestId":"{REQUEST}","workspaceFolders":["/tmp/a"],"windowId":42}}"#
            )),
            Err(FrameReject::ClientSuppliedNativeId)
        );
        assert_eq!(
            parse(&format!(
                r#"{{"version":1,"instanceId":"{INSTANCE}","state":"focused","workspaceFolders":["/tmp/a"],"pid":1}}"#
            )),
            Err(FrameReject::ClientSuppliedNativeId)
        );
        assert_eq!(
            parse(&format!(
                r#"{{"version":1,"instanceId":"{INSTANCE}","state":"focused","workspaceFolders":["/tmp/a"],"cwd":"/tmp/a"}}"#
            )),
            Err(FrameReject::ClientSuppliedNativeId)
        );
    }

    #[test]
    fn unknown_fields_empty_and_oversized_paths_are_rejected() {
        assert_eq!(
            parse(&format!(
                r#"{{"version":1,"instanceId":"{INSTANCE}","state":"focused","workspaceFolders":[],"title":"secret"}}"#
            )),
            Err(FrameReject::UnknownField)
        );
        assert_eq!(
            parse(&format!(
                r#"{{"version":2,"kind":"invoke","instanceId":"{INSTANCE}","requestId":"{REQUEST}","workspaceFolders":[""]}}"#
            )),
            Err(FrameReject::EmptyPath)
        );
        let long_path = "a".repeat(MAX_WORKSPACE_PATH_BYTES + 1);
        let oversized = format!(
            r#"{{"version":1,"instanceId":"{INSTANCE}","state":"focused","workspaceFolders":["{long_path}"]}}"#
        );
        assert_eq!(parse(&oversized), Err(FrameReject::PathTooLong));
        assert_eq!(
            parse_provider_frame(&vec![b'{'; (MAX_PROVIDER_MESSAGE_BYTES as usize) + 1]),
            Err(FrameReject::TooLarge)
        );
    }

    #[test]
    fn multi_root_is_parsed_so_the_adapter_can_treat_it_as_ambiguous() {
        let frame = parse(&format!(
            r#"{{"version":2,"kind":"invoke","instanceId":"{INSTANCE}","requestId":"{REQUEST}","workspaceFolders":["/tmp/a","/tmp/b"]}}"#
        ))
        .unwrap();
        match frame {
            ProviderFrame::Invoke {
                workspace_folders, ..
            } => assert_eq!(workspace_folders.len(), 2),
            other => panic!("expected invoke, got {other:?}"),
        }
    }

    #[test]
    fn v2_terminal_surface_is_distinct_from_editor() {
        let frame = parse(&format!(
            r#"{{"version":2,"kind":"invoke","instanceId":"{INSTANCE}","requestId":"{REQUEST}","surface":"terminal","workspaceFolders":["/tmp/worktree"]}}"#
        ))
        .unwrap();
        match frame {
            ProviderFrame::Invoke {
                surface,
                workspace_folders,
                ..
            } => {
                assert_eq!(surface, InvokeSurface::Terminal);
                assert_eq!(workspace_folders, ["/tmp/worktree"]);
            }
            other => panic!("expected invoke, got {other:?}"),
        }
        assert_eq!(
            parse(&format!(
                r#"{{"version":2,"kind":"invoke","instanceId":"{INSTANCE}","requestId":"{REQUEST}","surface":"pane","workspaceFolders":[]}}"#
            )),
            Err(FrameReject::InvalidJson)
        );
    }

    #[test]
    fn invoke_reply_budget_matches_the_linux_reception_threshold() {
        assert_eq!(INVOKE_REPLY_BUDGET, Duration::from_millis(300));
    }
}
