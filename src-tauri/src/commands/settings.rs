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
    speech: State<'_, crate::intelligence::model::SpeechModelManager>,
) -> CommandResult<AppSettings> {
    if serde_json::from_value::<UpdateSettingsInput>(input.clone())
        .ok()
        .and_then(|request| request.patch.local_speech_enabled)
        == Some(true)
        && !speech.installed()
    {
        return CommandResult::failure(AppError {
            code: ErrorCode::ModelNotAvailable,
            message: "Install the local speech model before enabling transcription".to_owned(),
            retryable: false,
            details: ErrorDetails::default(),
        });
    }
    let mut platform = NativeSettingsPlatform::new(app);
    update_settings_value(input, database.inner(), &mut platform)
}

fn update_settings_value(
    input: serde_json::Value,
    database: &Mutex<Database>,
    platform: &mut impl crate::settings::SettingsPlatform,
) -> CommandResult<AppSettings> {
    let Ok(input) = serde_json::from_value::<UpdateSettingsInput>(input) else {
        return CommandResult::failure(validation_error("input"));
    };
    let Ok(mut database) = database.lock() else {
        return CommandResult::failure(internal_error());
    };
    match update(&mut database, input.patch, platform) {
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::json;

    use crate::{
        contract::AppSettings,
        settings::SettingsPlatform,
        storage::{Database, settings::SettingsRepository},
    };

    use super::update_settings_value;

    struct ConflictingPlatform;

    impl SettingsPlatform for ConflictingPlatform {
        fn replace_shortcut(&mut self, _current: &str, _next: &str) -> Result<(), ()> {
            Err(())
        }

        fn apply_theme(&mut self, _settings: &AppSettings) {}
    }

    #[test]
    fn update_rejects_unknown_fields_at_the_command_boundary() {
        let database = Mutex::new(Database::open_in_memory().unwrap());

        let result = update_settings_value(
            json!({ "patch": {}, "unexpected": true }),
            &database,
            &mut ConflictingPlatform,
        );

        assert_eq!(
            serde_json::to_value(result).unwrap()["error"]["code"],
            "VALIDATION_ERROR"
        );
    }

    #[test]
    fn shortcut_conflict_maps_safely_and_preserves_the_stored_setting() {
        let database = Mutex::new(Database::open_in_memory().unwrap());

        let result = update_settings_value(
            json!({ "patch": { "globalShortcut": "Control+Alt+L" } }),
            &database,
            &mut ConflictingPlatform,
        );

        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["error"]["code"], "SHORTCUT_CONFLICT");
        assert_eq!(value["error"]["retryable"], true);
        assert_eq!(
            SettingsRepository::new(database.lock().unwrap().connection())
                .get()
                .unwrap()
                .global_shortcut,
            "Control+Shift+Space"
        );
    }
}
