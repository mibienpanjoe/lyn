//! Gateway errors contain closed reasons, never native paths or OS errors.

use super::coordinator::{self, Cause};
use crate::{
    contract::{ManageGnomeActivationInput, ManageGnomeActivationResult},
    error::{AppError, ErrorCode, ErrorDetailKey, ErrorDetailValue, ErrorDetails},
};
use std::{collections::BTreeMap, path::Path};

pub(crate) fn execute(
    home: &Path,
    input: ManageGnomeActivationInput,
) -> Result<ManageGnomeActivationResult, AppError> {
    coordinator::manage(home, input).map_err(public_error)
}

fn public_error(failure: coordinator::Failure) -> AppError {
    let (state, message) = match failure.cause {
        Cause::StaleRevision => (
            "stale_revision",
            "The GNOME configuration changed. Refresh its status before acting again.",
        ),
        Cause::NoArtifact => (
            "artifact_unavailable",
            "Native installation is unavailable in this build.",
        ),
        Cause::InvalidLocation => (
            "location_unavailable",
            "The native installation location is unavailable.",
        ),
        Cause::Conflict => (
            "configuration_changed",
            "The GNOME configuration could not be verified. Refresh its status.",
        ),
        Cause::Service(_) => (
            "service_unavailable",
            "The GNOME service configuration could not be verified. Refresh its status.",
        ),
        Cause::Io(_) => (
            "configuration_unavailable",
            "The GNOME operation could not be completed. Refresh its status before acting again.",
        ),
    };
    AppError {
        code: ErrorCode::PermissionDenied,
        message: message.into(),
        retryable: false,
        details: ErrorDetails(BTreeMap::from([(
            ErrorDetailKey::State,
            ErrorDetailValue::String(state.into()),
        )])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn management_errors_keep_raw_failures_private_and_never_retry_automatically() {
        let error = public_error(coordinator::Failure {
            stage: coordinator::Stage::Applying,
            cause: Cause::Io(std::io::Error::other("raw-os-error /private/secret-path")),
        });
        let payload = serde_json::to_string(&error).unwrap();
        assert!(!payload.contains("raw-os-error") && !payload.contains("secret-path"));
        assert!(!error.retryable);
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert_eq!(
            error.details.0.get(&ErrorDetailKey::State),
            Some(&ErrorDetailValue::String(
                "configuration_unavailable".into()
            ))
        );
    }
}
