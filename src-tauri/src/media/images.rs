use png::{BitDepth, ColorType, Encoder};

use crate::{
    contract::{CaptureSessionId, StagedMedia},
    media::staging::{MediaStore, StagingError},
    platform::clipboard::ClipboardImage,
};

const MAX_IMAGE_PIXELS: u64 = 40_000_000;

pub(crate) fn stage_clipboard_image(
    store: &mut MediaStore,
    session_id: CaptureSessionId,
    image: ClipboardImage,
) -> Result<StagedMedia, StagingError> {
    let png = encode_png(&image)?;
    store.stage_image_png(session_id, &png, image.width, image.height)
}

fn encode_png(image: &ClipboardImage) -> Result<Vec<u8>, StagingError> {
    let pixels = u64::from(image.width) * u64::from(image.height);
    let expected_size = pixels.checked_mul(4).ok_or(StagingError::InvalidMedia)?;
    if image.width == 0
        || image.height == 0
        || pixels > MAX_IMAGE_PIXELS
        || image.rgba.len() as u64 != expected_size
    {
        return Err(StagingError::InvalidMedia);
    }
    let mut output = Vec::new();
    let mut encoder = Encoder::new(&mut output, image.width, image.height);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    {
        let mut writer = encoder
            .write_header()
            .map_err(|_| StagingError::InvalidMedia)?;
        writer
            .write_image_data(&image.rgba)
            .map_err(|_| StagingError::InvalidMedia)?;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use png::Decoder;
    use tempfile::tempdir;

    use crate::{
        contract::{CaptureSessionId, MediaKind, MediaMimeType},
        media::{
            images::stage_clipboard_image,
            staging::{MediaStore, StagingError},
        },
        platform::clipboard::ClipboardImage,
    };

    #[test]
    fn clipboard_rgba_becomes_a_valid_png_with_opaque_staging_metadata() {
        let directory = tempdir().unwrap();
        let mut store = MediaStore::open(directory.path()).unwrap();
        let staged = stage_clipboard_image(
            &mut store,
            CaptureSessionId::new(),
            ClipboardImage {
                width: 2,
                height: 1,
                rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
            },
        )
        .unwrap();

        assert_eq!(staged.kind, MediaKind::Image);
        assert_eq!(staged.mime_type, MediaMimeType::ImagePng);
        assert_eq!((staged.width_px, staged.height_px), (Some(2), Some(1)));
        let bytes = store.staged_bytes(staged.staged_media_id).unwrap();
        let decoder = Decoder::new(Cursor::new(bytes));
        let reader = decoder.read_info().unwrap();
        assert_eq!((reader.info().width, reader.info().height), (2, 1));
    }

    #[test]
    fn malformed_clipboard_rgba_is_rejected_before_staging() {
        let directory = tempdir().unwrap();
        let mut store = MediaStore::open(directory.path()).unwrap();

        assert!(matches!(
            stage_clipboard_image(
                &mut store,
                CaptureSessionId::new(),
                ClipboardImage {
                    width: 2,
                    height: 1,
                    rgba: vec![0; 7]
                },
            ),
            Err(StagingError::InvalidMedia)
        ));
    }
}
