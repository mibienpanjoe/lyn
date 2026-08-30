use rusqlite::{Connection, TransactionBehavior, params};

use crate::{
    contract::{CaptureId, CaptureSessionId, ContextId, SaveCaptureResult, Timestamp},
    storage::StorageError,
};

pub(crate) struct CaptureRepository<'connection> {
    connection: &'connection mut Connection,
}

impl<'connection> CaptureRepository<'connection> {
    pub(crate) fn new(connection: &'connection mut Connection) -> Self {
        Self { connection }
    }

    pub(crate) fn save_text(
        &mut self,
        session_id: CaptureSessionId,
        context_id: ContextId,
        text_body: &str,
        branch_name: Option<&str>,
    ) -> Result<SaveCaptureResult, StorageError> {
        let capture_id = CaptureId::new();
        let captured_at = Timestamp::now_utc();
        let captured_at_text = captured_at.to_string();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO captures (
                id, session_id, context_id, kind, text_body, caption, caption_source,
                branch_name, source_app, source_window_title, captured_at, updated_at
             ) VALUES (?1, ?2, ?3, 'text', ?4, NULL, NULL, ?5, NULL, NULL, ?6, ?6)",
            params![
                capture_id.to_string(),
                session_id.to_string(),
                context_id.to_string(),
                text_body,
                branch_name,
                captured_at_text,
            ],
        )?;
        transaction.commit()?;

        Ok(SaveCaptureResult {
            capture_id,
            captured_at,
            enrichment_scheduled: false,
        })
    }

    pub(crate) fn save_image(
        &mut self,
        session_id: CaptureSessionId,
        context_id: ContextId,
        branch_name: Option<&str>,
        capture_id: CaptureId,
        media_id: crate::contract::MediaId,
        relative_path: &str,
        byte_size: u64,
        checksum: &str,
        caption: Option<&str>,
        width_px: u32,
        height_px: u32,
    ) -> Result<SaveCaptureResult, StorageError> {
        let captured_at = Timestamp::now_utc();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("INSERT INTO captures (id, session_id, context_id, kind, text_body, caption, caption_source, branch_name, source_app, source_window_title, captured_at, updated_at) VALUES (?1, ?2, ?3, 'image', NULL, ?4, CASE WHEN ?4 IS NULL THEN NULL ELSE 'user' END, ?5, NULL, NULL, ?6, ?6)", params![capture_id.to_string(), session_id.to_string(), context_id.to_string(), caption, branch_name, captured_at.to_string()])?;
        let byte_size = i64::try_from(byte_size)
            .map_err(|_| StorageError::Sql(rusqlite::Error::IntegralValueOutOfRange(0, 0)))?;
        transaction.execute("INSERT INTO media_assets (id, capture_id, kind, relative_path, mime_type, byte_size, checksum, duration_ms, width_px, height_px, created_at) VALUES (?1, ?2, 'image', ?3, 'image/png', ?4, ?5, NULL, ?6, ?7, ?8)", params![media_id.to_string(), capture_id.to_string(), relative_path, byte_size, checksum, width_px, height_px, captured_at.to_string()])?;
        transaction.commit()?;
        Ok(SaveCaptureResult {
            capture_id,
            captured_at,
            enrichment_scheduled: false,
        })
    }

    pub(crate) fn save_audio(
        &mut self,
        session_id: CaptureSessionId,
        context_id: ContextId,
        branch_name: Option<&str>,
        capture_id: CaptureId,
        media_id: crate::contract::MediaId,
        relative_path: &str,
        byte_size: u64,
        checksum: &str,
        caption: Option<&str>,
        duration_ms: u64,
    ) -> Result<SaveCaptureResult, StorageError> {
        let byte_size = i64::try_from(byte_size)
            .map_err(|_| StorageError::Sql(rusqlite::Error::IntegralValueOutOfRange(0, 0)))?;
        let duration_ms = i64::try_from(duration_ms)
            .map_err(|_| StorageError::Sql(rusqlite::Error::IntegralValueOutOfRange(0, 0)))?;
        let captured_at = Timestamp::now_utc();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO captures (id, session_id, context_id, kind, text_body, caption,
                caption_source, branch_name, source_app, source_window_title, captured_at, updated_at)
             VALUES (?1, ?2, ?3, 'audio', NULL, ?4,
                CASE WHEN ?4 IS NULL THEN NULL ELSE 'user' END, ?5, NULL, NULL, ?6, ?6)",
            params![
                capture_id.to_string(),
                session_id.to_string(),
                context_id.to_string(),
                caption,
                branch_name,
                captured_at.to_string()
            ],
        )?;
        transaction.execute(
            "INSERT INTO media_assets (id, capture_id, kind, relative_path, mime_type, byte_size,
                checksum, duration_ms, width_px, height_px, created_at)
             VALUES (?1, ?2, 'audio', ?3, 'audio/wav', ?4, ?5, ?6, NULL, NULL, ?7)",
            params![
                media_id.to_string(),
                capture_id.to_string(),
                relative_path,
                byte_size,
                checksum,
                duration_ms,
                captured_at.to_string()
            ],
        )?;
        transaction.commit()?;
        Ok(SaveCaptureResult {
            capture_id,
            captured_at,
            enrichment_scheduled: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        contract::CaptureSessionId,
        storage::{Database, captures::CaptureRepository, contexts::ContextRepository},
    };

    #[test]
    fn text_capture_preserves_exact_body_and_updates_fts_on_commit() {
        let mut database = Database::open_in_memory().unwrap();
        let context = ContextRepository::new(database.connection())
            .create_standalone("Notes")
            .unwrap();
        let session_id = CaptureSessionId::new();
        let body = "  Première ligne\n第二行  ";

        let saved = CaptureRepository::new(database.connection_mut())
            .save_text(session_id, context.id, body, None)
            .unwrap();
        let stored: (String, Option<String>, Option<String>, Option<String>) = database
            .connection()
            .query_row(
                "SELECT text_body, caption, caption_source, branch_name FROM captures WHERE id = ?1",
                [saved.capture_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        let indexed: String = database
            .connection()
            .query_row(
                "SELECT search_text FROM captures_fts WHERE capture_id = ?1",
                [saved.capture_id.to_string()],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(stored, (body.to_owned(), None, None, None));
        assert_eq!(indexed, body);
        assert!(!saved.enrichment_scheduled);
        assert!(
            serde_json::to_value(saved.captured_at)
                .unwrap()
                .as_str()
                .unwrap()
                .ends_with('Z')
        );
    }

    #[test]
    fn repeated_session_id_cannot_publish_a_second_capture() {
        let mut database = Database::open_in_memory().unwrap();
        let context = ContextRepository::new(database.connection())
            .create_standalone("Notes")
            .unwrap();
        let session_id = CaptureSessionId::new();

        CaptureRepository::new(database.connection_mut())
            .save_text(session_id, context.id, "first", None)
            .unwrap();
        let repeated = CaptureRepository::new(database.connection_mut())
            .save_text(session_id, context.id, "second", None);
        let (capture_count, indexed_count, body): (i64, i64, String) = database
            .connection()
            .query_row(
                "SELECT (SELECT count(*) FROM captures),
                        (SELECT count(*) FROM captures_fts),
                        (SELECT text_body FROM captures LIMIT 1)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();

        assert!(repeated.is_err());
        assert_eq!(
            (capture_count, indexed_count, body),
            (1, 1, "first".to_owned())
        );
    }
}
