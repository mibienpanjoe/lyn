//! Stable, content-safe application error contract.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    ValidationError,
    StaleSession,
    ContextRequired,
    ContextAmbiguous,
    ContextSourceNotFound,
    ContextSourceStale,
    EmptyCapture,
    CaptureNotFound,
    ContextNotFound,
    StorageUnavailable,
    StorageWriteFailed,
    MediaStageFailed,
    MediaFinalizeFailed,
    MediaNotFound,
    UnsupportedClipboardContent,
    PermissionDenied,
    AudioDeviceUnavailable,
    AudioRecordingFailed,
    AudioPlaybackFailed,
    SearchFailed,
    ShortcutConflict,
    ModelNotAvailable,
    ModelDownloadFailed,
    EnrichmentFailed,
    InternalError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ErrorDetailKey {
    Field,
    Limit,
    Operation,
    Permission,
    ResourceKind,
    RetryAfterMs,
    State,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum ErrorDetailValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Null,
}

/// Allowlisted metadata only. Capture content, raw OS errors, and private paths
/// cannot be named as error details through this type.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(transparent)]
pub struct ErrorDetails(pub BTreeMap<ErrorDetailKey, ErrorDetailValue>);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    pub details: ErrorDetails,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub struct True;

impl Serialize for True {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bool(true)
    }
}

impl<'de> Deserialize<'de> for True {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if bool::deserialize(deserializer)? {
            Ok(Self)
        } else {
            Err(de::Error::custom("expected true"))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub struct False;

impl Serialize for False {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bool(false)
    }
}

impl<'de> Deserialize<'de> for False {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if bool::deserialize(deserializer)? {
            Err(de::Error::custom("expected false"))
        } else {
            Ok(Self)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum CommandResult<T> {
    Success {
        #[ts(type = "true")]
        ok: True,
        data: T,
    },
    Failure {
        #[ts(type = "false")]
        ok: False,
        error: AppError,
    },
}

impl<T> CommandResult<T> {
    pub fn success(data: T) -> Self {
        Self::Success { ok: True, data }
    }
    pub fn failure(error: AppError) -> Self {
        Self::Failure { ok: False, error }
    }
}
