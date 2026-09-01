use std::collections::HashSet;

use rusqlite::Connection;

use crate::{
    contract::{MediaId, MediaKind, MediaMimeType},
    storage::StorageError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredMediaAsset {
    pub(crate) id: MediaId,
    pub(crate) kind: MediaKind,
    pub(crate) relative_path: String,
    pub(crate) mime_type: MediaMimeType,
    pub(crate) duration_ms: Option<u64>,
}

pub(crate) struct MediaAssetRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> MediaAssetRepository<'connection> {
    pub(crate) fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub(crate) fn referenced_relative_paths(&self) -> Result<HashSet<String>, StorageError> {
        let mut statement = self
            .connection
            .prepare("SELECT relative_path FROM media_assets")?;
        let paths = statement
            .query_map([], |row| row.get(0))?
            .collect::<Result<HashSet<String>, _>>()?;
        Ok(paths)
    }

    pub(crate) fn find(&self, media_id: MediaId) -> Result<Option<StoredMediaAsset>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT id, kind, relative_path, mime_type, duration_ms
             FROM media_assets WHERE id = ?1",
        )?;
        let result = statement.query_row([media_id.to_string()], |row| {
            let kind = match row.get::<_, String>(1)?.as_str() {
                "image" => MediaKind::Image,
                "audio" => MediaKind::Audio,
                value => return Err(invalid_value(1, value)),
            };
            let mime_type = match row.get::<_, String>(3)?.as_str() {
                "image/png" => MediaMimeType::ImagePng,
                "audio/wav" => MediaMimeType::AudioWav,
                value => return Err(invalid_value(3, value)),
            };
            let duration_ms = row
                .get::<_, Option<i64>>(4)?
                .map(|value| {
                    u64::try_from(value)
                        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(4, value))
                })
                .transpose()?;
            Ok(StoredMediaAsset {
                id: row.get::<_, String>(0)?.parse().map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
                kind,
                relative_path: row.get(2)?,
                mime_type,
                duration_ms,
            })
        });
        match result {
            Ok(asset) => Ok(Some(asset)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

fn invalid_value(column: usize, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        rusqlite::types::Type::Text,
        format!("invalid stored media value: {value}").into(),
    )
}

#[cfg(test)]
mod tests {
    use crate::storage::{Database, media_assets::MediaAssetRepository};

    #[test]
    fn reads_only_durable_media_references_for_startup_reconciliation() {
        let database = Database::open_in_memory().unwrap();
        database
            .connection()
            .execute_batch(
                "INSERT INTO contexts (id, kind, name, created_at, updated_at)
                 VALUES ('context-1', 'standalone', 'Notes', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                 INSERT INTO captures (
                     id, session_id, context_id, kind, text_body, caption, caption_source,
                     branch_name, source_app, source_window_title, captured_at, updated_at
                 ) VALUES (
                     'capture-1', 'session-1', 'context-1', 'image', NULL, NULL, NULL,
                     NULL, NULL, NULL, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
                 );
                 INSERT INTO media_assets (
                     id, capture_id, kind, relative_path, mime_type, byte_size, checksum,
                     duration_ms, width_px, height_px, created_at
                 ) VALUES (
                     'media-1', 'capture-1', 'image', 'images/capture-1.png', 'image/png', 1,
                     'checksum', NULL, 1, 1, '2026-01-01T00:00:00Z'
                 );",
            )
            .unwrap();

        let paths = MediaAssetRepository::new(database.connection())
            .referenced_relative_paths()
            .unwrap();

        assert_eq!(paths.len(), 1);
        assert!(paths.contains("images/capture-1.png"));
    }
}
