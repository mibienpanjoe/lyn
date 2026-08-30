//! Narrow operating-system ports and adapters.

use std::fmt;

#[cfg(target_os = "linux")]
pub(crate) mod x11;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct WindowCorrelationToken(u64);

impl WindowCorrelationToken {
    pub(crate) fn from_native(value: u64) -> Self {
        Self(value)
    }

    pub(crate) fn native(self) -> u64 {
        self.0
    }
}

impl fmt::Debug for WindowCorrelationToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WindowCorrelationToken(<opaque>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ForegroundWindowIdentity {
    pub(crate) window: WindowCorrelationToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlatformError {
    Unsupported,
    FocusFailed,
}

pub(crate) trait CaptureWindowPlatform {
    fn capture_foreground(&mut self) -> Result<ForegroundWindowIdentity, PlatformError>;
    fn show_capture_popup(&mut self) -> Result<(), PlatformError>;
    fn hide_capture_popup(&mut self) -> Result<(), PlatformError>;
    fn restore_foreground(
        &mut self,
        identity: ForegroundWindowIdentity,
    ) -> Result<(), PlatformError>;
}

#[derive(Default)]
pub(crate) struct InvocationContext {
    foreground: Option<ForegroundWindowIdentity>,
}

impl InvocationContext {
    pub(crate) fn record_foreground(&mut self, foreground: Option<ForegroundWindowIdentity>) {
        self.foreground = foreground;
    }

    #[cfg(test)]
    pub(crate) fn invoke(
        &mut self,
        platform: &mut impl CaptureWindowPlatform,
    ) -> Result<(), PlatformError> {
        self.foreground = platform.capture_foreground().ok();
        platform.show_capture_popup()
    }

    pub(crate) fn dismiss(
        &mut self,
        platform: &mut impl CaptureWindowPlatform,
    ) -> Result<(), PlatformError> {
        platform.hide_capture_popup()?;
        let Some(foreground) = self.foreground.take() else {
            return Ok(());
        };
        platform.restore_foreground(foreground)
    }

    pub(crate) fn has_foreground(&self) -> bool {
        self.foreground.is_some()
    }

    pub(crate) fn foreground(&self) -> Option<ForegroundWindowIdentity> {
        self.foreground
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CaptureWindowPlatform, ForegroundWindowIdentity, InvocationContext, PlatformError,
        WindowCorrelationToken,
    };

    struct FakePlatform {
        calls: Vec<&'static str>,
        capture: Result<ForegroundWindowIdentity, PlatformError>,
        restore: Result<(), PlatformError>,
    }

    impl CaptureWindowPlatform for FakePlatform {
        fn capture_foreground(&mut self) -> Result<ForegroundWindowIdentity, PlatformError> {
            self.calls.push("capture");
            self.capture
        }

        fn show_capture_popup(&mut self) -> Result<(), PlatformError> {
            self.calls.push("show");
            Ok(())
        }

        fn hide_capture_popup(&mut self) -> Result<(), PlatformError> {
            self.calls.push("hide");
            Ok(())
        }

        fn restore_foreground(
            &mut self,
            _identity: ForegroundWindowIdentity,
        ) -> Result<(), PlatformError> {
            self.calls.push("restore");
            self.restore
        }
    }

    fn identity() -> ForegroundWindowIdentity {
        ForegroundWindowIdentity {
            window: WindowCorrelationToken::from_native(42),
        }
    }

    #[test]
    fn captures_foreground_before_showing_and_restores_after_hiding() {
        let mut platform = FakePlatform {
            calls: Vec::new(),
            capture: Ok(identity()),
            restore: Ok(()),
        };
        let mut invocation = InvocationContext::default();

        invocation.invoke(&mut platform).unwrap();
        invocation.dismiss(&mut platform).unwrap();

        assert_eq!(platform.calls, ["capture", "show", "hide", "restore"]);
        assert_eq!(invocation.foreground(), None);
    }

    #[test]
    fn unsupported_foreground_capture_still_shows_and_hides_the_popup() {
        let mut platform = FakePlatform {
            calls: Vec::new(),
            capture: Err(PlatformError::Unsupported),
            restore: Ok(()),
        };
        let mut invocation = InvocationContext::default();

        invocation.invoke(&mut platform).unwrap();
        invocation.dismiss(&mut platform).unwrap();

        assert_eq!(platform.calls, ["capture", "show", "hide"]);
        assert_eq!(invocation.foreground(), None);
    }

    #[test]
    fn opaque_tokens_do_not_reveal_native_window_values_in_diagnostics() {
        let token = WindowCorrelationToken::from_native(123_456);

        assert_eq!(format!("{token:?}"), "WindowCorrelationToken(<opaque>)");
        assert!(!format!("{token:?}").contains("123456"));
    }

    #[test]
    fn focus_restore_failure_is_recoverable_after_the_popup_is_hidden() {
        let mut platform = FakePlatform {
            calls: Vec::new(),
            capture: Ok(identity()),
            restore: Err(PlatformError::FocusFailed),
        };
        let mut invocation = InvocationContext::default();

        invocation.invoke(&mut platform).unwrap();
        assert_eq!(
            invocation.dismiss(&mut platform),
            Err(PlatformError::FocusFailed)
        );

        assert_eq!(platform.calls, ["capture", "show", "hide", "restore"]);
        assert_eq!(invocation.foreground(), None);
    }
}
