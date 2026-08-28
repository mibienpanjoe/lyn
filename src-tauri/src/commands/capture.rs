use std::{collections::BTreeMap, sync::Mutex};

use tauri::State;

use crate::{
    capture::session::{CaptureSessionService, SessionStateError},
    commands::is_empty_input,
    contract::{CancelCaptureSessionInput, CancelCaptureSessionResult, CaptureSession},
    error::{AppError, CommandResult, ErrorCode, ErrorDetailKey, ErrorDetailValue, ErrorDetails},
};

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
    use std::sync::Mutex;

    use serde_json::json;

    use crate::{
        capture::session::CaptureSessionService,
        contract::{CancelCaptureSessionInput, CaptureSessionId},
        error::{CommandResult, ErrorCode},
    };

    use super::{
        cancel_capture_session_value, get_active_capture_session_impl,
        get_active_capture_session_value,
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
}
