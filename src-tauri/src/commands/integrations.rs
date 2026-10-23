use std::collections::BTreeMap;

use crate::{
    commands::is_empty_input,
    contract::{InstallIntegrationInput, InstallIntegrationResult, IntegrationStatus},
    error::{AppError, CommandResult, ErrorCode, ErrorDetailKey, ErrorDetailValue, ErrorDetails},
    integrations::{install_integration_by_id, list_integration_statuses, user_home_dir},
};

#[tauri::command]
pub(crate) async fn get_integration_statuses(
    app: tauri::AppHandle,
    input: serde_json::Value,
) -> CommandResult<Vec<IntegrationStatus>> {
    if let Err(error) = validate_status_input(&input) {
        return CommandResult::failure(error);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
        CommandResult::success(Vec::new())
    }
    #[cfg(target_os = "linux")]
    {
        use tauri::Manager;
        let broker = app
            .try_state::<std::sync::Arc<crate::context::gnome_broker::NativeBroker>>()
            .map(|broker| std::sync::Arc::clone(broker.inner()));
        let home = user_home_dir();
        match tauri::async_runtime::spawn_blocking(move || {
            let mut statuses = list_integration_statuses(&home);
            let connection = broker.as_ref().map_or(
                crate::context::gnome_broker::NativeConnectionStatus::Unavailable,
                |broker| broker.connection_status(),
            );
            statuses.push(crate::integrations::gnome::status::observe(
                &home, connection,
            ));
            statuses
        })
        .await
        {
            Ok(statuses) => CommandResult::success(statuses),
            Err(_) => CommandResult::failure(AppError {
                code: ErrorCode::InternalError,
                message: "Integration status is unavailable.".to_owned(),
                retryable: true,
                details: ErrorDetails::default(),
            }),
        }
    }
}

fn validate_status_input(input: &serde_json::Value) -> Result<(), AppError> {
    if is_empty_input(input) {
        Ok(())
    } else {
        Err(validation_error("input"))
    }
}

#[tauri::command]
pub(crate) fn install_integration(
    input: serde_json::Value,
) -> CommandResult<InstallIntegrationResult> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = input;
        CommandResult::failure(AppError {
            code: ErrorCode::PermissionDenied,
            message: "Context integrations are only supported on Linux in this release.".to_owned(),
            retryable: false,
            details: ErrorDetails::default(),
        })
    }
    #[cfg(target_os = "linux")]
    {
        let Ok(input) = serde_json::from_value::<InstallIntegrationInput>(input) else {
            return CommandResult::failure(validation_error("input"));
        };
        let home = user_home_dir();
        let result = install_integration_by_id(&home, input);
        CommandResult::success(result)
    }
}

fn validation_error(field: &str) -> AppError {
    AppError {
        code: ErrorCode::ValidationError,
        message: "The integration request is invalid".to_owned(),
        retryable: false,
        details: ErrorDetails(BTreeMap::from([(
            ErrorDetailKey::Field,
            ErrorDetailValue::String(field.to_owned()),
        )])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn get_integration_statuses_rejects_non_empty_input() {
        assert!(validate_status_input(&json!({ "unexpected": true })).is_err());
        assert!(validate_status_input(&json!({})).is_ok());
    }

    #[test]
    fn install_integration_rejects_malformed_input() {
        let result = install_integration(json!({ "id": "unknown_tool" }));
        assert!(matches!(result, CommandResult::Failure { .. }));
    }
}
