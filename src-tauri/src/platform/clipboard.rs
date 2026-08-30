//! Explicit clipboard image access behind a narrow native port.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClipboardImage {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClipboardError {
    UnsupportedContent,
    Unavailable,
}

pub(crate) trait ClipboardImagePlatform {
    fn read_image(&mut self) -> Result<ClipboardImage, ClipboardError>;
}

pub(crate) struct NativeClipboardPlatform;

impl ClipboardImagePlatform for NativeClipboardPlatform {
    fn read_image(&mut self) -> Result<ClipboardImage, ClipboardError> {
        let mut clipboard = arboard::Clipboard::new().map_err(|_| ClipboardError::Unavailable)?;
        let image = clipboard
            .get_image()
            .map_err(|_| ClipboardError::UnsupportedContent)?;
        let width = u32::try_from(image.width).map_err(|_| ClipboardError::UnsupportedContent)?;
        let height = u32::try_from(image.height).map_err(|_| ClipboardError::UnsupportedContent)?;
        Ok(ClipboardImage {
            width,
            height,
            rgba: image.bytes.into_owned(),
        })
    }
}
