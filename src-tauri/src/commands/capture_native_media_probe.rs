//! Fixed synthetic media recipes. Test-only, not a Tauri command or provider API.

use super::*;
use crate::contract::MediaKind;

fn stage_fixture(
    store: &mut MediaStore,
    session: crate::contract::CaptureSessionId,
    kind: MediaKind,
) -> StagedMedia {
    match kind {
        MediaKind::Image => images::stage_clipboard_image(
            store,
            session,
            crate::platform::clipboard::ClipboardImage {
                width: 2,
                height: 1,
                rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
            },
        )
        .unwrap(),
        MediaKind::Audio => {
            let (bytes, duration) = audio::encode_mono_pcm_wav(&[0.25; 1_600], 16_000, 1).unwrap();
            store.stage_audio_wav(session, &bytes, duration).unwrap()
        }
    }
}

#[test]
fn synthetic_native_media_are_valid_png_and_pcm_wav() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = MediaStore::open(directory.path()).unwrap();
    let session = crate::contract::CaptureSessionId::new();
    let image = stage_fixture(&mut store, session, MediaKind::Image);
    let (bytes, _) = store.staged_preview(image.staged_media_id).unwrap();
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut pixels).unwrap();
    assert_eq!((frame.width, frame.height), (2, 1));
    assert_eq!(
        &pixels[..frame.buffer_size()],
        &[255, 0, 0, 255, 0, 255, 0, 255]
    );
    let audio = stage_fixture(&mut store, session, MediaKind::Audio);
    let (bytes, _) = store.staged_preview(audio.staged_media_id).unwrap();
    let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(reader.spec().sample_rate, 16_000);
    assert_eq!(reader.spec().channels, 1);
    assert_eq!(reader.spec().bits_per_sample, 16);
    assert_eq!(reader.spec().sample_format, hound::SampleFormat::Int);
    assert_eq!(reader.duration(), 1_600);
    assert_eq!(audio.duration_ms, Some(100));
    assert!(
        reader
            .samples::<i16>()
            .all(|sample| sample.unwrap() == 8_192)
    );
}
