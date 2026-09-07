use std::collections::BTreeMap;

use crate::{
    commands::is_empty_input,
    contract::{InstallIntegrationInput, InstallIntegrationResult, IntegrationStatus},
    error::{AppError, CommandResult, ErrorCode, ErrorDetailKey, ErrorDetailValue, ErrorDetails},
    integrations::{install_integration_by_id, list_integration_statuses, user_home_dir},
};

#[tauri::command]
pub(crate) fn get_integration_statuses(
    input: serde_json::Value,
) -> CommandResult<Vec<IntegrationStatus>> {
    if !is_empty_input(&input) {
        return CommandResult::failure(validation_error("input"));
    }
    let home = user_home_dir();
    CommandResult::success(list_integration_statuses(&home))
}

#[tauri::command]
pub(crate) fn install_integration(
    input: serde_json::Value,
) -> CommandResult<InstallIntegrationResult> {
    let Ok(input) = serde_json::from_value::<InstallIntegrationInput>(input) else {
        return CommandResult::failure(validation_error("input"));
    };
    let home = user_home_dir();
    let result = install_integration_by_id(&home, input);
    CommandResult::success(result)
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
        let result = get_integration_statuses(json!({ "unexpected": true }));
        assert!(matches!(result, CommandResult::Failure { .. }));
    }

    #[test]
    fn install_integration_rejects_malformed_input() {
        let result = install_integration(json!({ "id": "unknown_tool" }));
        assert!(matches!(result, CommandResult::Failure { .. }));
    }
}
