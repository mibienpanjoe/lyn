use std::{collections::BTreeMap, sync::Mutex, time::Instant};

use tauri::State;

use crate::{
    capture::session::{CaptureSessionService, SaveOnceError, SaveOnceResult, SessionStateError},
    commands::is_empty_input,
    context::{provider::ProviderSourceKind, session_registry::ContextSourceRegistry},
    contract::{
        CancelCaptureSessionInput, CancelCaptureSessionResult, CaptureSession, ContextCandidate,
        ContextProviderKind, ContextResolution, ContextSelection, ContextSourceKind,
        ContextSourceOption, ListCaptureContextSourcesInput, ListCaptureContextSourcesResult,
        SaveCaptureResult, SaveImageCaptureInput, SaveTextCaptureInput,
        SelectCaptureContextSourceInput, StageClipboardImageInput, StagedMedia,
    },
    error::{AppError, CommandResult, ErrorCode, ErrorDetailKey, ErrorDetailValue, ErrorDetails},
    media::{images, staging::MediaStore},
    platform::{
        InvocationContext,
        clipboard::{ClipboardError, ClipboardImagePlatform, NativeClipboardPlatform},
    },
    storage::{Database, captures::CaptureRepository, contexts::ContextRepository},
};

pub(crate) const MAX_TEXT_BODY_BYTES: usize = 1024 * 1024;
const MAX_CONTEXT_SOURCE_QUERY_CHARS: usize = 100;

#[tauri::command]
pub(crate) fn list_capture_context_sources(
    input: serde_json::Value,
    database: State<'_, Mutex<Database>>,
    service: State<'_, Mutex<CaptureSessionService>>,
    registry: State<'_, Mutex<ContextSourceRegistry>>,
    invocation: State<'_, Mutex<InvocationContext>>,
) -> CommandResult<ListCaptureContextSourcesResult> {
    let foreground = invocation
        .lock()
        .ok()
        .and_then(|invocation| invocation.foreground())
        .map(|identity| identity.window);
    list_capture_context_sources_value(
        input,
        database.inner(),
        service.inner(),
        registry.inner(),
        foreground,
    )
}

#[tauri::command]
pub(crate) fn stage_clipboard_image(
    input: serde_json::Value,
    service: State<'_, Mutex<CaptureSessionService>>,
    media_store: State<'_, Mutex<MediaStore>>,
    clipboard: State<'_, Mutex<NativeClipboardPlatform>>,
) -> CommandResult<StagedMedia> {
    stage_clipboard_image_value(
        input,
        service.inner(),
        media_store.inner(),
        clipboard.inner(),
    )
}

#[tauri::command]
pub(crate) fn save_image_capture(
    input: serde_json::Value,
    database: State<'_, Mutex<Database>>,
    service: State<'_, Mutex<CaptureSessionService>>,
    media_store: State<'_, Mutex<MediaStore>>,
) -> CommandResult<SaveCaptureResult> {
    save_image_capture_value(
        input,
        database.inner(),
        service.inner(),
        media_store.inner(),
    )
}

fn save_image_capture_value(
    input: serde_json::Value,
    database: &Mutex<Database>,
    service: &Mutex<CaptureSessionService>,
    media_store: &Mutex<MediaStore>,
) -> CommandResult<SaveCaptureResult> {
    let Ok(input) = serde_json::from_value::<SaveImageCaptureInput>(input) else {
        return CommandResult::failure(validation_error());
    };
    let caption = normalize_optional_caption(input.caption);
    let Ok(mut service) = service.lock() else {
        return CommandResult::failure(internal_error());
    };
    match service.save_once(input.session_id, |session| {
        let crate::contract::ContextResolution::Resolved { candidate, .. } =
            &session.context_resolution
        else {
            return Err(context_required_error());
        };
        if session
            .staged_media
            .as_ref()
            .map(|media| media.staged_media_id)
            != Some(input.staged_media_id)
        {
            return Err(media_stage_error());
        }
        let capture_id = crate::contract::CaptureId::new();
        let mut media = media_store.lock().map_err(|_| internal_error())?;
        let finalized = media
            .finalize(
                input.staged_media_id,
                capture_id,
                crate::contract::MediaKind::Image,
            )
            .map_err(|_| media_stage_error())?;
        let dimensions = session
            .staged_media
            .as_ref()
            .and_then(|media| media.width_px.zip(media.height_px))
            .ok_or_else(media_stage_error)?;
        let saved = database
            .lock()
            .map_err(|_| internal_error())
            .and_then(|mut database| {
                CaptureRepository::new(database.connection_mut())
                    .save_image(
                        input.session_id,
                        candidate.context.id,
                        candidate.branch_name.as_deref(),
                        capture_id,
                        finalized.media_id,
                        &finalized.relative_path,
                        finalized.byte_size,
                        &finalized.checksum,
                        caption.as_deref(),
                        dimensions.0,
                        dimensions.1,
                    )
                    .map_err(|_| storage_write_error())
            });
        if saved.is_err() {
            let _ = media.remove_final(&finalized.relative_path);
        }
        saved.map(|saved| (capture_id, saved))
    }) {
        Ok(SaveOnceResult::Saved { value, .. }) => CommandResult::success(value),
        Ok(SaveOnceResult::AlreadySaved(_)) | Err(SaveOnceError::Session(_)) => {
            CommandResult::failure(stale_session_error())
        }
        Err(SaveOnceError::Persistence(error)) => CommandResult::failure(error),
    }
}

fn normalize_optional_caption(caption: Option<String>) -> Option<String> {
    caption.filter(|caption| !caption.trim().is_empty())
}

fn stage_clipboard_image_value<Clipboard>(
    input: serde_json::Value,
    service: &Mutex<CaptureSessionService>,
    media_store: &Mutex<MediaStore>,
    clipboard: &Mutex<Clipboard>,
) -> CommandResult<StagedMedia>
where
    Clipboard: ClipboardImagePlatform,
{
    let Ok(input) = serde_json::from_value::<StageClipboardImageInput>(input) else {
        return CommandResult::failure(validation_error());
    };
    let Ok(mut service) = service.lock() else {
        return CommandResult::failure(internal_error());
    };
    let previous_staged_media_id = service
        .active_session()
        .filter(|session| session.session_id == input.session_id)
        .and_then(|session| session.staged_media.map(|media| media.staged_media_id));
    if service.active_session().map(|session| session.session_id) != Some(input.session_id) {
        return CommandResult::failure(stale_session_error());
    }
    let image = match clipboard.lock() {
        Ok(mut clipboard) => match clipboard.read_image() {
            Ok(image) => image,
            Err(ClipboardError::UnsupportedContent) => {
                return CommandResult::failure(unsupported_clipboard_error());
            }
            Err(ClipboardError::Unavailable) => {
                return CommandResult::failure(clipboard_unavailable_error());
            }
        },
        Err(_) => return CommandResult::failure(internal_error()),
    };
    let Ok(mut media_store) = media_store.lock() else {
        return CommandResult::failure(internal_error());
    };
    let staged = match images::stage_clipboard_image(&mut media_store, input.session_id, image) {
        Ok(staged) => staged,
        Err(_) => return CommandResult::failure(media_stage_error()),
    };
    match service.set_staged_media(input.session_id, staged.clone()) {
        Ok(_) => {
            if let Some(previous) = previous_staged_media_id {
                let _ = media_store.discard_staged(input.session_id, previous);
            }
            CommandResult::success(staged)
        }
        Err(SessionStateError::StaleSession) => CommandResult::failure(stale_session_error()),
    }
}

fn list_capture_context_sources_value(
    input: serde_json::Value,
    database: &Mutex<Database>,
    service: &Mutex<CaptureSessionService>,
    registry: &Mutex<ContextSourceRegistry>,
    foreground: Option<crate::platform::WindowCorrelationToken>,
) -> CommandResult<ListCaptureContextSourcesResult> {
    let Ok(input) = serde_json::from_value::<ListCaptureContextSourcesInput>(input) else {
        return CommandResult::failure(validation_error());
    };
    if input.limit == 0
        || input.limit > 100
        || input.query.as_ref().is_some_and(|query| {
            query.chars().count() > MAX_CONTEXT_SOURCE_QUERY_CHARS
                || query.chars().any(char::is_control)
        })
    {
        return CommandResult::failure(validation_error());
    }
    if service
        .lock()
        .ok()
        .and_then(|service| service.active_session())
        .map(|session| session.session_id)
        != Some(input.session_id)
    {
        return CommandResult::failure(stale_session_error());
    }

    let query = input.query.as_deref().map(str::to_lowercase);
    let source_rows = {
        let Ok(mut registry) = registry.lock() else {
            return CommandResult::failure(internal_error());
        };
        registry
            .live_sources(Instant::now())
            .into_iter()
            .filter(|source| {
                query.as_ref().is_none_or(|query| {
                    source.label().to_lowercase().contains(query)
                        || source.application_name().to_lowercase().contains(query)
                })
            })
            .take(usize::from(input.limit))
            .map(|source| {
                let identity = source.identity();
                (
                    ContextSourceOption {
                        source_id: source.source_id(),
                        kind: public_source_kind(source.source_kind()),
                        provider: source.provider(),
                        application_name: source.application_name().to_owned(),
                        label: source.label().to_owned(),
                        context: source.context().clone(),
                        branch_name: identity.branch_name.clone(),
                        is_foreground: source.window() == foreground,
                    },
                    identity.project_key.clone(),
                    identity.project_path.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    let Ok(database) = database.lock() else {
        return CommandResult::failure(storage_unavailable_error());
    };
    let repository = ContextRepository::new(database.connection());
    let saved_contexts = match repository.list(None, input.query.as_deref(), input.limit) {
        Ok(contexts) => contexts,
        Err(_) => return CommandResult::failure(storage_unavailable_error()),
    };
    let mut live_sources = Vec::with_capacity(source_rows.len());
    for (mut option, project_key, project_path) in source_rows {
        match repository.find_project(project_key.as_deref(), &project_path) {
            Ok(Some(context)) => option.context = context,
            Ok(None) => {}
            Err(_) => return CommandResult::failure(storage_unavailable_error()),
        }
        live_sources.push(option);
    }
    CommandResult::success(ListCaptureContextSourcesResult {
        live_sources,
        saved_contexts,
    })
}

fn public_source_kind(kind: ProviderSourceKind) -> ContextSourceKind {
    match kind {
        ProviderSourceKind::VscodeWindow => ContextSourceKind::VscodeWindow,
        ProviderSourceKind::VscodeIntegratedTerminal => ContextSourceKind::IntegratedTerminal,
        ProviderSourceKind::ExternalTerminal => ContextSourceKind::ExternalTerminal,
        ProviderSourceKind::ShellSession => ContextSourceKind::Shell,
        ProviderSourceKind::ForegroundWindow => ContextSourceKind::ForegroundWindow,
    }
}

#[tauri::command]
pub(crate) fn get_active_capture_session(
    input: serde_json::Value,
    service: State<'_, Mutex<CaptureSessionService>>,
) -> CommandResult<CaptureSession> {
    get_active_capture_session_value(input, service.inner())
}

fn get_active_capture_session_value(
    input: serde_json::Value,
    service: &Mutex<CaptureSessionService>,
) -> CommandResult<CaptureSession> {
    if !is_empty_input(&input) {
        return CommandResult::failure(validation_error());
    }
    get_active_capture_session_impl(service)
}

fn get_active_capture_session_impl(
    service: &Mutex<CaptureSessionService>,
) -> CommandResult<CaptureSession> {
    let Ok(mut service) = service.lock() else {
        return CommandResult::failure(internal_error());
    };
    CommandResult::success(service.get_or_prepare())
}

#[tauri::command]
pub(crate) fn cancel_capture_session(
    input: serde_json::Value,
    service: State<'_, Mutex<CaptureSessionService>>,
    media_store: State<'_, Mutex<MediaStore>>,
) -> CommandResult<CancelCaptureSessionResult> {
    let result = cancel_capture_session_value(input, service.inner());
    if matches!(result, CommandResult::Success { .. }) {
        drain_staging_cleanup(service.inner(), media_store.inner());
    }
    result
}

fn drain_staging_cleanup(service: &Mutex<CaptureSessionService>, media_store: &Mutex<MediaStore>) {
    if let (Ok(mut service), Ok(mut media_store)) = (service.lock(), media_store.lock()) {
        while let Some(cleanup) = service.take_cleanup_request() {
            let _ = media_store.discard_staged(cleanup.session_id, cleanup.staged_media_id);
        }
    }
}

fn cancel_capture_session_value(
    input: serde_json::Value,
    service: &Mutex<CaptureSessionService>,
) -> CommandResult<CancelCaptureSessionResult> {
    let Ok(input) = serde_json::from_value::<CancelCaptureSessionInput>(input) else {
        return CommandResult::failure(validation_error());
    };
    let Ok(mut service) = service.lock() else {
        return CommandResult::failure(internal_error());
    };
    match service.cancel(input.session_id) {
        Ok(()) => CommandResult::success(CancelCaptureSessionResult { cancelled: true }),
        Err(SessionStateError::StaleSession) => CommandResult::failure(stale_session_error()),
    }
}

#[tauri::command]
pub(crate) fn select_capture_context_source(
    input: serde_json::Value,
    database: State<'_, Mutex<Database>>,
    service: State<'_, Mutex<CaptureSessionService>>,
    registry: State<'_, Mutex<ContextSourceRegistry>>,
) -> CommandResult<CaptureSession> {
    select_capture_context_source_with_registry_value(
        input,
        database.inner(),
        service.inner(),
        registry.inner(),
    )
}

fn select_capture_context_source_with_registry_value(
    input: serde_json::Value,
    database: &Mutex<Database>,
    service: &Mutex<CaptureSessionService>,
    registry: &Mutex<ContextSourceRegistry>,
) -> CommandResult<CaptureSession> {
    let Ok(input) = serde_json::from_value::<SelectCaptureContextSourceInput>(input) else {
        return CommandResult::failure(validation_error());
    };
    let Ok(mut service) = service.lock() else {
        return CommandResult::failure(internal_error());
    };
    if service.active_session().map(|session| session.session_id) != Some(input.session_id) {
        return CommandResult::failure(stale_session_error());
    }

    let resolution = match input.selection {
        ContextSelection::SavedContext { context_id } => {
            let context = {
                let Ok(database) = database.lock() else {
                    return CommandResult::failure(internal_error());
                };
                match ContextRepository::new(database.connection()).get(context_id) {
                    Ok(Some(context)) => context,
                    Ok(None) => return CommandResult::failure(context_not_found_error()),
                    Err(_) => return CommandResult::failure(storage_unavailable_error()),
                }
            };
            ContextResolution::Resolved {
                candidate: ContextCandidate {
                    context,
                    branch_name: None,
                    provider: ContextProviderKind::Manual,
                    requires_confirmation: false,
                },
                selection: Some(ContextSelection::SavedContext { context_id }),
            }
        }
        ContextSelection::LiveSource { source_id } => {
            let source = {
                let Ok(mut registry) = registry.lock() else {
                    return CommandResult::failure(internal_error());
                };
                let Some(source) = registry.get(source_id, Instant::now()) else {
                    return CommandResult::failure(context_source_not_found_error());
                };
                (
                    source.context().clone(),
                    source.identity().project_key.clone(),
                    source.identity().project_path.clone(),
                    source.identity().branch_name.clone(),
                    source.provider(),
                )
            };
            let context = {
                let Ok(database) = database.lock() else {
                    return CommandResult::failure(internal_error());
                };
                match ContextRepository::new(database.connection()).ensure_project(
                    source.0.id,
                    &source.0.name,
                    source.1.as_deref(),
                    &source.2,
                ) {
                    Ok(context) => context,
                    Err(_) => return CommandResult::failure(storage_unavailable_error()),
                }
            };
            ContextResolution::Resolved {
                candidate: ContextCandidate {
                    context,
                    branch_name: source.3,
                    provider: source.4,
                    requires_confirmation: false,
                },
                selection: Some(ContextSelection::LiveSource { source_id }),
            }
        }
    };

    match service.set_context_resolution(input.session_id, resolution) {
        Ok(session) => CommandResult::success(session),
        Err(SessionStateError::StaleSession) => CommandResult::failure(stale_session_error()),
    }
}

#[cfg(test)]
fn select_capture_context_source_value(
    input: serde_json::Value,
    database: &Mutex<Database>,
    service: &Mutex<CaptureSessionService>,
) -> CommandResult<CaptureSession> {
    select_capture_context_source_with_registry_value(
        input,
        database,
        service,
        &Mutex::new(ContextSourceRegistry::default()),
    )
}

#[tauri::command]
pub(crate) fn save_text_capture(
    input: serde_json::Value,
    database: State<'_, Mutex<Database>>,
    service: State<'_, Mutex<CaptureSessionService>>,
    registry: State<'_, Mutex<ContextSourceRegistry>>,
) -> CommandResult<SaveCaptureResult> {
    save_text_capture_with_registry_value(
        input,
        database.inner(),
        service.inner(),
        registry.inner(),
    )
}

fn save_text_capture_with_registry_value(
    input: serde_json::Value,
    database: &Mutex<Database>,
    service: &Mutex<CaptureSessionService>,
    registry: &Mutex<ContextSourceRegistry>,
) -> CommandResult<SaveCaptureResult> {
    let Ok(input) = serde_json::from_value::<SaveTextCaptureInput>(input) else {
        return CommandResult::failure(validation_error());
    };
    if input.text_body.len() > MAX_TEXT_BODY_BYTES {
        return CommandResult::failure(text_limit_error());
    }
    if input.text_body.trim().is_empty() {
        return CommandResult::failure(empty_capture_error());
    }

    let Ok(mut service) = service.lock() else {
        return CommandResult::failure(internal_error());
    };
    if service
        .active_session()
        .is_none_or(|session| session.session_id != input.session_id)
    {
        return CommandResult::failure(stale_session_error());
    }
    if let Some(session) = service.active_session()
        && let ContextResolution::Resolved {
            selection: Some(ContextSelection::LiveSource { source_id }),
            ..
        } = session.context_resolution
    {
        let refreshed = {
            let Ok(mut registry) = registry.lock() else {
                return CommandResult::failure(internal_error());
            };
            registry.get(source_id, Instant::now()).map(|source| {
                (
                    source.context().clone(),
                    source.identity().project_key.clone(),
                    source.identity().project_path.clone(),
                    source.identity().branch_name.clone(),
                    source.provider(),
                )
            })
        };
        let Some(refreshed) = refreshed else {
            return CommandResult::failure(context_source_stale_error());
        };
        let context = {
            let Ok(database) = database.lock() else {
                return CommandResult::failure(internal_error());
            };
            match ContextRepository::new(database.connection()).ensure_project(
                refreshed.0.id,
                &refreshed.0.name,
                refreshed.1.as_deref(),
                &refreshed.2,
            ) {
                Ok(context) => context,
                Err(_) => return CommandResult::failure(storage_unavailable_error()),
            }
        };
        if service
            .set_context_resolution(
                input.session_id,
                ContextResolution::Resolved {
                    candidate: ContextCandidate {
                        context,
                        branch_name: refreshed.3,
                        provider: refreshed.4,
                        requires_confirmation: false,
                    },
                    selection: Some(ContextSelection::LiveSource { source_id }),
                },
            )
            .is_err()
        {
            return CommandResult::failure(stale_session_error());
        }
    }
    match service.save_once(input.session_id, |session| {
        let (context_id, branch_name) = match &session.context_resolution {
            ContextResolution::Resolved { candidate, .. } => {
                (candidate.context.id, candidate.branch_name.as_deref())
            }
            ContextResolution::Ambiguous { .. } => return Err(context_ambiguous_error()),
            ContextResolution::Required { .. } => return Err(context_required_error()),
        };
        let Ok(mut database) = database.lock() else {
            return Err(internal_error());
        };
        let saved = CaptureRepository::new(database.connection_mut())
            .save_text(input.session_id, context_id, &input.text_body, branch_name)
            .map_err(|_| storage_write_error())?;
        Ok((saved.capture_id, saved))
    }) {
        Ok(SaveOnceResult::Saved { value, .. }) => CommandResult::success(value),
        Ok(SaveOnceResult::AlreadySaved(_))
        | Err(SaveOnceError::Session(SessionStateError::StaleSession)) => {
            CommandResult::failure(stale_session_error())
        }
        Err(SaveOnceError::Persistence(error)) => CommandResult::failure(error),
    }
}

#[cfg(test)]
fn save_text_capture_value(
    input: serde_json::Value,
    database: &Mutex<Database>,
    service: &Mutex<CaptureSessionService>,
) -> CommandResult<SaveCaptureResult> {
    save_text_capture_with_registry_value(
        input,
        database,
        service,
        &Mutex::new(ContextSourceRegistry::default()),
    )
}

fn validation_error() -> AppError {
    AppError {
        code: ErrorCode::ValidationError,
        message: "The session request is invalid".to_owned(),
        retryable: false,
        details: ErrorDetails(BTreeMap::from([(
            ErrorDetailKey::Field,
            ErrorDetailValue::String("input".to_owned()),
        )])),
    }
}

fn text_limit_error() -> AppError {
    AppError {
        code: ErrorCode::ValidationError,
        message: "The text capture exceeds the supported size".to_owned(),
        retryable: false,
        details: ErrorDetails(BTreeMap::from([
            (
                ErrorDetailKey::Field,
                ErrorDetailValue::String("textBody".to_owned()),
            ),
            (
                ErrorDetailKey::Limit,
                ErrorDetailValue::Number(MAX_TEXT_BODY_BYTES as f64),
            ),
        ])),
    }
}

fn empty_capture_error() -> AppError {
    AppError {
        code: ErrorCode::EmptyCapture,
        message: "Enter text before saving".to_owned(),
        retryable: false,
        details: ErrorDetails(BTreeMap::from([(
            ErrorDetailKey::Field,
            ErrorDetailValue::String("textBody".to_owned()),
        )])),
    }
}

fn context_required_error() -> AppError {
    AppError {
        code: ErrorCode::ContextRequired,
        message: "Choose a context before saving".to_owned(),
        retryable: true,
        details: ErrorDetails::default(),
    }
}

fn context_ambiguous_error() -> AppError {
    AppError {
        code: ErrorCode::ContextAmbiguous,
        message: "Choose one context before saving".to_owned(),
        retryable: true,
        details: ErrorDetails::default(),
    }
}

fn context_not_found_error() -> AppError {
    AppError {
        code: ErrorCode::ContextNotFound,
        message: "The selected context is unavailable".to_owned(),
        retryable: false,
        details: ErrorDetails::default(),
    }
}

fn context_source_not_found_error() -> AppError {
    AppError {
        code: ErrorCode::ContextSourceNotFound,
        message: "The selected context source is unavailable".to_owned(),
        retryable: true,
        details: ErrorDetails::default(),
    }
}

fn context_source_stale_error() -> AppError {
    AppError {
        code: ErrorCode::ContextSourceStale,
        message: "The selected context source is stale".to_owned(),
        retryable: true,
        details: ErrorDetails::default(),
    }
}

fn storage_write_error() -> AppError {
    AppError {
        code: ErrorCode::StorageWriteFailed,
        message: "The capture could not be saved".to_owned(),
        retryable: true,
        details: ErrorDetails::default(),
    }
}

fn storage_unavailable_error() -> AppError {
    AppError {
        code: ErrorCode::StorageUnavailable,
        message: "Contexts are temporarily unavailable".to_owned(),
        retryable: true,
        details: ErrorDetails::default(),
    }
}

fn stale_session_error() -> AppError {
    AppError {
        code: ErrorCode::StaleSession,
        message: "This capture session is no longer active".to_owned(),
        retryable: false,
        details: ErrorDetails(BTreeMap::from([(
            ErrorDetailKey::State,
            ErrorDetailValue::String("stale".to_owned()),
        )])),
    }
}

fn unsupported_clipboard_error() -> AppError {
    AppError {
        code: ErrorCode::UnsupportedClipboardContent,
        message: "The clipboard does not contain a supported image".to_owned(),
        retryable: true,
        details: ErrorDetails::default(),
    }
}

fn clipboard_unavailable_error() -> AppError {
    AppError {
        code: ErrorCode::PermissionDenied,
        message: "The clipboard is unavailable".to_owned(),
        retryable: true,
        details: ErrorDetails::default(),
    }
}

fn media_stage_error() -> AppError {
    AppError {
        code: ErrorCode::MediaStageFailed,
        message: "The image could not be staged".to_owned(),
        retryable: true,
        details: ErrorDetails::default(),
    }
}

fn internal_error() -> AppError {
    AppError {
        code: ErrorCode::InternalError,
        message: "Lyn could not complete the request".to_owned(),
        retryable: false,
        details: ErrorDetails::default(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Instant,
    };

    use serde_json::json;
    use tempfile::tempdir;

    use crate::{
        capture::session::CaptureSessionService,
        context::{
            provider::{
                CorrelationToken, ObservationLiveness, ProviderObservation, ProviderSourceKind,
            },
            session_registry::ContextSourceRegistry,
        },
        contract::{
            CancelCaptureSessionInput, CaptureSessionId, ContextCandidate, ContextProviderKind,
            ContextResolution, ContextSelection, ListCaptureContextSourcesInput, MediaKind,
            MediaMimeType, SaveImageCaptureInput, SaveTextCaptureInput,
            SelectCaptureContextSourceInput,
        },
        error::{CommandResult, ErrorCode},
        media::staging::MediaStore,
        platform::{
            WindowCorrelationToken,
            clipboard::{ClipboardError, ClipboardImage, ClipboardImagePlatform},
        },
        storage::{Database, contexts::ContextRepository},
    };

    use super::{
        cancel_capture_session_value, drain_staging_cleanup, get_active_capture_session_impl,
        get_active_capture_session_value, list_capture_context_sources_value,
        save_image_capture_value, save_text_capture_value, save_text_capture_with_registry_value,
        select_capture_context_source_value, select_capture_context_source_with_registry_value,
        stage_clipboard_image_value,
    };

    struct FakeClipboard(Result<ClipboardImage, ClipboardError>);

    impl ClipboardImagePlatform for FakeClipboard {
        fn read_image(&mut self) -> Result<ClipboardImage, ClipboardError> {
            self.0.clone()
        }
    }

    #[test]
    fn image_staging_uses_the_native_port_and_preserves_only_opaque_session_media() {
        let directory = tempdir().unwrap();
        let service = Mutex::new(CaptureSessionService::default());
        let session = service.lock().unwrap().get_or_prepare();
        let media = Mutex::new(MediaStore::open(directory.path()).unwrap());
        let clipboard = Mutex::new(FakeClipboard(Ok(ClipboardImage {
            width: 1,
            height: 1,
            rgba: vec![1, 2, 3, 255],
        })));

        let result = stage_clipboard_image_value(
            serde_json::to_value(crate::contract::StageClipboardImageInput {
                session_id: session.session_id,
            })
            .unwrap(),
            &service,
            &media,
            &clipboard,
        );

        let CommandResult::Success { data: staged, .. } = result else {
            panic!("image staging failed")
        };
        assert_eq!(staged.width_px, Some(1));
        assert!(
            !serde_json::to_string(&staged)
                .unwrap()
                .contains(&directory.path().display().to_string())
        );
        assert_eq!(
            service
                .lock()
                .unwrap()
                .active_session()
                .unwrap()
                .staged_media,
            Some(staged.clone())
        );

        let replacement = stage_clipboard_image_value(
            json!({ "sessionId": session.session_id }),
            &service,
            &media,
            &clipboard,
        );
        let CommandResult::Success {
            data: replacement, ..
        } = replacement
        else {
            panic!("replacement staging failed")
        };
        assert!(
            media
                .lock()
                .unwrap()
                .staged_preview(staged.staged_media_id)
                .is_err()
        );
        assert!(
            media
                .lock()
                .unwrap()
                .staged_preview(replacement.staged_media_id)
                .is_ok()
        );
    }

    #[test]
    fn unsupported_clipboard_content_leaves_the_active_session_unchanged() {
        let directory = tempdir().unwrap();
        let service = Mutex::new(CaptureSessionService::default());
        let session = service.lock().unwrap().get_or_prepare();
        let media = Mutex::new(MediaStore::open(directory.path()).unwrap());
        let clipboard = Mutex::new(FakeClipboard(Err(ClipboardError::UnsupportedContent)));

        let result = stage_clipboard_image_value(
            json!({ "sessionId": session.session_id }),
            &service,
            &media,
            &clipboard,
        );

        let CommandResult::Failure { error, .. } = result else {
            panic!("unsupported clipboard succeeded")
        };
        assert_eq!(error.code, ErrorCode::UnsupportedClipboardContent);
        assert_eq!(service.lock().unwrap().active_session().unwrap(), session);
    }

    #[test]
    fn image_save_commits_exact_manual_caption_and_matching_media_atomically() {
        let directory = tempdir().unwrap();
        let database = Database::open_in_memory().unwrap();
        let context = ContextRepository::new(database.connection())
            .create_standalone("Screenshots")
            .unwrap();
        let service = Mutex::new(CaptureSessionService::default());
        let session = service.lock().unwrap().get_or_prepare();
        service
            .lock()
            .unwrap()
            .set_context_resolution(
                session.session_id,
                ContextResolution::Resolved {
                    candidate: ContextCandidate {
                        context,
                        branch_name: None,
                        provider: ContextProviderKind::Manual,
                        requires_confirmation: false,
                    },
                    selection: None,
                },
            )
            .unwrap();
        let mut media = MediaStore::open(directory.path()).unwrap();
        let staged = media
            .stage_image_png(session.session_id, b"validated-png", 2, 1)
            .unwrap();
        service
            .lock()
            .unwrap()
            .set_staged_media(session.session_id, staged.clone())
            .unwrap();
        let database = Mutex::new(database);
        let media = Mutex::new(media);

        let result = save_image_capture_value(
            serde_json::to_value(SaveImageCaptureInput {
                session_id: session.session_id,
                staged_media_id: staged.staged_media_id,
                caption: Some("  exact caption  ".to_owned()),
            })
            .unwrap(),
            &database,
            &service,
            &media,
        );

        let CommandResult::Success { data: saved, .. } = result else {
            panic!("image save failed")
        };
        let stored: (String, String, String, i64, i64) = database
            .lock()
            .unwrap()
            .connection()
            .query_row(
                "SELECT captures.caption, captures.caption_source, media_assets.kind,
                        media_assets.width_px, media_assets.height_px
                 FROM captures JOIN media_assets ON media_assets.capture_id = captures.id
                 WHERE captures.id = ?1",
                [saved.capture_id.to_string()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            stored,
            (
                "  exact caption  ".to_owned(),
                "user".to_owned(),
                "image".to_owned(),
                2,
                1
            )
        );
        assert!(matches!(staged.kind, MediaKind::Image));
        assert!(matches!(staged.mime_type, MediaMimeType::ImagePng));
    }

    #[test]
    fn image_caption_normalization_keeps_authored_text_and_nulls_only_blank_input() {
        assert_eq!(
            super::normalize_optional_caption(Some("  authored  ".to_owned())),
            Some("  authored  ".to_owned())
        );
        assert_eq!(
            super::normalize_optional_caption(Some(" \n\t ".to_owned())),
            None
        );
        assert_eq!(super::normalize_optional_caption(None), None);
    }

    #[test]
    fn repeated_get_returns_the_same_active_session() {
        let service = Mutex::new(CaptureSessionService::default());

        let first = get_active_capture_session_impl(&service);
        let repeated = get_active_capture_session_impl(&service);

        let CommandResult::Success { data: first, .. } = first else {
            panic!("first get failed")
        };
        let CommandResult::Success { data: repeated, .. } = repeated else {
            panic!("repeated get failed")
        };
        assert_eq!(repeated, first);
    }

    #[test]
    fn cancel_is_idempotent_but_unknown_session_is_stale() {
        let service = Mutex::new(CaptureSessionService::default());
        let CommandResult::Success { data: session, .. } =
            get_active_capture_session_impl(&service)
        else {
            panic!("get failed")
        };
        let input = serde_json::to_value(CancelCaptureSessionInput {
            session_id: session.session_id,
        })
        .unwrap();

        let first = cancel_capture_session_value(input.clone(), &service);
        let repeated = cancel_capture_session_value(input, &service);
        let stale =
            cancel_capture_session_value(json!({ "sessionId": CaptureSessionId::new() }), &service);

        assert!(matches!(first, CommandResult::Success { .. }));
        assert_eq!(repeated, first);
        let CommandResult::Failure { error, .. } = stale else {
            panic!("unknown cancel succeeded")
        };
        assert_eq!(error.code, ErrorCode::StaleSession);
    }

    #[test]
    fn cancellation_removes_only_the_cancelled_sessions_staged_media() {
        let directory = tempdir().unwrap();
        let service = Mutex::new(CaptureSessionService::default());
        let session = service.lock().unwrap().get_or_prepare();
        let mut media = MediaStore::open(directory.path()).unwrap();
        let staged = media
            .stage_image_png(session.session_id, b"png", 1, 1)
            .unwrap();
        service
            .lock()
            .unwrap()
            .set_staged_media(session.session_id, staged.clone())
            .unwrap();
        let media = Mutex::new(media);

        let result =
            cancel_capture_session_value(json!({ "sessionId": session.session_id }), &service);
        drain_staging_cleanup(&service, &media);

        assert!(matches!(result, CommandResult::Success { .. }));
        assert!(
            media
                .lock()
                .unwrap()
                .staged_preview(staged.staged_media_id)
                .is_err()
        );
    }

    #[test]
    fn malformed_cancel_returns_validation_error_without_cancelling() {
        let service = Mutex::new(CaptureSessionService::default());
        let CommandResult::Success { data: active, .. } = get_active_capture_session_impl(&service)
        else {
            panic!("get failed")
        };

        let malformed = cancel_capture_session_value(
            json!({ "sessionId": "not-a-uuid", "unexpected": true }),
            &service,
        );
        let current = get_active_capture_session_impl(&service);

        let CommandResult::Failure { error, .. } = malformed else {
            panic!("malformed cancel succeeded")
        };
        assert_eq!(error.code, ErrorCode::ValidationError);
        let CommandResult::Success { data: current, .. } = current else {
            panic!("get failed")
        };
        assert_eq!(current, active);
    }

    #[test]
    fn malformed_get_does_not_prepare_a_session() {
        let service = Mutex::new(CaptureSessionService::default());

        let malformed = get_active_capture_session_value(json!({ "unexpected": true }), &service);

        let CommandResult::Failure { error, .. } = malformed else {
            panic!("malformed get succeeded")
        };
        assert_eq!(error.code, ErrorCode::ValidationError);
        assert!(service.lock().unwrap().active_session().is_none());
    }

    #[test]
    fn saved_context_selection_and_text_save_form_one_durable_flow() {
        let database = Mutex::new(Database::open_in_memory().unwrap());
        let service = Mutex::new(CaptureSessionService::default());
        let context = {
            let database = database.lock().unwrap();
            ContextRepository::new(database.connection())
                .create_standalone("Notes")
                .unwrap()
        };
        let CommandResult::Success { data: session, .. } =
            get_active_capture_session_impl(&service)
        else {
            panic!("get failed")
        };
        let selection = serde_json::to_value(SelectCaptureContextSourceInput {
            session_id: session.session_id,
            selection: ContextSelection::SavedContext {
                context_id: context.id,
            },
        })
        .unwrap();
        let body = "  Première ligne\n第二行  ";
        let save_input = serde_json::to_value(SaveTextCaptureInput {
            session_id: session.session_id,
            text_body: body.to_owned(),
        })
        .unwrap();

        let selected = select_capture_context_source_value(selection, &database, &service);
        let saved = save_text_capture_value(save_input.clone(), &database, &service);
        let CommandResult::Success {
            data: next_session, ..
        } = get_active_capture_session_impl(&service)
        else {
            panic!("next get failed")
        };
        let replayed = save_text_capture_value(save_input, &database, &service);

        assert!(matches!(selected, CommandResult::Success { .. }));
        let CommandResult::Success { data: saved, .. } = saved else {
            panic!("save failed")
        };
        let database = database.lock().unwrap();
        let (stored_body, indexed_body, capture_count): (String, String, i64) = database
            .connection()
            .query_row(
                "SELECT captures.text_body, captures_fts.search_text,
                        (SELECT count(*) FROM captures)
                 FROM captures
                 JOIN captures_fts ON captures_fts.capture_id = captures.id
                 WHERE captures.id = ?1",
                [saved.capture_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!((stored_body.as_str(), indexed_body.as_str()), (body, body));
        assert_eq!(capture_count, 1);
        drop(database);
        let CommandResult::Failure { error, .. } = replayed else {
            panic!("replayed save succeeded")
        };
        assert_eq!(error.code, ErrorCode::StaleSession);
        assert_eq!(service.lock().unwrap().active_session(), Some(next_session));
    }

    #[test]
    fn blank_oversized_and_contextless_text_preserve_the_session() {
        let database = Mutex::new(Database::open_in_memory().unwrap());
        let service = Mutex::new(CaptureSessionService::default());
        let CommandResult::Success { data: session, .. } =
            get_active_capture_session_impl(&service)
        else {
            panic!("get failed")
        };

        let blank = save_text_capture_value(
            json!({ "sessionId": session.session_id, "textBody": " \n\t " }),
            &database,
            &service,
        );
        let oversized = save_text_capture_value(
            json!({ "sessionId": session.session_id, "textBody": "a".repeat(super::MAX_TEXT_BODY_BYTES + 1) }),
            &database,
            &service,
        );
        let contextless = save_text_capture_value(
            json!({ "sessionId": session.session_id, "textBody": "draft" }),
            &database,
            &service,
        );

        let codes = [blank, oversized, contextless].map(|result| match result {
            CommandResult::Failure { error, .. } => error.code,
            CommandResult::Success { .. } => panic!("invalid save succeeded"),
        });
        assert_eq!(
            codes,
            [
                ErrorCode::EmptyCapture,
                ErrorCode::ValidationError,
                ErrorCode::ContextRequired,
            ]
        );
        assert_eq!(service.lock().unwrap().active_session(), Some(session));
    }

    #[test]
    fn storage_failure_keeps_the_same_session_retryable() {
        let database = Mutex::new(Database::open_in_memory().unwrap());
        let service = Mutex::new(CaptureSessionService::default());
        let context = {
            let database = database.lock().unwrap();
            ContextRepository::new(database.connection())
                .create_standalone("Notes")
                .unwrap()
        };
        let CommandResult::Success { data: session, .. } =
            get_active_capture_session_impl(&service)
        else {
            panic!("get failed")
        };
        select_capture_context_source_value(
            serde_json::to_value(SelectCaptureContextSourceInput {
                session_id: session.session_id,
                selection: ContextSelection::SavedContext {
                    context_id: context.id,
                },
            })
            .unwrap(),
            &database,
            &service,
        );
        database
            .lock()
            .unwrap()
            .connection()
            .execute_batch(
                "CREATE TRIGGER reject_text_capture BEFORE INSERT ON captures
                 BEGIN SELECT RAISE(ABORT, 'simulated write failure'); END;",
            )
            .unwrap();
        let input = json!({ "sessionId": session.session_id, "textBody": "retry me" });

        let failed = save_text_capture_value(input.clone(), &database, &service);
        assert_eq!(
            service.lock().unwrap().active_session().unwrap().session_id,
            session.session_id
        );
        database
            .lock()
            .unwrap()
            .connection()
            .execute_batch("DROP TRIGGER reject_text_capture;")
            .unwrap();
        let retried = save_text_capture_value(input, &database, &service);

        let CommandResult::Failure { error, .. } = failed else {
            panic!("forced failure succeeded")
        };
        assert_eq!(error.code, ErrorCode::StorageWriteFailed);
        assert!(matches!(retried, CommandResult::Success { .. }));
    }

    #[test]
    fn malformed_save_does_not_mutate_session_or_storage() {
        let database = Mutex::new(Database::open_in_memory().unwrap());
        let service = Mutex::new(CaptureSessionService::default());
        let CommandResult::Success { data: session, .. } =
            get_active_capture_session_impl(&service)
        else {
            panic!("get failed")
        };

        let malformed = save_text_capture_value(
            json!({
                "sessionId": "not-a-uuid",
                "textBody": "draft",
                "title": "must not exist"
            }),
            &database,
            &service,
        );

        let CommandResult::Failure { error, .. } = malformed else {
            panic!("malformed save succeeded")
        };
        assert_eq!(error.code, ErrorCode::ValidationError);
        assert_eq!(service.lock().unwrap().active_session(), Some(session));
        let count: i64 = database
            .lock()
            .unwrap()
            .connection()
            .query_row("SELECT count(*) FROM captures", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn concurrent_duplicate_saves_publish_exactly_once() {
        let database = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let service = Arc::new(Mutex::new(CaptureSessionService::default()));
        let context = {
            let database = database.lock().unwrap();
            ContextRepository::new(database.connection())
                .create_standalone("Notes")
                .unwrap()
        };
        let CommandResult::Success { data: session, .. } =
            get_active_capture_session_impl(&service)
        else {
            panic!("get failed")
        };
        select_capture_context_source_value(
            serde_json::to_value(SelectCaptureContextSourceInput {
                session_id: session.session_id,
                selection: ContextSelection::SavedContext {
                    context_id: context.id,
                },
            })
            .unwrap(),
            &database,
            &service,
        );
        let input = json!({ "sessionId": session.session_id, "textBody": "once" });

        let results = std::thread::scope(|scope| {
            let first_database = Arc::clone(&database);
            let first_service = Arc::clone(&service);
            let first_input = input.clone();
            let first = scope.spawn(move || {
                save_text_capture_value(first_input, &first_database, &first_service)
            });
            let second_database = Arc::clone(&database);
            let second_service = Arc::clone(&service);
            let second = scope
                .spawn(move || save_text_capture_value(input, &second_database, &second_service));
            [first.join().unwrap(), second.join().unwrap()]
        });

        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, CommandResult::Success { .. }))
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(
                    result,
                    CommandResult::Failure { error, .. }
                        if error.code == ErrorCode::StaleSession
                ))
                .count(),
            1
        );
        let count: i64 = database
            .lock()
            .unwrap()
            .connection()
            .query_row("SELECT count(*) FROM captures", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn invalid_or_unavailable_context_selection_preserves_the_session() {
        let database = Mutex::new(Database::open_in_memory().unwrap());
        let service = Mutex::new(CaptureSessionService::default());
        let CommandResult::Success { data: session, .. } =
            get_active_capture_session_impl(&service)
        else {
            panic!("get failed")
        };

        let malformed = select_capture_context_source_value(
            json!({
                "sessionId": session.session_id,
                "selection": { "kind": "saved_context", "contextId": "not-a-uuid" },
                "unexpected": true
            }),
            &database,
            &service,
        );
        let missing = select_capture_context_source_value(
            json!({
                "sessionId": session.session_id,
                "selection": {
                    "kind": "saved_context",
                    "contextId": crate::contract::ContextId::new()
                }
            }),
            &database,
            &service,
        );
        let unavailable_live = select_capture_context_source_value(
            json!({
                "sessionId": session.session_id,
                "selection": {
                    "kind": "live_source",
                    "sourceId": crate::contract::ContextSourceId::new()
                }
            }),
            &database,
            &service,
        );

        let codes = [malformed, missing, unavailable_live].map(|result| match result {
            CommandResult::Failure { error, .. } => error.code,
            CommandResult::Success { .. } => panic!("invalid selection succeeded"),
        });
        assert_eq!(
            codes,
            [
                ErrorCode::ValidationError,
                ErrorCode::ContextNotFound,
                ErrorCode::ContextSourceNotFound,
            ]
        );
        assert_eq!(service.lock().unwrap().active_session(), Some(session));
    }

    #[test]
    fn live_source_list_selection_and_stale_save_preserve_the_session() {
        let directory = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let window = WindowCorrelationToken::from_native(77);
        let process = CorrelationToken::new();
        let session_token = CorrelationToken::new();
        let live = ProviderObservation::new(
            ContextProviderKind::Vscode,
            ProviderSourceKind::VscodeIntegratedTerminal,
            Some(window),
            Some(process),
            Some(session_token),
            directory.path().to_path_buf(),
            now,
            ObservationLiveness::Live,
        );
        let database = Mutex::new(Database::open_in_memory().unwrap());
        let service = Mutex::new(CaptureSessionService::default());
        let registry = Mutex::new(ContextSourceRegistry::default());
        let source_id = registry.lock().unwrap().register(live, now).unwrap();
        let active = service.lock().unwrap().get_or_prepare();

        let listed = list_capture_context_sources_value(
            serde_json::to_value(ListCaptureContextSourcesInput {
                session_id: active.session_id,
                query: None,
                limit: 10,
            })
            .unwrap(),
            &database,
            &service,
            &registry,
            Some(window),
        );
        let CommandResult::Success { data: listed, .. } = listed else {
            panic!("list failed")
        };
        assert_eq!(listed.live_sources.len(), 1);
        assert!(listed.live_sources[0].is_foreground);
        assert!(
            !serde_json::to_string(&listed)
                .unwrap()
                .contains(&directory.path().display().to_string())
        );

        let selected = select_capture_context_source_with_registry_value(
            serde_json::to_value(SelectCaptureContextSourceInput {
                session_id: active.session_id,
                selection: ContextSelection::LiveSource { source_id },
            })
            .unwrap(),
            &database,
            &service,
            &registry,
        );
        assert!(matches!(selected, CommandResult::Success { .. }));

        registry.lock().unwrap().register(
            ProviderObservation::new(
                ContextProviderKind::Vscode,
                ProviderSourceKind::VscodeIntegratedTerminal,
                Some(window),
                Some(process),
                Some(session_token),
                directory.path().to_path_buf(),
                Instant::now(),
                ObservationLiveness::Ended,
            ),
            Instant::now(),
        );
        let before = service.lock().unwrap().active_session().unwrap();
        let stale = save_text_capture_with_registry_value(
            serde_json::to_value(SaveTextCaptureInput {
                session_id: active.session_id,
                text_body: "draft remains".to_owned(),
            })
            .unwrap(),
            &database,
            &service,
            &registry,
        );

        let CommandResult::Failure { error, .. } = stale else {
            panic!("stale source saved")
        };
        assert_eq!(error.code, ErrorCode::ContextSourceStale);
        assert_eq!(service.lock().unwrap().active_session(), Some(before));
        let capture_count: i64 = database
            .lock()
            .unwrap()
            .connection()
            .query_row("SELECT count(*) FROM captures", [], |row| row.get(0))
            .unwrap();
        assert_eq!(capture_count, 0);
    }
}
