use tauri::State;

use crate::{
    commands::is_empty_input,
    contract::{
        CancelSpeechModelInstallResult, InstallSpeechModelResult, RemoveSpeechModelResult,
        SpeechModelInput, SpeechModelStatus,
    },
    error::{AppError, CommandResult, ErrorCode, ErrorDetails},
    intelligence::model::{MODEL_ID, ModelError, SpeechModelManager},
};

#[tauri::command]
pub(crate) fn get_speech_model_status(
    input: serde_json::Value,
    manager: State<'_, SpeechModelManager>,
) -> CommandResult<SpeechModelStatus> {
    if !is_empty_input(&input) {
        return CommandResult::failure(error(
            ErrorCode::ValidationError,
            "The model request is invalid",
            false,
        ));
    }
    CommandResult::success(manager.status())
}

#[tauri::command]
pub(crate) fn install_speech_model(
    input: serde_json::Value,
    app: tauri::AppHandle,
    manager: State<'_, SpeechModelManager>,
) -> CommandResult<InstallSpeechModelResult> {
    let Ok(input) = serde_json::from_value::<SpeechModelInput>(input) else {
        return CommandResult::failure(error(
            ErrorCode::ValidationError,
            "The model request is invalid",
            false,
        ));
    };
    match manager.start_install(&input.model_id, app) {
        Ok(()) => CommandResult::success(InstallSpeechModelResult {
            accepted: true,
            model_id: MODEL_ID.to_owned(),
        }),
        Err(ModelError::InvalidModelId) => CommandResult::failure(error(
            ErrorCode::ModelNotAvailable,
            "That local speech model is unavailable",
            false,
        )),
        Err(ModelError::Busy) => CommandResult::failure(error(
            ErrorCode::ModelDownloadFailed,
            "A local speech change is already running",
            true,
        )),
        Err(_) => CommandResult::failure(error(
            ErrorCode::ModelDownloadFailed,
            "The local speech model could not be installed",
            true,
        )),
    }
}

#[tauri::command]
pub(crate) fn cancel_speech_model_install(
    input: serde_json::Value,
    manager: State<'_, SpeechModelManager>,
) -> CommandResult<CancelSpeechModelInstallResult> {
    if !is_empty_input(&input) {
        return CommandResult::failure(error(
            ErrorCode::ValidationError,
            "The model request is invalid",
            false,
        ));
    }
    CommandResult::success(CancelSpeechModelInstallResult {
        cancelled: manager.cancel_install(),
    })
}

#[tauri::command]
pub(crate) fn remove_speech_model(
    input: serde_json::Value,
    manager: State<'_, SpeechModelManager>,
) -> CommandResult<RemoveSpeechModelResult> {
    let Ok(input) = serde_json::from_value::<SpeechModelInput>(input) else {
        return CommandResult::failure(error(
            ErrorCode::ValidationError,
            "The model request is invalid",
            false,
        ));
    };
    match manager.remove(&input.model_id) {
        Ok(removed) => CommandResult::success(RemoveSpeechModelResult { removed }),
        Err(ModelError::InvalidModelId) => CommandResult::failure(error(
            ErrorCode::ModelNotAvailable,
            "That local speech model is unavailable",
            false,
        )),
        Err(ModelError::Busy) => CommandResult::failure(error(
            ErrorCode::ModelDownloadFailed,
            "The local speech model is currently in use",
            true,
        )),
        Err(_) => CommandResult::failure(error(
            ErrorCode::InternalError,
            "The local speech model could not be removed",
            true,
        )),
    }
}

fn error(code: ErrorCode, message: &str, retryable: bool) -> AppError {
    AppError {
        code,
        message: message.to_owned(),
        retryable,
        details: ErrorDetails::default(),
    }
}
