#![cfg_attr(not(test), allow(dead_code))]

use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::contract::{
    CaptureId, CaptureSessionId, MediaId, MediaKind, MediaMimeType, StagedMedia, StagedMediaId,
};

const MEDIA_DIRECTORY: &str = "media";

#[derive(Debug)]
pub(crate) enum StagingError {
    Io(std::io::Error),
    UnknownStagedMedia,
    KindMismatch,
    InvalidMedia,
    PathOutsideRoot,
    FinalPathExists,
}

impl fmt::Display for StagingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(_) => formatter.write_str("media storage is unavailable"),
            Self::UnknownStagedMedia => formatter.write_str("staged media is unavailable"),
            Self::KindMismatch => formatter.write_str("media kind does not match staged media"),
            Self::InvalidMedia => formatter.write_str("staged media validation failed"),
            Self::PathOutsideRoot => formatter.write_str("media path is outside Lyn storage"),
            Self::FinalPathExists => formatter.write_str("media already exists for this capture"),
        }
    }
}

impl Error for StagingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for StagingError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FinalizedMedia {
    pub(crate) media_id: MediaId,
    pub(crate) kind: MediaKind,
    pub(crate) mime_type: MediaMimeType,
    pub(crate) relative_path: String,
    pub(crate) byte_size: u64,
    pub(crate) checksum: String,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) width_px: Option<u32>,
    pub(crate) height_px: Option<u32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ReconciliationReport {
    pub(crate) removed_orphans: usize,
    pub(crate) removed_staging_files: usize,
}

#[derive(Debug)]
struct StagedAsset {
    session_id: CaptureSessionId,
    kind: MediaKind,
    mime_type: MediaMimeType,
    path: PathBuf,
    byte_size: u64,
    checksum: String,
    duration_ms: Option<u64>,
    width_px: Option<u32>,
    height_px: Option<u32>,
}

#[derive(Debug)]
pub(crate) struct MediaStore {
    root: PathBuf,
    staged: HashMap<StagedMediaId, StagedAsset>,
}

impl MediaStore {
    pub(crate) fn open(root: impl AsRef<Path>) -> Result<Self, StagingError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join(MEDIA_DIRECTORY).join("staging"))?;
        fs::create_dir_all(root.join(MEDIA_DIRECTORY).join("images"))?;
        fs::create_dir_all(root.join(MEDIA_DIRECTORY).join("audio"))?;
        Ok(Self {
            root,
            staged: HashMap::new(),
        })
    }

    pub(crate) fn stage_bytes(
        &mut self,
        session_id: CaptureSessionId,
        kind: MediaKind,
        mime_type: MediaMimeType,
        bytes: &[u8],
    ) -> Result<StagedMedia, StagingError> {
        if extension(kind, mime_type).is_none() || bytes.is_empty() {
            return Err(StagingError::InvalidMedia);
        }

        let staged_media_id = StagedMediaId::new();
        let path = self.staging_path(session_id, staged_media_id, kind, mime_type)?;
        let parent = path.parent().ok_or(StagingError::PathOutsideRoot)?;
        fs::create_dir_all(parent)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(bytes)?;
        file.sync_all()?;

        let asset = StagedAsset {
            session_id,
            kind,
            mime_type,
            path,
            byte_size: bytes.len() as u64,
            checksum: checksum(bytes),
            duration_ms: None,
            width_px: None,
            height_px: None,
        };
        self.staged.insert(staged_media_id, asset);
        Ok(StagedMedia {
            staged_media_id,
            kind,
            preview_uri: format!("lyn-media://staged/{staged_media_id}"),
            mime_type,
            byte_size: bytes.len() as u64,
            duration_ms: None,
            width_px: None,
            height_px: None,
        })
    }

    pub(crate) fn stage_image_png(
        &mut self,
        session_id: CaptureSessionId,
        bytes: &[u8],
        width_px: u32,
        height_px: u32,
    ) -> Result<StagedMedia, StagingError> {
        if width_px == 0 || height_px == 0 {
            return Err(StagingError::InvalidMedia);
        }
        let mut staged =
            self.stage_bytes(session_id, MediaKind::Image, MediaMimeType::ImagePng, bytes)?;
        let asset = self
            .staged
            .get_mut(&staged.staged_media_id)
            .ok_or(StagingError::UnknownStagedMedia)?;
        asset.width_px = Some(width_px);
        asset.height_px = Some(height_px);
        staged.width_px = Some(width_px);
        staged.height_px = Some(height_px);
        Ok(staged)
    }

    pub(crate) fn stage_audio_wav(
        &mut self,
        session_id: CaptureSessionId,
        bytes: &[u8],
        duration_ms: u64,
    ) -> Result<StagedMedia, StagingError> {
        if duration_ms == 0 {
            return Err(StagingError::InvalidMedia);
        }
        let mut staged =
            self.stage_bytes(session_id, MediaKind::Audio, MediaMimeType::AudioWav, bytes)?;
        let asset = self
            .staged
            .get_mut(&staged.staged_media_id)
            .ok_or(StagingError::UnknownStagedMedia)?;
        asset.duration_ms = Some(duration_ms);
        staged.duration_ms = Some(duration_ms);
        Ok(staged)
    }

    pub(crate) fn finalize(
        &mut self,
        staged_media_id: StagedMediaId,
        capture_id: CaptureId,
        kind: MediaKind,
    ) -> Result<FinalizedMedia, StagingError> {
        let asset = self
            .staged
            .get(&staged_media_id)
            .ok_or(StagingError::UnknownStagedMedia)?;
        if asset.kind != kind {
            return Err(StagingError::KindMismatch);
        }
        validate_staged_asset(asset)?;

        let relative_path = final_relative_path(capture_id, asset.kind, asset.mime_type)
            .ok_or(StagingError::InvalidMedia)?;
        let destination = self.final_path(&relative_path)?;
        if destination.exists() {
            return Err(StagingError::FinalPathExists);
        }
        let parent = destination.parent().ok_or(StagingError::PathOutsideRoot)?;
        fs::create_dir_all(parent)?;
        fs::rename(&asset.path, &destination)?;

        let asset = self
            .staged
            .remove(&staged_media_id)
            .ok_or(StagingError::UnknownStagedMedia)?;
        Ok(FinalizedMedia {
            media_id: MediaId::new(),
            kind: asset.kind,
            mime_type: asset.mime_type,
            relative_path,
            byte_size: asset.byte_size,
            checksum: asset.checksum,
            duration_ms: asset.duration_ms,
            width_px: asset.width_px,
            height_px: asset.height_px,
        })
    }

    pub(crate) fn remove_final(&self, relative_path: &str) -> Result<(), StagingError> {
        let path = self.final_path(relative_path)?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    pub(crate) fn restore_staged_after_failed_save(
        &mut self,
        session_id: CaptureSessionId,
        staged_media_id: StagedMediaId,
        finalized: &FinalizedMedia,
    ) -> Result<(), StagingError> {
        let source = self.final_path(&finalized.relative_path)?;
        let destination = self.staging_path(
            session_id,
            staged_media_id,
            finalized.kind,
            finalized.mime_type,
        )?;
        let parent = destination.parent().ok_or(StagingError::PathOutsideRoot)?;
        fs::create_dir_all(parent)?;
        fs::rename(source, &destination)?;
        self.staged.insert(
            staged_media_id,
            StagedAsset {
                session_id,
                kind: finalized.kind,
                mime_type: finalized.mime_type,
                path: destination,
                byte_size: finalized.byte_size,
                checksum: finalized.checksum.clone(),
                duration_ms: finalized.duration_ms,
                width_px: finalized.width_px,
                height_px: finalized.height_px,
            },
        );
        Ok(())
    }

    pub(crate) fn cleanup_session(
        &mut self,
        session_id: CaptureSessionId,
    ) -> Result<(), StagingError> {
        let directory = self.staging_directory(session_id)?;
        if directory.exists() {
            fs::remove_dir_all(&directory)?;
        }
        self.staged
            .retain(|_, asset| asset.session_id != session_id);
        Ok(())
    }

    pub(crate) fn discard_staged(
        &mut self,
        session_id: CaptureSessionId,
        staged_media_id: StagedMediaId,
    ) -> Result<(), StagingError> {
        let asset = self
            .staged
            .get(&staged_media_id)
            .filter(|asset| asset.session_id == session_id)
            .ok_or(StagingError::UnknownStagedMedia)?;
        fs::remove_file(&asset.path)?;
        self.staged.remove(&staged_media_id);
        Ok(())
    }

    pub(crate) fn staged_preview(
        &self,
        staged_media_id: StagedMediaId,
    ) -> Result<(Vec<u8>, MediaMimeType), StagingError> {
        let asset = self
            .staged
            .get(&staged_media_id)
            .ok_or(StagingError::UnknownStagedMedia)?;
        Ok((read_bytes(&asset.path)?, asset.mime_type))
    }

    pub(crate) fn reconcile(
        &mut self,
        referenced_paths: &HashSet<String>,
    ) -> Result<ReconciliationReport, StagingError> {
        let mut report = ReconciliationReport::default();
        let staging_root = self.root.join(MEDIA_DIRECTORY).join("staging");
        for entry in fs::read_dir(&staging_root)? {
            let entry = entry?;
            report.removed_staging_files += remove_tree(&entry.path())?;
        }
        self.staged.clear();

        for directory in ["images", "audio"] {
            let root = self.root.join(MEDIA_DIRECTORY).join(directory);
            for entry in fs::read_dir(root)? {
                let entry = entry?;
                if !entry.file_type()?.is_file() {
                    continue;
                }
                let relative = format!("{directory}/{}", entry.file_name().to_string_lossy());
                if !referenced_paths.contains(&relative) {
                    fs::remove_file(entry.path())?;
                    report.removed_orphans += 1;
                }
            }
        }
        Ok(report)
    }

    #[cfg(test)]
    pub(crate) fn staged_bytes(
        &self,
        staged_media_id: StagedMediaId,
    ) -> Result<Vec<u8>, StagingError> {
        self.staged_preview(staged_media_id)
            .map(|preview| preview.0)
    }

    #[cfg(test)]
    pub(crate) fn read_final(&self, relative_path: &str) -> Result<Vec<u8>, StagingError> {
        read_bytes(&self.final_path(relative_path)?)
    }

    fn staging_directory(&self, session_id: CaptureSessionId) -> Result<PathBuf, StagingError> {
        let path = self
            .root
            .join(MEDIA_DIRECTORY)
            .join("staging")
            .join(session_id.to_string());
        self.ensure_contained(&path)
    }

    fn staging_path(
        &self,
        session_id: CaptureSessionId,
        staged_media_id: StagedMediaId,
        kind: MediaKind,
        mime_type: MediaMimeType,
    ) -> Result<PathBuf, StagingError> {
        let extension = extension(kind, mime_type).ok_or(StagingError::InvalidMedia)?;
        let path = self
            .staging_directory(session_id)?
            .join(format!("{staged_media_id}.{extension}"));
        self.ensure_contained(&path)
    }

    fn final_path(&self, relative_path: &str) -> Result<PathBuf, StagingError> {
        let relative = Path::new(relative_path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(StagingError::PathOutsideRoot);
        }
        self.ensure_contained(&self.root.join(MEDIA_DIRECTORY).join(relative))
    }

    fn ensure_contained(&self, path: &Path) -> Result<PathBuf, StagingError> {
        let media_root = self.root.join(MEDIA_DIRECTORY);
        if path.starts_with(&media_root) {
            Ok(path.to_path_buf())
        } else {
            Err(StagingError::PathOutsideRoot)
        }
    }
}

fn extension(kind: MediaKind, mime_type: MediaMimeType) -> Option<&'static str> {
    match (kind, mime_type) {
        (MediaKind::Image, MediaMimeType::ImagePng) => Some("png"),
        (MediaKind::Audio, MediaMimeType::AudioWav) => Some("wav"),
        _ => None,
    }
}

fn final_relative_path(
    capture_id: CaptureId,
    kind: MediaKind,
    mime_type: MediaMimeType,
) -> Option<String> {
    let extension = extension(kind, mime_type)?;
    let directory = match kind {
        MediaKind::Image => "images",
        MediaKind::Audio => "audio",
    };
    Some(format!("{directory}/{capture_id}.{extension}"))
}

fn validate_staged_asset(asset: &StagedAsset) -> Result<(), StagingError> {
    let metadata = fs::metadata(&asset.path)?;
    if !metadata.is_file() || metadata.len() != asset.byte_size {
        return Err(StagingError::InvalidMedia);
    }
    if checksum_reader(File::open(&asset.path)?)? != asset.checksum {
        return Err(StagingError::InvalidMedia);
    }
    Ok(())
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, StagingError> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn checksum(bytes: &[u8]) -> String {
    checksum_hex(Sha256::digest(bytes).as_slice())
}

fn checksum_reader(mut file: File) -> Result<String, StagingError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8_192];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    Ok(checksum_hex(digest.as_slice()))
}

fn checksum_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn remove_tree(path: &Path) -> Result<usize, StagingError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        let mut count = 0;
        for entry in fs::read_dir(path)? {
            count += remove_tree(&entry?.path())?;
        }
        fs::remove_dir(path)?;
        Ok(count)
    } else {
        fs::remove_file(path)?;
        Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use tempfile::tempdir;

    use crate::contract::{CaptureId, CaptureSessionId, MediaKind, MediaMimeType};

    use super::{MediaStore, StagingError};

    #[test]
    fn stages_opaque_media_inside_the_session_directory() {
        let directory = tempdir().unwrap();
        let mut store = MediaStore::open(directory.path()).unwrap();
        let session_id = CaptureSessionId::new();

        let staged = store
            .stage_bytes(
                session_id,
                MediaKind::Image,
                MediaMimeType::ImagePng,
                b"png bytes",
            )
            .unwrap();

        assert_eq!(staged.kind, MediaKind::Image);
        assert_eq!(
            staged.preview_uri,
            format!("lyn-media://staged/{}", staged.staged_media_id)
        );
        assert!(
            !serde_json::to_string(&staged)
                .unwrap()
                .contains(&directory.path().display().to_string())
        );
        assert_eq!(
            store.staged_bytes(staged.staged_media_id).unwrap(),
            b"png bytes"
        );
    }

    #[test]
    fn finalization_moves_one_complete_staged_file_to_a_capture_derived_path() {
        let directory = tempdir().unwrap();
        let mut store = MediaStore::open(directory.path()).unwrap();
        let staged = store
            .stage_bytes(
                CaptureSessionId::new(),
                MediaKind::Audio,
                MediaMimeType::AudioWav,
                b"wav bytes",
            )
            .unwrap();
        let capture_id = CaptureId::new();

        let finalized = store
            .finalize(staged.staged_media_id, capture_id, MediaKind::Audio)
            .unwrap();

        assert_eq!(finalized.relative_path, format!("audio/{capture_id}.wav"));
        assert_eq!(finalized.byte_size, 9);
        assert_eq!(
            store.read_final(&finalized.relative_path).unwrap(),
            b"wav bytes"
        );
        assert!(matches!(
            store.finalize(staged.staged_media_id, CaptureId::new(), MediaKind::Audio),
            Err(StagingError::UnknownStagedMedia)
        ));
    }

    #[test]
    fn session_cleanup_never_deletes_other_sessions_or_final_media() {
        let directory = tempdir().unwrap();
        let mut store = MediaStore::open(directory.path()).unwrap();
        let first_session = CaptureSessionId::new();
        let first = store
            .stage_bytes(
                first_session,
                MediaKind::Image,
                MediaMimeType::ImagePng,
                b"first",
            )
            .unwrap();
        let second = store
            .stage_bytes(
                CaptureSessionId::new(),
                MediaKind::Image,
                MediaMimeType::ImagePng,
                b"second",
            )
            .unwrap();
        let final_asset = store
            .finalize(first.staged_media_id, CaptureId::new(), MediaKind::Image)
            .unwrap();

        store.cleanup_session(first_session).unwrap();

        assert!(store.staged_bytes(second.staged_media_id).is_ok());
        assert!(store.read_final(&final_asset.relative_path).is_ok());
    }

    #[test]
    fn startup_reconciliation_removes_only_orphans_and_abandoned_staging() {
        let directory = tempdir().unwrap();
        let mut store = MediaStore::open(directory.path()).unwrap();
        let retained = store
            .stage_bytes(
                CaptureSessionId::new(),
                MediaKind::Image,
                MediaMimeType::ImagePng,
                b"retained",
            )
            .unwrap();
        let retained = store
            .finalize(retained.staged_media_id, CaptureId::new(), MediaKind::Image)
            .unwrap();
        let orphan = store
            .stage_bytes(
                CaptureSessionId::new(),
                MediaKind::Audio,
                MediaMimeType::AudioWav,
                b"orphan",
            )
            .unwrap();
        let orphan = store
            .finalize(orphan.staged_media_id, CaptureId::new(), MediaKind::Audio)
            .unwrap();
        let abandoned = store
            .stage_bytes(
                CaptureSessionId::new(),
                MediaKind::Image,
                MediaMimeType::ImagePng,
                b"abandoned",
            )
            .unwrap();
        let references = HashSet::from([retained.relative_path.clone()]);

        let report = store.reconcile(&references).unwrap();

        assert_eq!(report.removed_orphans, 1);
        assert_eq!(report.removed_staging_files, 1);
        assert!(store.read_final(&retained.relative_path).is_ok());
        assert!(store.read_final(&orphan.relative_path).is_err());
        assert!(matches!(
            store.staged_bytes(abandoned.staged_media_id),
            Err(StagingError::UnknownStagedMedia)
        ));
    }

    #[test]
    fn rejects_relative_paths_that_escape_lyn_media_storage() {
        let directory = tempdir().unwrap();
        let store = MediaStore::open(directory.path()).unwrap();

        assert!(matches!(
            store.read_final("../outside.png"),
            Err(StagingError::PathOutsideRoot)
        ));
        assert!(matches!(
            store.read_final("/tmp/outside.png"),
            Err(StagingError::PathOutsideRoot)
        ));
    }

    #[test]
    fn does_not_replace_existing_final_media_or_consume_the_second_stage() {
        let directory = tempdir().unwrap();
        let mut store = MediaStore::open(directory.path()).unwrap();
        let capture_id = CaptureId::new();
        let first = store
            .stage_bytes(
                CaptureSessionId::new(),
                MediaKind::Image,
                MediaMimeType::ImagePng,
                b"first",
            )
            .unwrap();
        store
            .finalize(first.staged_media_id, capture_id, MediaKind::Image)
            .unwrap();
        let second = store
            .stage_bytes(
                CaptureSessionId::new(),
                MediaKind::Image,
                MediaMimeType::ImagePng,
                b"second",
            )
            .unwrap();

        assert!(matches!(
            store.finalize(second.staged_media_id, capture_id, MediaKind::Image),
            Err(StagingError::FinalPathExists)
        ));
        assert_eq!(
            store.staged_bytes(second.staged_media_id).unwrap(),
            b"second"
        );
    }
}
