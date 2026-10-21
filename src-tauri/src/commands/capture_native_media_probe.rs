//! Fixed synthetic media recipes. Test-only, not a Tauri command or provider API.

use super::*;
use crate::context::{gnome_broker::NativeBroker, gnome_capture, gnome_process::ProcessInvocation};
use crate::contract::MediaKind;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

const CAPTION: &str = "  synthetic native media caption — keep spacing  ";

pub(crate) fn run(
    broker: Arc<NativeBroker>,
    window: u32,
    directory: PathBuf,
    kind: MediaKind,
    retire_generation: bool,
) -> Value {
    let invocation = ProcessInvocation::now().unwrap();
    let database = Mutex::new(Database::open_in_memory().unwrap());
    let service = Mutex::new(CaptureSessionService::default());
    let registry = Mutex::new(ContextSourceRegistry::default());
    let root = tempfile::tempdir().unwrap();
    let media = Mutex::new(MediaStore::open(root.path()).unwrap());
    let session = service.lock().unwrap().get_or_prepare();
    let staged = stage_fixture(&mut media.lock().unwrap(), session.session_id, kind);
    service
        .lock()
        .unwrap()
        .set_staged_media(session.session_id, staged.clone())
        .unwrap();
    let bytes = media
        .lock()
        .unwrap()
        .staged_bytes(staged.staged_media_id)
        .unwrap();
    let ticket = service
        .lock()
        .unwrap()
        .begin_automatic_query(session.session_id)
        .unwrap();
    let Some((resolution, source)) =
        gnome_capture::prepare(broker.clone(), window, invocation, &database)
    else {
        return json!({"status":"unavailable"});
    };
    let expected =
        crate::context::inspect_project_directory(&std::fs::canonicalize(directory).unwrap())
            .unwrap();
    let context = ContextRepository::new(database.lock().unwrap().connection())
        .find_project(expected.project_key.as_deref(), &expected.project_path)
        .unwrap()
        .unwrap();
    let ContextResolution::Resolved { candidate, .. } = &resolution else {
        panic!("native source unresolved")
    };
    assert_eq!(candidate.context.id, context.id);
    assert_eq!(candidate.branch_name, expected.branch_name);
    service
        .lock()
        .unwrap()
        .finish_automatic_query(ticket, resolution)
        .unwrap()
        .unwrap();
    registry
        .lock()
        .unwrap()
        .register_native(session.session_id, source);
    let before = service.lock().unwrap().active_session().unwrap();
    assert_eq!(before.staged_media, Some(staged.clone()));
    let repeated = service
        .lock()
        .unwrap()
        .prepare_invocation(Some(uuid::Uuid::new_v4()));
    assert!(!repeated.is_new());
    assert_eq!(repeated.session(), before);
    let input = json!({"sessionId":session.session_id,"stagedMediaId":staged.staged_media_id,"caption":CAPTION});
    let save = || match kind {
        MediaKind::Image => super::save_image_capture_with_registry_value(
            input.clone(),
            &database,
            &service,
            &media,
            &registry,
        ),
        MediaKind::Audio => super::save_audio_capture_with_registry_value(
            input.clone(),
            &database,
            &service,
            &media,
            false,
            &registry,
        ),
    };
    let mut expected_context = context.id;
    let mut expected_branch = expected.branch_name;
    let mut stale_save_refused = None;
    let mut late_automatic_refused = None;
    if retire_generation {
        let (old, new) = broker
            .reconnect_fixture_peer()
            .expect("fresh production-policy handshake required");
        assert_ne!(old, new);
        assert!(
            matches!(save(), CommandResult::Failure {error, ..} if error.code == ErrorCode::ContextSourceStale)
        );
        stale_save_refused = Some(true);
        assert_eq!(
            service.lock().unwrap().active_session(),
            Some(before.clone())
        );
        assert_eq!(
            media
                .lock()
                .unwrap()
                .staged_bytes(staged.staged_media_id)
                .unwrap(),
            bytes
        );
        assert_eq!(stored_counts(&database), (0, 0));
        // A genuinely pending, unused ticket: prepare a fresh native answer,
        // then commit a user choice before handing that answer to the session.
        let late_ticket = service
            .lock()
            .unwrap()
            .begin_automatic_query(session.session_id)
            .unwrap();
        let (late_resolution, late_source) =
            gnome_capture::prepare(broker, window, ProcessInvocation::now().unwrap(), &database)
                .expect("fresh native preparation after reconnect required");
        let corrected = ContextRepository::new(database.lock().unwrap().connection())
            .create_standalone("Synthetic corrected media")
            .unwrap();
        let selected = super::select_capture_context_source_with_registry_value(
            json!({"sessionId":session.session_id,"selection":{"kind":"saved_context","contextId":corrected.id}}),
            &database,
            &service,
            &registry,
        );
        let CommandResult::Success { data: selected, .. } = selected else {
            panic!("manual correction refused")
        };
        assert_eq!(selected.staged_media, Some(staged.clone()));
        assert!(
            service
                .lock()
                .unwrap()
                .finish_automatic_query(late_ticket, late_resolution)
                .unwrap()
                .is_none()
        );
        assert_eq!(service.lock().unwrap().active_session(), Some(selected));
        drop(late_source); // Never register an answer rejected by the session.
        expected_context = corrected.id;
        expected_branch = None;
        late_automatic_refused = Some(true);
    }
    assert_eq!(
        media
            .lock()
            .unwrap()
            .staged_bytes(staged.staged_media_id)
            .unwrap(),
        bytes
    );
    let CommandResult::Success { data: saved, .. } = save() else {
        panic!("native media save refused")
    };
    assert!(!saved.enrichment_scheduled);
    let duplicate_refused = matches!(save(), CommandResult::Failure {error, ..} if error.code == ErrorCode::StaleSession);
    let CommandResult::Success { data: detail, .. } = crate::commands::library::get_capture_value(
        json!({"captureId":saved.capture_id}),
        &database,
        &media,
    ) else {
        panic!("Library cannot read native media capture")
    };
    let summary = detail
        .media
        .as_ref()
        .expect("saved media absent from Library");
    assert_eq!(summary.kind, kind);
    assert_eq!(summary.duration_ms, staged.duration_ms);
    assert_eq!(summary.width_px, staged.width_px);
    assert_eq!(summary.height_px, staged.height_px);
    assert!(summary.available && summary.preview_uri.starts_with("lyn-media://"));
    assert_eq!(
        detail.caption_source,
        Some(crate::contract::CaptionSource::User)
    );
    assert!(service.lock().unwrap().active_session().is_none());
    assert!(
        media
            .lock()
            .unwrap()
            .staged_preview(staged.staged_media_id)
            .is_err()
    );
    let (path, checksum, byte_size): (String, String, i64) = database
        .lock()
        .unwrap()
        .connection()
        .query_row(
            "SELECT relative_path, checksum, byte_size FROM media_assets WHERE capture_id=?1",
            [saved.capture_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    use sha2::Digest;
    let expected_checksum = sha2::Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(checksum, expected_checksum);
    assert_eq!(byte_size, bytes.len() as i64);
    let (capture_count, asset_count) = stored_counts(&database);
    json!({"status":"saved", "mediaKind":kind,"generationRetired":retire_generation,
        "captureCount":capture_count,"assetCount":asset_count,
        "bytesMatch":media.lock().unwrap().read_final(&path).unwrap()==bytes,
        "captionMatches":detail.caption.as_deref()==Some(CAPTION),
        "contextMatches":detail.context.id==expected_context,"branchMatches":detail.branch_name==expected_branch,
        "duplicateRefused":duplicate_refused,"leaseReleased":registry.lock().unwrap().live_sources(Instant::now()).is_empty(),
        "stagingPreserved":true,"duplicateInvocationPreserved":true,
        "staleSaveRefused":stale_save_refused,"lateAutomaticRefused":late_automatic_refused})
}

fn stored_counts(database: &Mutex<Database>) -> (i64, i64) {
    database
        .lock()
        .unwrap()
        .connection()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM captures), (SELECT COUNT(*) FROM media_assets)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

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
