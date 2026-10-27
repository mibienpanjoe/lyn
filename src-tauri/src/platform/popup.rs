use tauri::{PhysicalSize, WebviewWindow};

use crate::contract::CapturePopupLayout;

const COMPACT_HEIGHT: f64 = 210.0;
const AUDIO_HEIGHT: f64 = 300.0;
const ERROR_HEIGHT: f64 = 280.0;
const CHOOSER_HEIGHT: f64 = 460.0;
const MEDIA_HEIGHT: f64 = 560.0;

pub(crate) enum PopupResizeError {
    InvalidViewport,
    Platform,
}

fn viewport_scale(native_width: u32, viewport_width: f64) -> Option<f64> {
    if !viewport_width.is_finite() || !(1.0..=16384.0).contains(&viewport_width) {
        return None;
    }
    let ratio = f64::from(native_width) / viewport_width;
    (ratio.is_finite() && (0.5..=8.0).contains(&ratio)).then_some(ratio)
}

fn rendered_height(layout: CapturePopupLayout, ratio: f64, available: u32) -> u32 {
    ((layout.height() * ratio).ceil() as u32).min(available.max(1))
}

pub(crate) fn resize_capture_popup(
    window: &WebviewWindow,
    layout: CapturePopupLayout,
    viewport_width: Option<f64>,
) -> Result<(), PopupResizeError> {
    let scale_factor = window
        .scale_factor()
        .map_err(|_| PopupResizeError::Platform)?;
    let current_size = window
        .inner_size()
        .map_err(|_| PopupResizeError::Platform)?;
    // WebKitGTK text scaling changes CSS viewport width without changing DPR.
    // Rust owns the physical width; the UI supplies only its measured viewport.
    let ratio = match viewport_width {
        Some(width) => {
            viewport_scale(current_size.width, width).ok_or(PopupResizeError::InvalidViewport)?
        }
        None => scale_factor,
    };
    let available = window
        .current_monitor()
        .map_err(|_| PopupResizeError::Platform)?
        .map_or(4480, |monitor| {
            monitor
                .work_area()
                .size
                .height
                .saturating_sub((48.0 * scale_factor).ceil() as u32)
        });
    let height = rendered_height(layout, ratio, available);
    window
        .set_size(PhysicalSize::new(current_size.width, height))
        .map_err(|_| PopupResizeError::Platform)
}

impl CapturePopupLayout {
    fn height(self) -> f64 {
        match self {
            Self::Compact => COMPACT_HEIGHT,
            Self::Audio => AUDIO_HEIGHT,
            Self::Error => ERROR_HEIGHT,
            Self::Chooser => CHOOSER_HEIGHT,
            Self::Media => MEDIA_HEIGHT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AUDIO_HEIGHT, CHOOSER_HEIGHT, COMPACT_HEIGHT, ERROR_HEIGHT, MEDIA_HEIGHT, rendered_height,
        viewport_scale,
    };
    use crate::contract::CapturePopupLayout;

    #[test]
    fn rendered_heights_include_fractional_text_scale_and_monitor_limit() {
        assert_eq!(rendered_height(CapturePopupLayout::Compact, 1.0, 1000), 210);
        assert_eq!(
            rendered_height(CapturePopupLayout::Compact, 1.25, 1000),
            263
        );
        assert_eq!(rendered_height(CapturePopupLayout::Compact, 2.5, 2000), 525);
        assert_eq!(
            rendered_height(CapturePopupLayout::Chooser, 1.25, 1000),
            575
        );
        assert_eq!(rendered_height(CapturePopupLayout::Media, 2.5, 900), 900);
    }

    #[test]
    fn viewport_measurement_yields_bounded_finite_scale() {
        assert_eq!(viewport_scale(640, 512.0), Some(1.25));
        assert_eq!(viewport_scale(1280, 512.0), Some(2.5));
        assert_eq!(viewport_scale(640, 640.0), Some(1.0));
        for width in [
            0.0,
            -1.0,
            0.5,
            40.0,
            2000.0,
            16385.0,
            f64::INFINITY,
            f64::NAN,
        ] {
            assert_eq!(viewport_scale(640, width), None);
        }
    }

    #[test]
    fn semantic_layouts_map_to_bounded_heights() {
        assert_eq!(COMPACT_HEIGHT, 210.0);
        assert_eq!(CapturePopupLayout::Compact.height(), COMPACT_HEIGHT);
        assert_eq!(CapturePopupLayout::Audio.height(), AUDIO_HEIGHT);
        assert_eq!(CapturePopupLayout::Error.height(), ERROR_HEIGHT);
        assert_eq!(CapturePopupLayout::Chooser.height(), CHOOSER_HEIGHT);
        assert_eq!(CapturePopupLayout::Media.height(), MEDIA_HEIGHT);
        assert!(COMPACT_HEIGHT < AUDIO_HEIGHT);
        assert!(COMPACT_HEIGHT < ERROR_HEIGHT);
        assert!(ERROR_HEIGHT < CHOOSER_HEIGHT);
        assert!(CHOOSER_HEIGHT < MEDIA_HEIGHT);
        assert!(MEDIA_HEIGHT <= 600.0);
    }
}
