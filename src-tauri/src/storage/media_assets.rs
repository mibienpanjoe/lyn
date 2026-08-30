use std::collections::HashSet;

use rusqlite::Connection;

use crate::storage::StorageError;

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
