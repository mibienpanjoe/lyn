use std::{collections::BTreeMap, sync::Mutex};

use tauri::State;

use crate::{
    contract::{
        AudioPlaybackResult, CaptureDetail, CaptureSummary, GetCaptureInput, ListCapturesInput,
        MediaByIdInput, MediaKind, OpenMediaResult, Page,
    },
    error::{AppError, CommandResult, ErrorCode, ErrorDetailKey, ErrorDetailValue, ErrorDetails},
    library::service::{LibraryError, LibraryService},
    media::staging::MediaStore,
    platform::{
        media_open::{MediaOpenPlatform, NativeMediaOpenPlatform},
        playback::{AudioPlaybackPlatform, NativeAudioPlaybackPlatform},
    },
    storage::{Database, media_assets::MediaAssetRepository},
};

const MAX_BRANCH_NAME_CHARS: usize = 255;
const MAX_CAPTURE_KINDS: usize = 3;

#[tauri::command]
pub(crate) fn list_captures(
    input: serde_json::Value,
    database: State<'_, Mutex<Database>>,
    media_store: State<'_, Mutex<MediaStore>>,
) -> CommandResult<Page<CaptureSummary>> {
    list_captures_value(input, database.inner(), media_store.inner())
}

fn list_captures_value(
    input: serde_json::Value,
    database: &Mutex<Database>,
    media_store: &Mutex<MediaStore>,
) -> CommandResult<Page<CaptureSummary>> {
    let Ok(input) = serde_json::from_value::<ListCapturesInput>(input) else {
        return CommandResult::failure(validation_error("input"));
    };
    if let Err(field) = validate_list_input(&input) {
        return CommandResult::failure(validation_error(field));
    }
    let Ok(database) = database.lock() else {
        return CommandResult::failure(internal_error());
    };
    let Ok(media_store) = media_store.lock() else {
        return CommandResult::failure(internal_error());
    };
    match LibraryService::new(database.connection(), &media_store).list(&input) {
        Ok(page) => CommandResult::success(page),
        Err(error) => CommandResult::failure(library_error(error)),
    }
}

#[tauri::command]
pub(crate) fn get_capture(
    input: serde_json::Value,
    database: State<'_, Mutex<Database>>,
    media_store: State<'_, Mutex<MediaStore>>,
) -> CommandResult<CaptureDetail> {
    let Ok(input) = serde_json::from_value::<GetCaptureInput>(input) else {
        return CommandResult::failure(validation_error("input"));
    };
    let Ok(database) = database.lock() else {
        return CommandResult::failure(internal_error());
    };
    let Ok(media_store) = media_store.lock() else {
        return CommandResult::failure(internal_error());
    };
    match LibraryService::new(database.connection(), &media_store).get(input.capture_id) {
        Ok(capture) => CommandResult::success(capture),
        Err(error) => CommandResult::failure(library_error(error)),
    }
}

#[tauri::command]
pub(crate) fn play_media(
    input: serde_json::Value,
    database: State<'_, Mutex<Database>>,
    media_store: State<'_, Mutex<MediaStore>>,
    playback: State<'_, Mutex<NativeAudioPlaybackPlatform>>,
) -> CommandResult<AudioPlaybackResult> {
    play_media_value(
        input,
        database.inner(),
        media_store.inner(),
        playback.inner(),
    )
}

fn play_media_value<Playback: AudioPlaybackPlatform>(
    input: serde_json::Value,
    database: &Mutex<Database>,
    media_store: &Mutex<MediaStore>,
    playback: &Mutex<Playback>,
) -> CommandResult<AudioPlaybackResult> {
    let asset = match resolve_media(input, database) {
        Ok(asset) if asset.kind == MediaKind::Audio => asset,
        Ok(_) | Err(ResolveMediaError::NotFound) => {
            return CommandResult::failure(media_not_found_error());
        }
        Err(ResolveMediaError::Validation) => {
            return CommandResult::failure(validation_error("input"));
        }
        Err(ResolveMediaError::Storage) => {
            return CommandResult::failure(storage_unavailable_error());
        }
    };
    let bytes = match media_store
        .lock()
        .ok()
        .and_then(|store| store.read_final(&asset.relative_path).ok())
    {
        Some(bytes) => bytes,
        None => return CommandResult::failure(media_not_found_error()),
    };
    if playback
        .lock()
        .ok()
        .and_then(|mut playback| playback.play_wav(&asset.id.to_string(), bytes).ok())
        .is_none()
    {
        return CommandResult::failure(audio_playback_error());
    }
    CommandResult::success(AudioPlaybackResult {
        playing: true,
        duration_ms: asset.duration_ms,
    })
}

#[tauri::command]
pub(crate) fn open_media_external(
    input: serde_json::Value,
    database: State<'_, Mutex<Database>>,
    media_store: State<'_, Mutex<MediaStore>>,
    platform: State<'_, NativeMediaOpenPlatform>,
) -> CommandResult<OpenMediaResult> {
    open_media_value(
        input,
        database.inner(),
        media_store.inner(),
        platform.inner(),
    )
}

fn open_media_value<Platform: MediaOpenPlatform>(
    input: serde_json::Value,
    database: &Mutex<Database>,
    media_store: &Mutex<MediaStore>,
    platform: &Platform,
) -> CommandResult<OpenMediaResult> {
    let asset = match resolve_media(input, database) {
        Ok(asset) => asset,
        Err(ResolveMediaError::Validation) => {
            return CommandResult::failure(validation_error("input"));
        }
        Err(ResolveMediaError::NotFound) => return CommandResult::failure(media_not_found_error()),
        Err(ResolveMediaError::Storage) => {
            return CommandResult::failure(storage_unavailable_error());
        }
    };
    let path = match media_store
        .lock()
        .ok()
        .and_then(|store| store.final_path_for_external(&asset.relative_path).ok())
    {
        Some(path) => path,
        None => return CommandResult::failure(media_not_found_error()),
    };
    match platform.open(&path) {
        Ok(()) => CommandResult::success(OpenMediaResult { opened: true }),
        Err(_) => CommandResult::failure(permission_error()),
    }
}

enum ResolveMediaError {
    Validation,
    NotFound,
    Storage,
}

fn resolve_media(
    input: serde_json::Value,
    database: &Mutex<Database>,
) -> Result<crate::storage::media_assets::StoredMediaAsset, ResolveMediaError> {
    let input = serde_json::from_value::<MediaByIdInput>(input)
        .map_err(|_| ResolveMediaError::Validation)?;
    let database = database.lock().map_err(|_| ResolveMediaError::Storage)?;
    MediaAssetRepository::new(database.connection())
        .find(input.media_id)
        .map_err(|_| ResolveMediaError::Storage)?
        .ok_or(ResolveMediaError::NotFound)
}

fn validate_list_input(input: &ListCapturesInput) -> Result<(), &'static str> {
    if !(1..=100).contains(&input.limit) {
        return Err("limit");
    }
    if let Some(branch) = input.branch_name.as_deref()
        && (branch.is_empty()
            || branch.trim() != branch
            || branch.chars().count() > MAX_BRANCH_NAME_CHARS
            || branch.chars().any(char::is_control))
    {
        return Err("branchName");
    }
    if input.capture_kinds.len() > MAX_CAPTURE_KINDS
        || input
            .capture_kinds
            .iter()
            .enumerate()
            .any(|(index, kind)| input.capture_kinds[index + 1..].contains(kind))
    {
        return Err("captureKinds");
    }
    if input
        .captured_from
        .as_ref()
        .zip(input.captured_to.as_ref())
        .is_some_and(|(from, to)| from > to)
    {
        return Err("capturedFrom");
    }
    Ok(())
}

fn library_error(error: LibraryError) -> AppError {
    match error {
        LibraryError::InvalidCursor => validation_error("cursor"),
        LibraryError::ContextNotFound => simple_error(
            ErrorCode::ContextNotFound,
            "The Library context no longer exists",
            false,
        ),
        LibraryError::CaptureNotFound => simple_error(
            ErrorCode::CaptureNotFound,
            "The capture no longer exists",
            false,
        ),
        LibraryError::Storage(_) => storage_unavailable_error(),
    }
}

fn validation_error(field: &str) -> AppError {
    AppError {
        code: ErrorCode::ValidationError,
        message: "The Library request is invalid".to_owned(),
        retryable: false,
        details: ErrorDetails(BTreeMap::from([(
            ErrorDetailKey::Field,
            ErrorDetailValue::String(field.to_owned()),
        )])),
    }
}

fn storage_unavailable_error() -> AppError {
    simple_error(
        ErrorCode::StorageUnavailable,
        "The Library is temporarily unavailable",
        true,
    )
}

fn media_not_found_error() -> AppError {
    simple_error(
        ErrorCode::MediaNotFound,
        "This media file is unavailable",
        false,
    )
}

fn audio_playback_error() -> AppError {
    simple_error(
        ErrorCode::AudioPlaybackFailed,
        "The audio could not be played",
        true,
    )
}

fn permission_error() -> AppError {
    simple_error(
        ErrorCode::PermissionDenied,
        "The media could not be opened",
        true,
    )
}

fn internal_error() -> AppError {
    simple_error(ErrorCode::InternalError, "The Library is unavailable", true)
}

fn simple_error(code: ErrorCode, message: &str, retryable: bool) -> AppError {
    AppError {
        code,
        message: message.to_owned(),
        retryable,
        details: ErrorDetails::default(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::Mutex,
    };

    use tempfile::tempdir;

    use crate::{
        contract::{CaptureSessionId, MediaKind},
        error::{CommandResult, ErrorCode},
        media::staging::MediaStore,
        platform::{
            media_open::{MediaOpenError, MediaOpenPlatform},
            playback::{AudioPlaybackError, AudioPlaybackPlatform},
        },
        storage::Database,
    };

    use super::{list_captures_value, open_media_value, play_media_value};

    #[derive(Default)]
    struct FakePlayback {
        target: Option<String>,
        bytes: Vec<u8>,
    }

    impl AudioPlaybackPlatform for FakePlayback {
        fn play_wav(&mut self, target_id: &str, bytes: Vec<u8>) -> Result<(), AudioPlaybackError> {
            self.target = Some(target_id.to_owned());
            self.bytes = bytes;
            Ok(())
        }

        fn stop(&mut self, _target_id: &str) -> Result<(), AudioPlaybackError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeOpen {
        path: Mutex<Option<PathBuf>>,
    }

    impl MediaOpenPlatform for FakeOpen {
        fn open(&self, path: &Path) -> Result<(), MediaOpenError> {
            *self.path.lock().unwrap() = Some(path.to_owned());
            Ok(())
        }
    }

    fn media_fixture() -> (Mutex<Database>, Mutex<MediaStore>, String) {
        let database = Database::open_in_memory().unwrap();
        let directory = tempdir().unwrap().keep();
        let mut media_store = MediaStore::open(&directory).unwrap();
        let context_id = "11111111-1111-4111-8111-111111111111";
        let capture_id = "22222222-2222-4222-8222-222222222222";
        let staged = media_store
            .stage_audio_wav(CaptureSessionId::new(), b"wav bytes", 1200)
            .unwrap();
        let finalized = media_store
            .finalize(
                staged.staged_media_id,
                capture_id.parse().unwrap(),
                MediaKind::Audio,
            )
            .unwrap();
        database.connection().execute_batch(&format!(
            "INSERT INTO contexts (id, kind, name, created_at, updated_at)
             VALUES ('{context_id}', 'project', 'Lyn', '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z');
             INSERT INTO captures (id, session_id, context_id, kind, text_body, caption,
               caption_source, branch_name, source_app, source_window_title, captured_at, updated_at)
             VALUES ('{capture_id}', '33333333-3333-4333-8333-333333333333', '{context_id}',
               'audio', NULL, NULL, NULL, 'main', NULL, NULL,
               '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z');"
        )).unwrap();
        database
            .connection()
            .execute(
                "INSERT INTO media_assets (id, capture_id, kind, relative_path, mime_type,
             byte_size, checksum, duration_ms, width_px, height_px, created_at)
             VALUES (?1, ?2, 'audio', ?3, 'audio/wav', ?4, ?5, ?6, NULL, NULL,
             '2026-09-01T00:00:00Z')",
                (
                    finalized.media_id.to_string(),
                    capture_id,
                    &finalized.relative_path,
                    finalized.byte_size as i64,
                    &finalized.checksum,
                    finalized.duration_ms.unwrap() as i64,
                ),
            )
            .unwrap();
        (
            Mutex::new(database),
            Mutex::new(media_store),
            finalized.media_id.to_string(),
        )
    }

    #[test]
    fn rejects_unbounded_and_duplicate_list_filters() {
        let database = Mutex::new(Database::open_in_memory().unwrap());
        let media = Mutex::new(MediaStore::open(tempdir().unwrap().path()).unwrap());
        for input in [
            serde_json::json!({"scope":{"kind":"all"},"branchName":null,"captureKinds":[],"capturedFrom":null,"capturedTo":null,"cursor":null,"limit":0}),
            serde_json::json!({"scope":{"kind":"all"},"branchName":null,"captureKinds":["text","text"],"capturedFrom":null,"capturedTo":null,"cursor":null,"limit":50}),
        ] {
            let CommandResult::Failure { error, .. } =
                list_captures_value(input, &database, &media)
            else {
                panic!("invalid request accepted")
            };
            assert_eq!(error.code, ErrorCode::ValidationError);
        }
    }

    #[test]
    fn committed_audio_play_and_open_resolve_only_an_opaque_media_id() {
        let (database, media, media_id) = media_fixture();
        let playback = Mutex::new(FakePlayback::default());
        let opened = FakeOpen::default();
        let input = serde_json::json!({"mediaId": media_id});

        let played = play_media_value(input.clone(), &database, &media, &playback);
        let open_result = open_media_value(input, &database, &media, &opened);

        assert!(matches!(played, CommandResult::Success { .. }));
        assert!(matches!(open_result, CommandResult::Success { .. }));
        assert_eq!(playback.lock().unwrap().bytes, b"wav bytes");
        assert!(opened.path.lock().unwrap().as_ref().unwrap().is_file());
    }
}
