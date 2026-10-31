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

#[tauri::command]
pub(crate) fn retry_voice_transcription(
    input: serde_json::Value,
    app: tauri::AppHandle,
    database: State<'_, std::sync::Mutex<crate::storage::Database>>,
    manager: State<'_, SpeechModelManager>,
) -> CommandResult<crate::contract::VoiceCaptureMutationResult> {
    let Ok(request) =
        serde_json::from_value::<crate::contract::RetryVoiceTranscriptionInput>(input)
    else {
        return CommandResult::failure(error(
            ErrorCode::ValidationError,
            "The transcription request is invalid",
            false,
        ));
    };
    if !manager.installed() {
        return CommandResult::failure(error(
            ErrorCode::ModelNotAvailable,
            "Install Parakeet in Settings before retrying",
            false,
        ));
    }
    let result = (|| {
        let mut database = database.lock().map_err(|_| ())?;
        if !crate::storage::settings::SettingsRepository::new(database.connection())
            .get()
            .map_err(|_| ())?
            .local_speech_enabled
        {
            return Ok(None);
        }
        crate::enrichment::EnrichmentQueue::new(database.connection_mut())
            .retry_voice(request.capture_id, request.expected_caption_revision)
            .map(Some)
            .map_err(|_| ())
    })();
    match result {
        Ok(Some(true)) => {
            crate::spawn_enrichment_worker(app);
            CommandResult::success(crate::contract::VoiceCaptureMutationResult { accepted: true })
        }
        Ok(None) => CommandResult::failure(error(
            ErrorCode::ModelNotAvailable,
            "Enable local speech in Settings before retrying",
            false,
        )),
        Ok(Some(false)) => CommandResult::failure(error(
            ErrorCode::EnrichmentFailed,
            "This voice note changed or is no longer eligible. Refresh it before retrying",
            false,
        )),
        Err(()) => CommandResult::failure(error(
            ErrorCode::StorageWriteFailed,
            "The transcription could not be queued",
            true,
        )),
    }
}

#[tauri::command]
pub(crate) fn update_voice_caption(
    input: serde_json::Value,
    database: State<'_, std::sync::Mutex<crate::storage::Database>>,
) -> CommandResult<crate::contract::VoiceCaptureMutationResult> {
    update_voice_caption_value(input, database.inner())
}

fn update_voice_caption_value(
    input: serde_json::Value,
    database: &std::sync::Mutex<crate::storage::Database>,
) -> CommandResult<crate::contract::VoiceCaptureMutationResult> {
    let Ok(request) = serde_json::from_value::<crate::contract::UpdateVoiceCaptionInput>(input)
    else {
        return CommandResult::failure(error(
            ErrorCode::ValidationError,
            "The caption request is invalid",
            false,
        ));
    };
    let caption = request.caption.trim();
    if caption.chars().count() > 500 {
        return CommandResult::failure(error(
            ErrorCode::ValidationError,
            "Captions must be at most 500 characters",
            false,
        ));
    }
    let result = database.lock().map_err(|_| ()).and_then(|mut database| {
        crate::enrichment::EnrichmentQueue::new(database.connection_mut())
            .update_voice_caption(
                request.capture_id,
                request.expected_caption_revision,
                caption,
            )
            .map_err(|_| ())
    });
    match result {
        Ok(true) => {
            CommandResult::success(crate::contract::VoiceCaptureMutationResult { accepted: true })
        }
        Ok(false) => CommandResult::failure(error(
            ErrorCode::EnrichmentFailed,
            "This voice note changed. Reopen the caption editor before saving your correction",
            false,
        )),
        Err(()) => CommandResult::failure(error(
            ErrorCode::StorageWriteFailed,
            "The caption could not be saved",
            true,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caption_gateway_rejects_paths_unknown_fields_and_oversized_content() {
        let database = std::sync::Mutex::new(crate::storage::Database::open_in_memory().unwrap());
        for input in [
            serde_json::json!({"captureId":"not-an-id", "expectedCaptionRevision":0, "caption":"hello"}),
            serde_json::json!({"captureId":uuid::Uuid::new_v4(), "expectedCaptionRevision":0, "caption":"hello", "path":"/tmp/audio.wav"}),
            serde_json::json!({"captureId":uuid::Uuid::new_v4(), "expectedCaptionRevision":0, "caption":"a".repeat(501)}),
        ] {
            assert!(matches!(
                update_voice_caption_value(input, &database),
                CommandResult::Failure {
                    error: AppError {
                        code: ErrorCode::ValidationError,
                        ..
                    },
                    ..
                }
            ));
        }
    }
}
