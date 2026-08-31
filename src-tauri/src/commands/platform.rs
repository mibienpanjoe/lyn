use std::sync::Mutex;

use tauri::{AppHandle, State, WebviewWindow};

use crate::{
    commands::is_empty_input,
    contract::{
        DismissCapturePopupResult, SetCapturePopupLayoutInput, SetCapturePopupLayoutResult,
    },
    error::{AppError, CommandResult, ErrorCode, ErrorDetails},
    platform::{CaptureWindowPlatform, InvocationContext, PlatformError},
};

#[tauri::command]
pub(crate) fn set_capture_popup_layout(
    input: SetCapturePopupLayoutInput,
    window: WebviewWindow,
) -> CommandResult<SetCapturePopupLayoutResult> {
    match crate::platform::popup::resize_capture_popup(&window, input.layout) {
        Ok(()) => CommandResult::success(SetCapturePopupLayoutResult {
            layout: input.layout,
        }),
        Err(_) => CommandResult::failure(resize_error()),
    }
}

#[tauri::command]
pub(crate) fn dismiss_capture_popup(
    input: serde_json::Value,
    app: AppHandle,
    invocation: State<'_, Mutex<InvocationContext>>,
) -> CommandResult<DismissCapturePopupResult> {
    if !is_empty_input(&input) {
        return CommandResult::failure(validation_error());
    }

    #[cfg(target_os = "linux")]
    let mut platform = crate::platform::x11::X11CaptureWindowPlatform::new(app);

    #[cfg(not(target_os = "linux"))]
    let mut platform = crate::platform::UnsupportedCaptureWindowPlatform::new(app);

    let Ok(mut invocation) = invocation.lock() else {
        return CommandResult::failure(internal_error());
    };
    let had_foreground = invocation.has_foreground();
    match invocation.dismiss(&mut platform) {
        Ok(()) => CommandResult::success(DismissCapturePopupResult {
            dismissed: true,
            focus_restored: had_foreground,
        }),
        Err(PlatformError::Unsupported | PlatformError::FocusFailed) => {
            let hidden = platform.hide_capture_popup().is_ok();
            if hidden {
                CommandResult::success(DismissCapturePopupResult {
                    dismissed: true,
                    focus_restored: false,
                })
            } else {
                CommandResult::failure(internal_error())
            }
        }
    }
}

fn validation_error() -> AppError {
    AppError {
        code: ErrorCode::ValidationError,
        message: "The popup request is invalid".to_owned(),
        retryable: false,
        details: ErrorDetails::default(),
    }
}

fn internal_error() -> AppError {
    AppError {
        code: ErrorCode::InternalError,
        message: "Lyn could not dismiss the capture popup".to_owned(),
        retryable: true,
        details: ErrorDetails::default(),
    }
}

fn resize_error() -> AppError {
    AppError {
        code: ErrorCode::InternalError,
        message: "Lyn could not resize the capture popup".to_owned(),
        retryable: true,
        details: ErrorDetails::default(),
    }
}
