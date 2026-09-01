//! Rust-owned shared domain values serialized across the Tauri IPC boundary.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};
use ts_rs::{Config, TS};
use uuid::Uuid;

use crate::error::{
    AppError, CommandResult, ErrorCode, ErrorDetailKey, ErrorDetailValue, ErrorDetails,
};

macro_rules! opaque_uuid {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
        #[serde(transparent)]
        #[ts(type = "string")]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

opaque_uuid!(CaptureId);
opaque_uuid!(CaptureSessionId);
opaque_uuid!(ContextId);
opaque_uuid!(ContextSourceId);
opaque_uuid!(DirectorySelectionToken);
opaque_uuid!(MediaId);
opaque_uuid!(StagedMediaId);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, TS)]
#[ts(type = "string")]
pub struct Timestamp(OffsetDateTime);

impl Timestamp {
    pub fn now_utc() -> Self {
        Self(OffsetDateTime::now_utc())
    }
}

impl Serialize for Timestamp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.format(&Rfc3339).map_err(serde::ser::Error::custom)?)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.0.format(&Rfc3339).map_err(|_| fmt::Error)?;
        formatter.write_str(&value)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let timestamp = OffsetDateTime::parse(&value, &Rfc3339).map_err(de::Error::custom)?;
        if timestamp.offset() != UtcOffset::UTC {
            return Err(de::Error::custom("timestamp must use the UTC offset"));
        }
        Ok(Self(timestamp))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CaptureKind {
    Text,
    Image,
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ContextKind {
    Project,
    Standalone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CaptionSource {
    User,
    ContextGenerated,
    TranscriptGenerated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ContextProviderKind {
    Manual,
    Vscode,
    Shell,
    ForegroundWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ContextSourceKind {
    VscodeWindow,
    IntegratedTerminal,
    ExternalTerminal,
    Shell,
    ForegroundWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Image,
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CapturePopupLayout {
    Compact,
    Error,
    Audio,
    Chooser,
    Media,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum MediaMimeType {
    #[serde(rename = "image/png")]
    ImagePng,
    #[serde(rename = "audio/wav")]
    AudioWav,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum EnrichmentStatus {
    NotRequested,
    Pending,
    Completed,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ContextRef {
    pub id: ContextId,
    pub kind: ContextKind,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SelectedProjectDirectory {
    pub selected_directory_token: DirectorySelectionToken,
    pub suggested_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PickProjectDirectoryResult {
    pub selection: Option<SelectedProjectDirectory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListContextsInput {
    pub kind: Option<ContextKind>,
    pub query: Option<String>,
    pub limit: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ListContextsResult {
    pub contexts: Vec<ContextRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CreateContextInput {
    Standalone {
        name: String,
    },
    Project {
        name: String,
        #[serde(rename = "selectedDirectoryToken")]
        selected_directory_token: DirectorySelectionToken,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreateContextResult {
    pub context: ContextRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ContextCandidate {
    pub context: ContextRef,
    pub branch_name: Option<String>,
    pub provider: ContextProviderKind,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextSelection {
    LiveSource {
        #[serde(rename = "sourceId")]
        source_id: ContextSourceId,
    },
    SavedContext {
        #[serde(rename = "contextId")]
        context_id: ContextId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ContextSourceOption {
    pub source_id: ContextSourceId,
    pub kind: ContextSourceKind,
    pub provider: ContextProviderKind,
    pub application_name: String,
    pub label: String,
    pub context: ContextRef,
    pub branch_name: Option<String>,
    pub is_foreground: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListCaptureContextSourcesInput {
    pub session_id: CaptureSessionId,
    pub query: Option<String>,
    pub limit: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ListCaptureContextSourcesResult {
    pub live_sources: Vec<ContextSourceOption>,
    pub saved_contexts: Vec<ContextRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ContextResolution {
    Resolved {
        candidate: ContextCandidate,
        selection: Option<ContextSelection>,
    },
    Ambiguous {
        candidate: (),
        selection: (),
    },
    Required {
        candidate: (),
        selection: (),
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StagedMedia {
    pub staged_media_id: StagedMediaId,
    pub kind: MediaKind,
    pub preview_uri: String,
    pub mime_type: MediaMimeType,
    pub byte_size: u64,
    pub duration_ms: Option<u64>,
    pub width_px: Option<u32>,
    pub height_px: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RecordingState {
    Idle,
    Recording {
        #[serde(rename = "elapsedMs")]
        elapsed_ms: u64,
    },
    Stopped {
        #[serde(rename = "elapsedMs")]
        elapsed_ms: u64,
        #[serde(rename = "stagedMediaId")]
        staged_media_id: StagedMediaId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSession {
    pub session_id: CaptureSessionId,
    pub context_resolution: ContextResolution,
    pub staged_media: Option<StagedMedia>,
    pub recording_state: RecordingState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelCaptureSessionInput {
    pub session_id: CaptureSessionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CancelCaptureSessionResult {
    pub cancelled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DismissCapturePopupResult {
    pub dismissed: bool,
    pub focus_restored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetCapturePopupLayoutInput {
    pub layout: CapturePopupLayout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SetCapturePopupLayoutResult {
    pub layout: CapturePopupLayout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectCaptureContextSourceInput {
    pub session_id: CaptureSessionId,
    pub selection: ContextSelection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SaveCaptureResult {
    pub capture_id: CaptureId,
    pub captured_at: Timestamp,
    pub enrichment_scheduled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MediaSummary {
    pub media_id: MediaId,
    pub kind: MediaKind,
    pub preview_uri: String,
    pub duration_ms: Option<u64>,
    pub width_px: Option<u32>,
    pub height_px: Option<u32>,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSummary {
    pub id: CaptureId,
    pub kind: CaptureKind,
    pub context: ContextRef,
    pub branch_name: Option<String>,
    pub captured_at: Timestamp,
    pub text_excerpt: Option<String>,
    pub caption: Option<String>,
    pub caption_source: Option<CaptionSource>,
    pub media: Option<MediaSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CaptureDetail {
    pub id: CaptureId,
    pub kind: CaptureKind,
    pub context: ContextRef,
    pub branch_name: Option<String>,
    pub captured_at: Timestamp,
    pub text_excerpt: Option<String>,
    pub caption: Option<String>,
    pub caption_source: Option<CaptionSource>,
    pub media: Option<MediaSummary>,
    pub text_body: Option<String>,
    pub source_app: Option<String>,
    pub source_window_title: Option<String>,
    pub updated_at: Timestamp,
    pub enrichment_status: EnrichmentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LibraryScope {
    All,
    Recent,
    Context {
        #[serde(rename = "contextId")]
        context_id: ContextId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListCapturesInput {
    pub scope: LibraryScope,
    pub branch_name: Option<String>,
    pub capture_kinds: Vec<CaptureKind>,
    pub captured_from: Option<Timestamp>,
    pub captured_to: Option<Timestamp>,
    pub cursor: Option<String>,
    pub limit: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetCaptureInput {
    pub capture_id: CaptureId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediaByIdInput {
    pub media_id: MediaId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OpenMediaResult {
    pub opened: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SearchMatchedField {
    TextBody,
    Caption,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchCapturesInput {
    pub query: String,
    pub context_id: Option<ContextId>,
    pub branch_name: Option<String>,
    pub capture_kinds: Vec<CaptureKind>,
    pub captured_from: Option<Timestamp>,
    pub captured_to: Option<Timestamp>,
    pub cursor: Option<String>,
    pub limit: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultItem {
    pub capture: CaptureSummary,
    pub matched_field: SearchMatchedField,
    pub snippet: String,
}

/// First strict command input contract used by the durable text slice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveTextCaptureInput {
    pub session_id: CaptureSessionId,
    pub text_body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageClipboardImageInput {
    pub session_id: CaptureSessionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscardStagedMediaInput {
    pub session_id: CaptureSessionId,
    pub staged_media_id: StagedMediaId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveImageCaptureInput {
    pub session_id: CaptureSessionId,
    pub staged_media_id: StagedMediaId,
    pub caption: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartAudioRecordingInput {
    pub session_id: CaptureSessionId,
    pub input_device_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StopAudioRecordingInput {
    pub session_id: CaptureSessionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveAudioCaptureInput {
    pub session_id: CaptureSessionId,
    pub staged_media_id: StagedMediaId,
    pub caption: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayStagedAudioInput {
    pub session_id: CaptureSessionId,
    pub staged_media_id: StagedMediaId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StopAudioPlaybackInput {
    pub playback_target_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioPlaybackResult {
    pub playing: bool,
    pub duration_ms: Option<u64>,
}

pub fn typescript_bindings() -> String {
    let config = Config::default().with_large_int("number");
    let declarations = [
        CaptureId::decl(&config),
        CaptureSessionId::decl(&config),
        ContextId::decl(&config),
        ContextSourceId::decl(&config),
        DirectorySelectionToken::decl(&config),
        MediaId::decl(&config),
        StagedMediaId::decl(&config),
        Timestamp::decl(&config),
        ErrorCode::decl(&config),
        ErrorDetailKey::decl(&config),
        ErrorDetailValue::decl(&config),
        ErrorDetails::decl(&config),
        AppError::decl(&config),
        CommandResult::<String>::decl(&config),
        CaptureKind::decl(&config),
        ContextKind::decl(&config),
        CaptionSource::decl(&config),
        ContextProviderKind::decl(&config),
        ContextSourceKind::decl(&config),
        MediaKind::decl(&config),
        CapturePopupLayout::decl(&config),
        MediaMimeType::decl(&config),
        EnrichmentStatus::decl(&config),
        ContextRef::decl(&config),
        SelectedProjectDirectory::decl(&config),
        PickProjectDirectoryResult::decl(&config),
        ListContextsInput::decl(&config),
        ListContextsResult::decl(&config),
        CreateContextInput::decl(&config),
        CreateContextResult::decl(&config),
        ContextCandidate::decl(&config),
        ContextSelection::decl(&config),
        ContextSourceOption::decl(&config),
        ListCaptureContextSourcesInput::decl(&config),
        ListCaptureContextSourcesResult::decl(&config),
        ContextResolution::decl(&config),
        StagedMedia::decl(&config),
        RecordingState::decl(&config),
        CaptureSession::decl(&config),
        CancelCaptureSessionInput::decl(&config),
        CancelCaptureSessionResult::decl(&config),
        DismissCapturePopupResult::decl(&config),
        SetCapturePopupLayoutInput::decl(&config),
        SetCapturePopupLayoutResult::decl(&config),
        SelectCaptureContextSourceInput::decl(&config),
        SaveCaptureResult::decl(&config),
        MediaSummary::decl(&config),
        CaptureSummary::decl(&config),
        CaptureDetail::decl(&config),
        Page::<String>::decl(&config),
        LibraryScope::decl(&config),
        ListCapturesInput::decl(&config),
        GetCaptureInput::decl(&config),
        MediaByIdInput::decl(&config),
        OpenMediaResult::decl(&config),
        SearchMatchedField::decl(&config),
        SearchCapturesInput::decl(&config),
        SearchResultItem::decl(&config),
        SaveTextCaptureInput::decl(&config),
        StageClipboardImageInput::decl(&config),
        DiscardStagedMediaInput::decl(&config),
        SaveImageCaptureInput::decl(&config),
        StartAudioRecordingInput::decl(&config),
        StopAudioRecordingInput::decl(&config),
        SaveAudioCaptureInput::decl(&config),
        PlayStagedAudioInput::decl(&config),
        StopAudioPlaybackInput::decl(&config),
        AudioPlaybackResult::decl(&config),
    ]
    .map(|declaration| format!("export {declaration}"));
    format!(
        "// Generated by `pnpm bindings`; do not edit.\n\n{}\n",
        declarations.join("\n\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::{Value, json};
    use std::path::PathBuf;

    fn round_trip<T>(value: &T)
    where
        T: Serialize + DeserializeOwned + std::fmt::Debug + PartialEq,
    {
        let encoded = serde_json::to_value(value).expect("shared type serializes");
        let decoded: T = serde_json::from_value(encoded).expect("shared type deserializes");
        assert_eq!(&decoded, value);
    }

    #[test]
    fn every_shared_type_round_trips() {
        let context = ContextRef {
            id: ContextId::new(),
            kind: ContextKind::Project,
            name: "Lyn".to_owned(),
        };
        let candidate = ContextCandidate {
            context: context.clone(),
            branch_name: Some("main".to_owned()),
            provider: ContextProviderKind::Vscode,
            requires_confirmation: false,
        };
        let selection = ContextSelection::LiveSource {
            source_id: ContextSourceId::new(),
        };
        let source = ContextSourceOption {
            source_id: ContextSourceId::new(),
            kind: ContextSourceKind::VscodeWindow,
            provider: ContextProviderKind::Vscode,
            application_name: "Visual Studio Code".to_owned(),
            label: "Lyn".to_owned(),
            context: context.clone(),
            branch_name: Some("main".to_owned()),
            is_foreground: true,
        };
        let staged_media = StagedMedia {
            staged_media_id: StagedMediaId::new(),
            kind: MediaKind::Audio,
            preview_uri: "lyn-media://staged/example".to_owned(),
            mime_type: MediaMimeType::AudioWav,
            byte_size: 1_024,
            duration_ms: Some(850),
            width_px: None,
            height_px: None,
        };
        let session = CaptureSession {
            session_id: CaptureSessionId::new(),
            context_resolution: ContextResolution::Resolved {
                candidate: candidate.clone(),
                selection: Some(selection.clone()),
            },
            staged_media: Some(staged_media.clone()),
            recording_state: RecordingState::Stopped {
                elapsed_ms: 850,
                staged_media_id: staged_media.staged_media_id,
            },
        };
        let captured_at: Timestamp = serde_json::from_value(json!("2026-08-28T10:30:00Z")).unwrap();
        let media = MediaSummary {
            media_id: MediaId::new(),
            kind: MediaKind::Image,
            preview_uri: "lyn-media://capture/example".to_owned(),
            duration_ms: None,
            width_px: Some(1_280),
            height_px: Some(720),
            available: true,
        };
        let summary = CaptureSummary {
            id: CaptureId::new(),
            kind: CaptureKind::Image,
            context: context.clone(),
            branch_name: Some("main".to_owned()),
            captured_at: captured_at.clone(),
            text_excerpt: None,
            caption: Some("Build output".to_owned()),
            caption_source: Some(CaptionSource::User),
            media: Some(media.clone()),
        };
        let detail = CaptureDetail {
            id: summary.id,
            kind: summary.kind,
            context: context.clone(),
            branch_name: summary.branch_name.clone(),
            captured_at: captured_at.clone(),
            text_excerpt: summary.text_excerpt.clone(),
            caption: summary.caption.clone(),
            caption_source: summary.caption_source,
            media: Some(media.clone()),
            text_body: None,
            source_app: Some("Visual Studio Code".to_owned()),
            source_window_title: Some("Lyn".to_owned()),
            updated_at: captured_at.clone(),
            enrichment_status: EnrichmentStatus::NotRequested,
        };
        let page = Page {
            items: vec![detail.clone()],
            next_cursor: Some("opaque-cursor".to_owned()),
        };
        let save_result = SaveCaptureResult {
            capture_id: summary.id,
            captured_at,
            enrichment_scheduled: false,
        };
        let input = SaveTextCaptureInput {
            session_id: session.session_id,
            text_body: "Keep this exact text".to_owned(),
        };
        let cancel_input = CancelCaptureSessionInput {
            session_id: session.session_id,
        };
        let cancel_result = CancelCaptureSessionResult { cancelled: true };
        let popup_layout_input = SetCapturePopupLayoutInput {
            layout: CapturePopupLayout::Media,
        };
        let popup_layout_result = SetCapturePopupLayoutResult {
            layout: CapturePopupLayout::Media,
        };
        let select_context_input = SelectCaptureContextSourceInput {
            session_id: session.session_id,
            selection: ContextSelection::SavedContext {
                context_id: context.id,
            },
        };
        let directory_selection = SelectedProjectDirectory {
            selected_directory_token: DirectorySelectionToken::new(),
            suggested_name: "Lyn".to_owned(),
        };
        let picker_result = PickProjectDirectoryResult {
            selection: Some(directory_selection.clone()),
        };
        let list_contexts_input = ListContextsInput {
            kind: Some(ContextKind::Project),
            query: Some("Lyn".to_owned()),
            limit: 25,
        };
        let list_contexts_result = ListContextsResult {
            contexts: vec![context.clone()],
        };
        let list_capture_sources_input = ListCaptureContextSourcesInput {
            session_id: session.session_id,
            query: Some("Lyn".to_owned()),
            limit: 25,
        };
        let list_capture_sources_result = ListCaptureContextSourcesResult {
            live_sources: vec![source.clone()],
            saved_contexts: vec![context.clone()],
        };
        let create_context_input = CreateContextInput::Project {
            name: "Lyn".to_owned(),
            selected_directory_token: directory_selection.selected_directory_token,
        };
        let create_context_result = CreateContextResult {
            context: context.clone(),
        };
        let list_captures_input = ListCapturesInput {
            scope: LibraryScope::Context {
                context_id: context.id,
            },
            branch_name: Some("main".to_owned()),
            capture_kinds: vec![CaptureKind::Text, CaptureKind::Image],
            captured_from: None,
            captured_to: None,
            cursor: Some("opaque-cursor".to_owned()),
            limit: 50,
        };
        let get_capture_input = GetCaptureInput {
            capture_id: summary.id,
        };
        let search_input = SearchCapturesInput {
            query: "build output".to_owned(),
            context_id: Some(context.id),
            branch_name: Some("main".to_owned()),
            capture_kinds: vec![CaptureKind::Image],
            captured_from: None,
            captured_to: None,
            cursor: None,
            limit: 50,
        };
        let search_result = SearchResultItem {
            capture: summary.clone(),
            matched_field: SearchMatchedField::Caption,
            snippet: "Build output".to_owned(),
        };
        let error = AppError {
            code: ErrorCode::ValidationError,
            message: "Invalid capture".to_owned(),
            retryable: false,
            details: ErrorDetails(std::collections::BTreeMap::from([
                (
                    ErrorDetailKey::Field,
                    ErrorDetailValue::String("textBody".to_owned()),
                ),
                (ErrorDetailKey::Limit, ErrorDetailValue::Number(100.0)),
            ])),
        };

        round_trip(&context);
        round_trip(&candidate);
        round_trip(&selection);
        round_trip(&source);
        round_trip(&staged_media);
        round_trip(&session);
        round_trip(&cancel_input);
        round_trip(&cancel_result);
        round_trip(&popup_layout_input);
        round_trip(&popup_layout_result);
        round_trip(&select_context_input);
        round_trip(&media);
        round_trip(&summary);
        round_trip(&detail);
        round_trip(&page);
        round_trip(&save_result);
        round_trip(&input);
        round_trip(&directory_selection);
        round_trip(&picker_result);
        round_trip(&list_contexts_input);
        round_trip(&list_contexts_result);
        round_trip(&list_capture_sources_input);
        round_trip(&list_capture_sources_result);
        round_trip(&create_context_input);
        round_trip(&create_context_result);
        round_trip(&list_captures_input);
        round_trip(&get_capture_input);
        round_trip(&search_input);
        round_trip(&search_result);
        round_trip(&CommandResult::<CaptureSession>::failure(error));
    }

    #[test]
    fn command_input_fields_use_camel_case() {
        let input = SaveTextCaptureInput {
            session_id: CaptureSessionId::new(),
            text_body: "Keep this exact text".to_owned(),
        };
        let encoded = serde_json::to_value(&input).expect("input serializes");
        assert_eq!(encoded["textBody"], "Keep this exact text");
        assert!(encoded.get("sessionId").is_some());
    }

    #[test]
    fn malformed_boundary_values_are_rejected() {
        assert!(serde_json::from_value::<CaptureKind>(json!("video")).is_err());
        assert!(serde_json::from_value::<CaptureSessionId>(json!("not-a-uuid")).is_err());
        assert!(serde_json::from_value::<Timestamp>(json!("2026-08-28 10:30")).is_err());
        assert!(serde_json::from_value::<Timestamp>(json!("2026-08-28T10:30:00+01:00")).is_err());
        assert!(serde_json::from_value::<CapturePopupLayout>(json!("600px")).is_err());
    }

    #[test]
    fn popup_layout_input_rejects_arbitrary_dimensions() {
        let forged = json!({ "layout": "media", "height": 9000 });
        assert!(serde_json::from_value::<SetCapturePopupLayoutInput>(forged).is_err());
    }

    #[test]
    fn command_inputs_reject_unknown_fields() {
        let value = json!({ "sessionId": CaptureSessionId::new(), "textBody": "draft", "absolutePath": "/private/work" });
        assert!(serde_json::from_value::<SaveTextCaptureInput>(value).is_err());

        let forged_project = json!({
            "kind": "project",
            "name": "Private project",
            "selectedDirectoryToken": DirectorySelectionToken::new(),
            "projectPath": "/private/work"
        });
        assert!(serde_json::from_value::<CreateContextInput>(forged_project).is_err());

        let unknown_list_field = json!({
            "kind": null,
            "query": null,
            "limit": 100,
            "offset": 0
        });
        assert!(serde_json::from_value::<ListContextsInput>(unknown_list_field).is_err());

        let unknown_source_list_field = json!({
            "sessionId": CaptureSessionId::new(),
            "query": null,
            "limit": 100,
            "providerToken": "private"
        });
        assert!(
            serde_json::from_value::<ListCaptureContextSourcesInput>(unknown_source_list_field)
                .is_err()
        );

        let unknown_library_field = json!({
            "scope": { "kind": "all" },
            "branchName": null,
            "captureKinds": [],
            "capturedFrom": null,
            "capturedTo": null,
            "cursor": null,
            "limit": 50,
            "offset": 0
        });
        assert!(serde_json::from_value::<ListCapturesInput>(unknown_library_field).is_err());

        let unknown_search_field = json!({
            "query": "build",
            "contextId": null,
            "branchName": null,
            "captureKinds": [],
            "capturedFrom": null,
            "capturedTo": null,
            "cursor": null,
            "limit": 50,
            "rawFts": "build*"
        });
        assert!(serde_json::from_value::<SearchCapturesInput>(unknown_search_field).is_err());
    }

    #[test]
    fn result_envelope_uses_literal_boolean_discriminants() {
        assert_eq!(
            serde_json::to_value(CommandResult::success("saved")).unwrap(),
            json!({ "ok": true, "data": "saved" })
        );
        let failure = serde_json::to_value(CommandResult::<String>::failure(AppError {
            code: ErrorCode::StorageWriteFailed,
            message: "Capture could not be saved".to_owned(),
            retryable: true,
            details: ErrorDetails::default(),
        }))
        .unwrap();
        assert_eq!(failure["ok"], false);
    }

    #[test]
    fn error_details_accept_only_public_keys() {
        let safe = json!({ "code": "VALIDATION_ERROR", "message": "Invalid capture", "retryable": false,
            "details": { "field": "textBody", "limit": 100 } });
        let error: AppError = serde_json::from_value(safe).expect("safe details deserialize");
        assert_eq!(error.code, ErrorCode::ValidationError);
        let unsafe_details = json!({ "code": "INTERNAL_ERROR", "message": "Capture failed", "retryable": false,
            "details": { "absolutePath": "/private/work" } });
        assert!(serde_json::from_value::<AppError>(unsafe_details).is_err());
    }

    #[test]
    fn timestamp_serializes_as_rfc3339_utc() {
        let timestamp: Timestamp = serde_json::from_value(json!("2026-08-28T10:30:00Z")).unwrap();
        let encoded: Value = serde_json::to_value(timestamp).unwrap();
        assert_eq!(encoded, json!("2026-08-28T10:30:00Z"));
    }

    #[test]
    fn tracked_typescript_bindings_are_current() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/lib/ipc-types.ts");
        let tracked =
            std::fs::read_to_string(path).expect("generate bindings with `pnpm bindings`");
        assert_eq!(tracked, typescript_bindings());
    }
}
