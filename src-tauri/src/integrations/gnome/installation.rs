//! Explicit first-install preparation; activation remains a separate action.

use super::coordinator;
use crate::contract::{InstallIntegrationResult, IntegrationId};
use std::path::Path;

pub(crate) fn prepare(home: &Path) -> InstallIntegrationResult {
    result(coordinator::prepare_first(home))
}

pub(super) fn result(
    prepared: Result<coordinator::PreparedFirst, coordinator::Failure>,
) -> InstallIntegrationResult {
    let revision = prepared.as_ref().ok().map(|first| first.revision.clone());
    InstallIntegrationResult {
        configuration_revision: revision,
        id: IntegrationId::Gnome,
        success: prepared.is_ok(),
        installed: false,
        message: if prepared.is_ok() {
            "GNOME configuration prepared. Apply it before the next terminal start."
        } else {
            "GNOME preparation could not be completed. Refresh its status before acting again."
        }
        .into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preparation_failure_never_exposes_private_errors_or_claims_installation() {
        let response = result(Err(coordinator::Failure {
            stage: coordinator::Stage::Applying,
            cause: coordinator::Cause::Io(std::io::Error::other("raw /private/installation")),
        }));
        assert!(!response.success && !response.installed);
        let payload = serde_json::to_string(&response).unwrap();
        assert!(!payload.contains("raw") && !payload.contains("/private"));
    }
}
