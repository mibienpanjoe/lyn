use std::{collections::BTreeMap, sync::Mutex};

use tauri::{AppHandle, State};

use crate::{
    commands::is_empty_input,
    contract::{AppSettings, UpdateSettingsInput},
    error::{AppError, CommandResult, ErrorCode, ErrorDetailKey, ErrorDetailValue, ErrorDetails},
    platform::settings::NativeSettingsPlatform,
    settings::{SettingsError, update},
    storage::{Database, settings::SettingsRepository},
};

#[tauri::command]
pub(crate) fn get_settings(
    input: serde_json::Value,
    database: State<'_, Mutex<Database>>,
) -> CommandResult<AppSettings> {
    if !is_empty_input(&input) {
        return CommandResult::failure(validation_error("input"));
    }
    let Ok(database) = database.lock() else {
        return CommandResult::failure(internal_error());
    };
    match SettingsRepository::new(database.connection()).get() {
        Ok(settings) => CommandResult::success(settings),
        Err(_) => CommandResult::failure(storage_error()),
    }
}

#[tauri::command]
pub(crate) fn update_settings(
    input: serde_json::Value,
    app: AppHandle,
    database: State<'_, Mutex<Database>>,
) -> CommandResult<AppSettings> {
    let Ok(input) = serde_json::from_value::<UpdateSettingsInput>(input) else {
        return CommandResult::failure(validation_error("input"));
    };
    let Ok(mut database) = database.lock() else {
        return CommandResult::failure(internal_error());
    };
    let mut platform = NativeSettingsPlatform::new(app);
    match update(&mut database, input.patch, &mut platform) {
        Ok(settings) => CommandResult::success(settings),
        Err(SettingsError::InvalidShortcut) => {
            CommandResult::failure(validation_error("globalShortcut"))
        }
        Err(SettingsError::InvalidProviderOrder) => {
            CommandResult::failure(validation_error("providerTieBreakOrder"))
        }
        Err(SettingsError::ShortcutConflict) => CommandResult::failure(AppError {
            code: ErrorCode::ShortcutConflict,
            message: "That shortcut is already in use".to_owned(),
            retryable: true,
            details: ErrorDetails::default(),
        }),
        Err(SettingsError::Storage) => CommandResult::failure(storage_write_error()),
    }
}

fn validation_error(field: &str) -> AppError {
    AppError {
        code: ErrorCode::ValidationError,
        message: "The settings request is invalid".to_owned(),
        retryable: false,
        details: ErrorDetails(BTreeMap::from([(
            ErrorDetailKey::Field,
            ErrorDetailValue::String(field.to_owned()),
        )])),
    }
}

fn storage_error() -> AppError {
    AppError {
        code: ErrorCode::StorageUnavailable,
        message: "Settings are temporarily unavailable".to_owned(),
        retryable: true,
        details: ErrorDetails::default(),
    }
}

fn storage_write_error() -> AppError {
    AppError {
        code: ErrorCode::StorageWriteFailed,
        message: "Settings could not be saved".to_owned(),
        retryable: true,
        details: ErrorDetails::default(),
    }
}

fn internal_error() -> AppError {
    AppError {
        code: ErrorCode::InternalError,
        message: "Lyn could not update settings".to_owned(),
        retryable: true,
        details: ErrorDetails::default(),
    }
}
