use tauri::{LogicalSize, WebviewWindow};

use crate::contract::CapturePopupLayout;

const COMPACT_HEIGHT: f64 = 210.0;
const ERROR_HEIGHT: f64 = 280.0;
const CHOOSER_HEIGHT: f64 = 460.0;
const MEDIA_HEIGHT: f64 = 560.0;

pub(crate) fn resize_capture_popup(
    window: &WebviewWindow,
    layout: CapturePopupLayout,
) -> tauri::Result<()> {
    let scale_factor = window.scale_factor()?;
    let current_size = window.inner_size()?.to_logical::<f64>(scale_factor);
    window.set_size(LogicalSize::new(current_size.width, layout.height()))
}

impl CapturePopupLayout {
    fn height(self) -> f64 {
        match self {
            Self::Compact => COMPACT_HEIGHT,
            Self::Error => ERROR_HEIGHT,
            Self::Chooser => CHOOSER_HEIGHT,
            Self::Media => MEDIA_HEIGHT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CHOOSER_HEIGHT, COMPACT_HEIGHT, ERROR_HEIGHT, MEDIA_HEIGHT};
    use crate::contract::CapturePopupLayout;

    #[test]
    fn semantic_layouts_map_to_bounded_increasing_heights() {
        assert_eq!(COMPACT_HEIGHT, 210.0);
        assert_eq!(CapturePopupLayout::Compact.height(), COMPACT_HEIGHT);
        assert_eq!(CapturePopupLayout::Error.height(), ERROR_HEIGHT);
        assert_eq!(CapturePopupLayout::Chooser.height(), CHOOSER_HEIGHT);
        assert_eq!(CapturePopupLayout::Media.height(), MEDIA_HEIGHT);
        assert!(COMPACT_HEIGHT < ERROR_HEIGHT);
        assert!(ERROR_HEIGHT < CHOOSER_HEIGHT);
        assert!(CHOOSER_HEIGHT < MEDIA_HEIGHT);
        assert!(MEDIA_HEIGHT <= 600.0);
    }
}
