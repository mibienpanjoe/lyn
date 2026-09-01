//! Optional post-commit enrichment coordination.

use std::{error::Error, fmt, str::FromStr};

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    contract::{CaptionSource, CaptureId, Timestamp},
    storage::StorageError,
};

const MAX_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnrichmentKind {
    SpeechCaption,
}

impl EnrichmentKind {
    fn stored(self) -> &'static str {
        match self {
            Self::SpeechCaption => "speech_caption",
        }
    }

    fn source(self) -> CaptionSource {
        match self {
            Self::SpeechCaption => CaptionSource::TranscriptGenerated,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EnrichmentJob {
    pub(crate) id: String,
    pub(crate) capture_id: CaptureId,
    pub(crate) kind: EnrichmentKind,
    pub(crate) input_revision: u32,
    pub(crate) attempt_count: u32,
}

#[derive(Debug)]
pub(crate) enum EnrichmentError {
    Storage(StorageError),
    InvalidStoredJob,
}

impl fmt::Display for EnrichmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(_) => formatter.write_str("enrichment storage is unavailable"),
            Self::InvalidStoredJob => formatter.write_str("stored enrichment job is invalid"),
        }
    }
}

impl Error for EnrichmentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::InvalidStoredJob => None,
        }
    }
}

impl From<StorageError> for EnrichmentError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

impl From<rusqlite::Error> for EnrichmentError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(StorageError::Sql(error))
    }
}

pub(crate) struct EnrichmentQueue<'a> {
    connection: &'a mut Connection,
}

pub(crate) trait EnrichmentProcessor {
    fn generate(&mut self, job: &EnrichmentJob) -> Result<Option<String>, &'static str>;
}

impl<'a> EnrichmentQueue<'a> {
    pub(crate) fn new(connection: &'a mut Connection) -> Self {
        Self { connection }
    }

    pub(crate) fn enqueue(
        &self,
        capture_id: CaptureId,
        kind: EnrichmentKind,
        input_revision: u32,
    ) -> Result<bool, EnrichmentError> {
        let changed = self.connection.execute(
            "INSERT OR IGNORE INTO enrichment_jobs (
                id, capture_id, kind, status, input_revision, attempt_count,
                last_error_code, created_at, updated_at
             ) VALUES (?1, ?2, ?3, 'pending', ?4, 0, NULL, ?5, ?5)",
            params![
                uuid::Uuid::new_v4().to_string(),
                capture_id.to_string(),
                kind.stored(),
                input_revision,
                Timestamp::now_utc().to_string(),
            ],
        )?;
        Ok(changed == 1)
    }

    pub(crate) fn recover_interrupted(&self) -> Result<usize, EnrichmentError> {
        self.connection
            .execute(
                "UPDATE enrichment_jobs
                 SET status = 'pending', updated_at = ?1
                 WHERE status = 'running' AND attempt_count < ?2",
                params![Timestamp::now_utc().to_string(), MAX_ATTEMPTS],
            )
            .map_err(Into::into)
    }

    pub(crate) fn claim_next(&mut self) -> Result<Option<EnrichmentJob>, EnrichmentError> {
        let transaction = self.connection.transaction()?;
        let stored = transaction
            .query_row(
                "SELECT id, capture_id, kind, input_revision, attempt_count
                 FROM enrichment_jobs
                 WHERE status IN ('pending', 'failed') AND attempt_count < ?1
                 ORDER BY updated_at, id LIMIT 1",
                [MAX_ATTEMPTS],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, u32>(3)?,
                        row.get::<_, u32>(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, capture_id, kind, input_revision, attempt_count)) = stored else {
            return Ok(None);
        };
        transaction.execute(
            "UPDATE enrichment_jobs
             SET status = 'running', attempt_count = attempt_count + 1,
                 last_error_code = NULL, updated_at = ?1
             WHERE id = ?2",
            params![Timestamp::now_utc().to_string(), id],
        )?;
        transaction.commit()?;
        Ok(Some(EnrichmentJob {
            id,
            capture_id: CaptureId::from_str(&capture_id)
                .map_err(|_| EnrichmentError::InvalidStoredJob)?,
            kind: parse_kind(&kind)?,
            input_revision,
            attempt_count: attempt_count + 1,
        }))
    }

    pub(crate) fn run_one(
        &mut self,
        enabled: bool,
        processor: &mut impl EnrichmentProcessor,
    ) -> Result<bool, EnrichmentError> {
        let Some(job) = self.claim_next()? else {
            return Ok(false);
        };
        if !enabled {
            self.skip(&job)?;
            return Ok(true);
        }
        match processor.generate(&job) {
            Ok(Some(caption)) => {
                self.apply_generated(&job, &caption)?;
            }
            Ok(None) => self.skip(&job)?,
            Err(code) => self.fail(&job, code)?,
        }
        Ok(true)
    }

    pub(crate) fn apply_generated(
        &mut self,
        job: &EnrichmentJob,
        caption: &str,
    ) -> Result<bool, EnrichmentError> {
        if caption.trim().is_empty() || caption.trim() != caption || caption.chars().count() > 500 {
            self.fail(job, "INVALID_RESULT")?;
            return Ok(false);
        }
        let transaction = self.connection.transaction()?;
        let applied = transaction.execute(
            "UPDATE captures
             SET caption = ?1, caption_source = ?2,
                 caption_revision = caption_revision + 1, updated_at = ?3
             WHERE id = ?4 AND caption IS NULL AND caption_source IS NULL
               AND caption_revision = ?5",
            params![
                caption,
                caption_source_name(job.kind.source()),
                Timestamp::now_utc().to_string(),
                job.capture_id.to_string(),
                job.input_revision,
            ],
        )?;
        transaction.execute(
            "UPDATE enrichment_jobs SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![
                if applied == 1 { "completed" } else { "skipped" },
                Timestamp::now_utc().to_string(),
                job.id,
            ],
        )?;
        transaction.commit()?;
        Ok(applied == 1)
    }

    pub(crate) fn skip(&self, job: &EnrichmentJob) -> Result<(), EnrichmentError> {
        self.finish(job, "skipped", None)
    }

    pub(crate) fn fail(
        &self,
        job: &EnrichmentJob,
        safe_error_code: &str,
    ) -> Result<(), EnrichmentError> {
        let code = if safe_error_code.len() <= 40
            && safe_error_code
                .chars()
                .all(|character| character.is_ascii_uppercase() || character == '_')
        {
            safe_error_code
        } else {
            "ENRICHMENT_FAILED"
        };
        self.finish(job, "failed", Some(code))
    }

    fn finish(
        &self,
        job: &EnrichmentJob,
        status: &str,
        safe_error_code: Option<&str>,
    ) -> Result<(), EnrichmentError> {
        self.connection.execute(
            "UPDATE enrichment_jobs
             SET status = ?1, last_error_code = ?2, updated_at = ?3
             WHERE id = ?4 AND status = 'running'",
            params![
                status,
                safe_error_code,
                Timestamp::now_utc().to_string(),
                job.id,
            ],
        )?;
        Ok(())
    }
}

pub(crate) fn schedule_after_commit(
    connection: &mut Connection,
    capture_id: CaptureId,
    local_speech_enabled: bool,
    processor_available: bool,
) -> Result<bool, EnrichmentError> {
    if !local_speech_enabled || !processor_available {
        return Ok(false);
    }
    let eligible: bool = connection.query_row(
        "SELECT kind = 'audio' AND caption IS NULL FROM captures WHERE id = ?1",
        [capture_id.to_string()],
        |row| row.get(0),
    )?;
    if !eligible {
        return Ok(false);
    }
    EnrichmentQueue::new(connection).enqueue(capture_id, EnrichmentKind::SpeechCaption, 0)
}

fn parse_kind(value: &str) -> Result<EnrichmentKind, EnrichmentError> {
    match value {
        "speech_caption" => Ok(EnrichmentKind::SpeechCaption),
        _ => Err(EnrichmentError::InvalidStoredJob),
    }
}

fn caption_source_name(source: CaptionSource) -> &'static str {
    match source {
        CaptionSource::User => "user",
        CaptionSource::ContextGenerated => "context_generated",
        CaptionSource::TranscriptGenerated => "transcript_generated",
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::{
        contract::{CaptureId, CaptureSessionId, MediaId},
        storage::{Database, captures::CaptureRepository, contexts::ContextRepository},
    };

    use super::{EnrichmentKind, EnrichmentQueue, MAX_ATTEMPTS};

    fn audio(database: &mut Database, caption: Option<&str>) -> CaptureId {
        let context = ContextRepository::new(database.connection())
            .create_standalone("Notes")
            .unwrap();
        CaptureRepository::new(database.connection_mut())
            .save_audio(
                CaptureSessionId::new(),
                context.id,
                None,
                CaptureId::new(),
                MediaId::new(),
                &format!("audio/{}.wav", CaptureId::new()),
                4,
                "checksum",
                caption,
                1000,
            )
            .unwrap()
            .capture_id
    }

    #[test]
    fn generated_caption_applies_only_to_the_unchanged_revision() {
        let mut database = Database::open_in_memory().unwrap();
        let capture_id = audio(&mut database, None);
        let mut queue = EnrichmentQueue::new(database.connection_mut());
        assert!(
            queue
                .enqueue(capture_id, EnrichmentKind::SpeechCaption, 0)
                .unwrap()
        );
        let job = queue.claim_next().unwrap().unwrap();

        assert!(queue.apply_generated(&job, "Exact transcript").unwrap());
        let stored: (String, String, u32) = queue
            .connection
            .query_row(
                "SELECT caption, caption_source, caption_revision FROM captures WHERE id = ?1",
                [capture_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            stored,
            (
                "Exact transcript".to_owned(),
                "transcript_generated".to_owned(),
                1
            )
        );
    }

    #[test]
    fn user_caption_and_revision_win_over_a_stale_generated_result() {
        let mut database = Database::open_in_memory().unwrap();
        let capture_id = audio(&mut database, None);
        let mut queue = EnrichmentQueue::new(database.connection_mut());
        queue
            .enqueue(capture_id, EnrichmentKind::SpeechCaption, 0)
            .unwrap();
        let job = queue.claim_next().unwrap().unwrap();
        queue
            .connection
            .execute(
                "UPDATE captures SET caption = 'User words', caption_source = 'user',
             caption_revision = caption_revision + 1 WHERE id = ?1",
                [capture_id.to_string()],
            )
            .unwrap();

        assert!(!queue.apply_generated(&job, "Stale transcript").unwrap());
        let stored: (String, String) = queue
            .connection
            .query_row(
                "SELECT caption, caption_source FROM captures WHERE id = ?1",
                [capture_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored, ("User words".to_owned(), "user".to_owned()));
    }

    #[test]
    fn failures_retry_only_to_the_bound_and_disabled_work_can_be_skipped() {
        let mut database = Database::open_in_memory().unwrap();
        let capture_id = audio(&mut database, None);
        let mut queue = EnrichmentQueue::new(database.connection_mut());
        queue
            .enqueue(capture_id, EnrichmentKind::SpeechCaption, 0)
            .unwrap();
        for attempt in 1..=MAX_ATTEMPTS {
            let job = queue.claim_next().unwrap().unwrap();
            assert_eq!(job.attempt_count, attempt);
            queue.fail(&job, "MODEL_NOT_AVAILABLE").unwrap();
        }
        assert!(queue.claim_next().unwrap().is_none());

        let second = audio(&mut database, None);
        let mut queue = EnrichmentQueue::new(database.connection_mut());
        queue
            .enqueue(second, EnrichmentKind::SpeechCaption, 0)
            .unwrap();
        let disabled = queue.claim_next().unwrap().unwrap();
        queue.skip(&disabled).unwrap();
        assert!(queue.claim_next().unwrap().is_none());
    }

    #[test]
    fn interrupted_running_job_returns_to_pending_after_restart() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("lyn.db");
        {
            let mut database = Database::open(&path).unwrap();
            let capture_id = audio(&mut database, None);
            let mut queue = EnrichmentQueue::new(database.connection_mut());
            queue
                .enqueue(capture_id, EnrichmentKind::SpeechCaption, 0)
                .unwrap();
            assert!(queue.claim_next().unwrap().is_some());
        }
        let mut reopened = Database::open(&path).unwrap();
        let mut queue = EnrichmentQueue::new(reopened.connection_mut());

        assert_eq!(queue.recover_interrupted().unwrap(), 1);
        assert!(queue.claim_next().unwrap().is_some());
    }

    #[test]
    fn post_commit_scheduling_requires_opt_in_and_an_available_processor() {
        let mut database = Database::open_in_memory().unwrap();
        let capture_id = audio(&mut database, None);

        assert!(
            !super::schedule_after_commit(database.connection_mut(), capture_id, false, true,)
                .unwrap()
        );
        assert!(
            !super::schedule_after_commit(database.connection_mut(), capture_id, true, false,)
                .unwrap()
        );
        assert!(
            super::schedule_after_commit(database.connection_mut(), capture_id, true, true,)
                .unwrap()
        );
    }
}
