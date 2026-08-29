use std::{collections::BTreeMap, sync::Mutex};

use tauri::State;

use crate::{
    capture::session::{CaptureSessionService, SaveOnceError, SaveOnceResult, SessionStateError},
    commands::is_empty_input,
    contract::{
        CancelCaptureSessionInput, CancelCaptureSessionResult, CaptureSession, ContextCandidate,
        ContextProviderKind, ContextResolution, ContextSelection, SaveCaptureResult,
        SaveTextCaptureInput, SelectCaptureContextSourceInput,
    },
    error::{AppError, CommandResult, ErrorCode, ErrorDetailKey, ErrorDetailValue, ErrorDetails},
    storage::{Database, captures::CaptureRepository, contexts::ContextRepository},
};

pub(crate) const MAX_TEXT_BODY_BYTES: usize = 1024 * 1024;

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
) -> CommandResult<CancelCaptureSessionResult> {
    cancel_capture_session_value(input, service.inner())
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
) -> CommandResult<CaptureSession> {
    select_capture_context_source_value(input, database.inner(), service.inner())
}

fn select_capture_context_source_value(
    input: serde_json::Value,
    database: &Mutex<Database>,
    service: &Mutex<CaptureSessionService>,
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
        ContextSelection::LiveSource { .. } => {
            return CommandResult::failure(context_source_not_found_error());
        }
    };

    match service.set_context_resolution(input.session_id, resolution) {
        Ok(session) => CommandResult::success(session),
        Err(SessionStateError::StaleSession) => CommandResult::failure(stale_session_error()),
    }
}

#[tauri::command]
pub(crate) fn save_text_capture(
    input: serde_json::Value,
    database: State<'_, Mutex<Database>>,
    service: State<'_, Mutex<CaptureSessionService>>,
) -> CommandResult<SaveCaptureResult> {
    save_text_capture_value(input, database.inner(), service.inner())
}

fn save_text_capture_value(
    input: serde_json::Value,
    database: &Mutex<Database>,
    service: &Mutex<CaptureSessionService>,
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
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use crate::{
        capture::session::CaptureSessionService,
        contract::{
            CancelCaptureSessionInput, CaptureSessionId, ContextSelection, SaveTextCaptureInput,
            SelectCaptureContextSourceInput,
        },
        error::{CommandResult, ErrorCode},
        storage::{Database, contexts::ContextRepository},
    };

    use super::{
        cancel_capture_session_value, get_active_capture_session_impl,
        get_active_capture_session_value, save_text_capture_value,
        select_capture_context_source_value,
    };

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
}
