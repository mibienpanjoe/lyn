use std::{error::Error, fmt, str::FromStr};

use rusqlite::{Connection, Row, params_from_iter, types::Value};
use serde::{Deserialize, Serialize};

use crate::{
    contract::{
        CaptionSource, CaptureDetail, CaptureId, CaptureKind, CaptureSummary, ContextKind,
        ContextRef, EnrichmentStatus, LibraryScope, ListCapturesInput, MediaId, MediaKind,
        MediaSummary, Page, Timestamp,
    },
    media::staging::MediaStore,
};

const TEXT_EXCERPT_CHARS: usize = 280;
const MAX_CURSOR_BYTES: usize = 512;

#[derive(Debug)]
pub(crate) enum LibraryError {
    InvalidCursor,
    ContextNotFound,
    CaptureNotFound,
    Storage(rusqlite::Error),
}

impl fmt::Display for LibraryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCursor => formatter.write_str("invalid Library cursor"),
            Self::ContextNotFound => formatter.write_str("context not found"),
            Self::CaptureNotFound => formatter.write_str("capture not found"),
            Self::Storage(_) => formatter.write_str("Library storage is unavailable"),
        }
    }
}

impl Error for LibraryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for LibraryError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LibraryCursor {
    captured_at: Timestamp,
    capture_id: CaptureId,
}

pub(crate) struct LibraryService<'a> {
    connection: &'a Connection,
    media_store: &'a MediaStore,
}

impl<'a> LibraryService<'a> {
    pub(crate) fn new(connection: &'a Connection, media_store: &'a MediaStore) -> Self {
        Self {
            connection,
            media_store,
        }
    }

    pub(crate) fn list(
        &self,
        input: &ListCapturesInput,
    ) -> Result<Page<CaptureSummary>, LibraryError> {
        if let LibraryScope::Context { context_id } = input.scope
            && !self.context_exists(context_id)?
        {
            return Err(LibraryError::ContextNotFound);
        }
        let cursor = input.cursor.as_deref().map(decode_cursor).transpose()?;
        let mut sql = format!("{} WHERE 1 = 1", select_capture_fields());
        let mut parameters = Vec::<Value>::new();

        if let LibraryScope::Context { context_id } = input.scope {
            sql.push_str(" AND c.context_id = ?");
            parameters.push(context_id.to_string().into());
        }
        if let Some(branch_name) = input.branch_name.as_deref() {
            sql.push_str(" AND c.branch_name = ?");
            parameters.push(branch_name.to_owned().into());
        }
        if !input.capture_kinds.is_empty() {
            sql.push_str(" AND c.kind IN (");
            sql.push_str(&vec!["?"; input.capture_kinds.len()].join(", "));
            sql.push(')');
            parameters.extend(
                input
                    .capture_kinds
                    .iter()
                    .map(|kind| capture_kind_name(*kind).to_owned().into()),
            );
        }
        if let Some(captured_from) = input.captured_from.as_ref() {
            sql.push_str(" AND c.captured_at >= ?");
            parameters.push(captured_from.to_string().into());
        }
        if let Some(captured_to) = input.captured_to.as_ref() {
            sql.push_str(" AND c.captured_at <= ?");
            parameters.push(captured_to.to_string().into());
        }
        if let Some(cursor) = cursor {
            sql.push_str(" AND (c.captured_at < ? OR (c.captured_at = ? AND c.id < ?))");
            parameters.push(cursor.captured_at.to_string().into());
            parameters.push(cursor.captured_at.to_string().into());
            parameters.push(cursor.capture_id.to_string().into());
        }
        sql.push_str(" ORDER BY c.captured_at DESC, c.id DESC LIMIT ?");
        parameters.push(Value::Integer(i64::from(input.limit) + 1));

        let mut statement = self.connection.prepare(&sql)?;
        let mut items = statement
            .query_map(params_from_iter(parameters), |row| {
                decode_capture_row(row, self.media_store).map(|capture| capture.summary)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let has_more = items.len() > usize::from(input.limit);
        if has_more {
            items.truncate(usize::from(input.limit));
        }
        let next_cursor = has_more
            .then(|| items.last())
            .flatten()
            .map(|capture| {
                encode_cursor(&LibraryCursor {
                    captured_at: capture.captured_at.clone(),
                    capture_id: capture.id,
                })
            })
            .transpose()?;
        Ok(Page { items, next_cursor })
    }

    pub(crate) fn get(&self, capture_id: CaptureId) -> Result<CaptureDetail, LibraryError> {
        let sql = format!("{} WHERE c.id = ?", select_capture_fields());
        self.connection
            .query_row(&sql, [capture_id.to_string()], |row| {
                decode_capture_row(row, self.media_store).map(|capture| capture.detail)
            })
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => LibraryError::CaptureNotFound,
                error => LibraryError::Storage(error),
            })
    }

    fn context_exists(&self, context_id: crate::contract::ContextId) -> Result<bool, LibraryError> {
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM contexts WHERE id = ?1)",
                [context_id.to_string()],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }
}

struct DecodedCapture {
    summary: CaptureSummary,
    detail: CaptureDetail,
}

fn select_capture_fields() -> &'static str {
    "SELECT
        c.id, c.kind, c.branch_name, c.captured_at, c.text_body, c.caption,
        c.caption_source, c.source_app, c.source_window_title, c.updated_at,
        context.id, context.kind, context.name,
        media.id, media.kind, media.relative_path, media.duration_ms,
        media.width_px, media.height_px,
        COALESCE((
            SELECT status FROM enrichment_jobs job
            WHERE job.capture_id = c.id
            ORDER BY job.updated_at DESC, job.id DESC LIMIT 1
        ), 'not_requested')
     FROM captures c
     JOIN contexts context ON context.id = c.context_id
     LEFT JOIN media_assets media ON media.capture_id = c.id"
}

fn decode_capture_row(row: &Row<'_>, media_store: &MediaStore) -> rusqlite::Result<DecodedCapture> {
    let capture_id = parse_id::<CaptureId>(row.get::<_, String>(0)?, 0)?;
    let capture_kind = parse_capture_kind(&row.get::<_, String>(1)?, 1)?;
    let captured_at = parse_timestamp(row.get(3)?, 3)?;
    let text_body: Option<String> = row.get(4)?;
    let caption_source = row
        .get::<_, Option<String>>(6)?
        .map(|value| parse_caption_source(&value, 6))
        .transpose()?;
    let updated_at = parse_timestamp(row.get(9)?, 9)?;
    let context = ContextRef {
        id: parse_id(row.get(10)?, 10)?,
        kind: parse_context_kind(&row.get::<_, String>(11)?, 11)?,
        name: row.get(12)?,
    };
    let media_id = row.get::<_, Option<String>>(13)?;
    let media = media_id
        .map(|media_id| {
            let media_id = parse_id::<MediaId>(media_id, 13)?;
            let kind = parse_media_kind(&row.get::<_, String>(14)?, 14)?;
            let relative_path: String = row.get(15)?;
            Ok::<MediaSummary, rusqlite::Error>(MediaSummary {
                media_id,
                kind,
                preview_uri: format!("lyn-media://capture/{media_id}"),
                duration_ms: optional_u64(row.get(16)?, 16)?,
                width_px: optional_u32(row.get(17)?, 17)?,
                height_px: optional_u32(row.get(18)?, 18)?,
                available: media_store.final_available(&relative_path),
            })
        })
        .transpose()?;
    let enrichment_status = parse_enrichment_status(&row.get::<_, String>(19)?, 19)?;
    let text_excerpt = text_body.as_deref().map(text_excerpt);
    let summary = CaptureSummary {
        id: capture_id,
        kind: capture_kind,
        context: context.clone(),
        branch_name: row.get(2)?,
        captured_at: captured_at.clone(),
        text_excerpt,
        caption: row.get(5)?,
        caption_source,
        media,
    };
    let detail = CaptureDetail {
        id: summary.id,
        kind: summary.kind,
        context,
        branch_name: summary.branch_name.clone(),
        captured_at,
        text_excerpt: summary.text_excerpt.clone(),
        caption: summary.caption.clone(),
        caption_source: summary.caption_source,
        media: summary.media.clone(),
        text_body,
        source_app: row.get(7)?,
        source_window_title: row.get(8)?,
        updated_at,
        enrichment_status,
    };
    Ok(DecodedCapture { summary, detail })
}

fn text_excerpt(body: &str) -> String {
    let mut excerpt = body
        .chars()
        .take(TEXT_EXCERPT_CHARS + 1)
        .collect::<String>();
    if excerpt.chars().count() > TEXT_EXCERPT_CHARS {
        excerpt.pop();
        excerpt.push('…');
    }
    excerpt
}

fn encode_cursor(cursor: &LibraryCursor) -> Result<String, LibraryError> {
    let bytes = serde_json::to_vec(cursor).map_err(|_| LibraryError::InvalidCursor)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn decode_cursor(value: &str) -> Result<LibraryCursor, LibraryError> {
    if value.len() > MAX_CURSOR_BYTES * 2 || value.len() % 2 != 0 {
        return Err(LibraryError::InvalidCursor);
    }
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digits = std::str::from_utf8(pair).map_err(|_| LibraryError::InvalidCursor)?;
            u8::from_str_radix(digits, 16).map_err(|_| LibraryError::InvalidCursor)
        })
        .collect::<Result<Vec<_>, _>>()?;
    serde_json::from_slice(&bytes).map_err(|_| LibraryError::InvalidCursor)
}

fn capture_kind_name(kind: CaptureKind) -> &'static str {
    match kind {
        CaptureKind::Text => "text",
        CaptureKind::Image => "image",
        CaptureKind::Audio => "audio",
    }
}

fn parse_capture_kind(value: &str, column: usize) -> rusqlite::Result<CaptureKind> {
    match value {
        "text" => Ok(CaptureKind::Text),
        "image" => Ok(CaptureKind::Image),
        "audio" => Ok(CaptureKind::Audio),
        _ => Err(invalid_value(column, value)),
    }
}

fn parse_context_kind(value: &str, column: usize) -> rusqlite::Result<ContextKind> {
    match value {
        "project" => Ok(ContextKind::Project),
        "standalone" => Ok(ContextKind::Standalone),
        _ => Err(invalid_value(column, value)),
    }
}

fn parse_caption_source(value: &str, column: usize) -> rusqlite::Result<CaptionSource> {
    match value {
        "user" => Ok(CaptionSource::User),
        "context_generated" => Ok(CaptionSource::ContextGenerated),
        "transcript_generated" => Ok(CaptionSource::TranscriptGenerated),
        _ => Err(invalid_value(column, value)),
    }
}

fn parse_media_kind(value: &str, column: usize) -> rusqlite::Result<MediaKind> {
    match value {
        "image" => Ok(MediaKind::Image),
        "audio" => Ok(MediaKind::Audio),
        _ => Err(invalid_value(column, value)),
    }
}

fn parse_enrichment_status(value: &str, column: usize) -> rusqlite::Result<EnrichmentStatus> {
    match value {
        "not_requested" => Ok(EnrichmentStatus::NotRequested),
        "pending" | "running" => Ok(EnrichmentStatus::Pending),
        "completed" => Ok(EnrichmentStatus::Completed),
        "skipped" => Ok(EnrichmentStatus::Skipped),
        "failed" => Ok(EnrichmentStatus::Failed),
        _ => Err(invalid_value(column, value)),
    }
}

fn parse_id<T>(value: String, column: usize) -> rusqlite::Result<T>
where
    T: FromStr,
    T::Err: Error + Send + Sync + 'static,
{
    T::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn parse_timestamp(value: String, column: usize) -> rusqlite::Result<Timestamp> {
    serde_json::from_value(serde_json::Value::String(value)).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn optional_u64(value: Option<i64>, column: usize) -> rusqlite::Result<Option<u64>> {
    value
        .map(|value| u64::try_from(value).map_err(|_| integral_error(column, value)))
        .transpose()
}

fn optional_u32(value: Option<i64>, column: usize) -> rusqlite::Result<Option<u32>> {
    value
        .map(|value| u32::try_from(value).map_err(|_| integral_error(column, value)))
        .transpose()
}

fn integral_error(column: usize, value: i64) -> rusqlite::Error {
    rusqlite::Error::IntegralValueOutOfRange(column, value)
}

fn invalid_value(column: usize, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        rusqlite::types::Type::Text,
        format!("invalid stored value: {value}").into(),
    )
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use tempfile::tempdir;

    use crate::{
        contract::{
            CaptureId, CaptureSessionId, ContextId, LibraryScope, ListCapturesInput, MediaKind,
            MediaMimeType,
        },
        media::staging::MediaStore,
        storage::Database,
    };

    use super::LibraryService;

    fn insert_context(database: &Database, id: &str, name: &str) {
        database
            .connection()
            .execute(
                "INSERT INTO contexts (id, kind, name, created_at, updated_at)
                 VALUES (?1, 'project', ?2, '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z')",
                (id, name),
            )
            .unwrap();
    }

    fn insert_text_capture(
        database: &Database,
        id: &str,
        session_id: &str,
        context_id: &str,
        branch: &str,
        captured_at: &str,
        body: &str,
    ) {
        database
            .connection()
            .execute(
                "INSERT INTO captures (
                    id, session_id, context_id, kind, text_body, caption, caption_source,
                    branch_name, source_app, source_window_title, captured_at, updated_at
                 ) VALUES (?1, ?2, ?3, 'text', ?4, NULL, NULL, ?5, NULL, NULL, ?6, ?6)",
                (id, session_id, context_id, body, branch, captured_at),
            )
            .unwrap();
    }

    fn input(scope: LibraryScope, limit: u16) -> ListCapturesInput {
        ListCapturesInput {
            scope,
            branch_name: None,
            capture_kinds: vec![],
            captured_from: None,
            captured_to: None,
            cursor: None,
            limit,
        }
    }

    #[test]
    fn context_scope_owns_one_stream_and_branch_is_only_a_predicate() {
        let database = Database::open_in_memory().unwrap();
        let directory = tempdir().unwrap();
        let media = MediaStore::open(directory.path()).unwrap();
        let project_a = "11111111-1111-4111-8111-111111111111";
        let project_b = "22222222-2222-4222-8222-222222222222";
        insert_context(&database, project_a, "Project A");
        insert_context(&database, project_b, "Project B");
        insert_text_capture(
            &database,
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1",
            "aaaaaaaa-1111-4111-8111-111111111111",
            project_a,
            "main",
            "2026-09-01T12:00:00Z",
            "main note",
        );
        insert_text_capture(
            &database,
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2",
            "aaaaaaaa-2222-4222-8222-222222222222",
            project_a,
            "feature",
            "2026-09-01T11:00:00Z",
            "feature note",
        );
        insert_text_capture(
            &database,
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb1",
            "bbbbbbbb-1111-4111-8111-111111111111",
            project_b,
            "main",
            "2026-09-01T13:00:00Z",
            "other project",
        );

        let service = LibraryService::new(database.connection(), &media);
        let mut request = input(
            LibraryScope::Context {
                context_id: ContextId::from_str(project_a).unwrap(),
            },
            50,
        );
        let all_branches = service.list(&request).unwrap();
        request.branch_name = Some("feature".to_owned());
        let feature_only = service.list(&request).unwrap();

        assert_eq!(
            all_branches
                .items
                .iter()
                .map(|capture| capture.branch_name.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("main"), Some("feature")]
        );
        assert_eq!(feature_only.items.len(), 1);
        assert_eq!(feature_only.items[0].context.name, "Project A");
    }

    #[test]
    fn cursor_pages_equal_timestamps_by_descending_capture_id_without_overlap() {
        let database = Database::open_in_memory().unwrap();
        let directory = tempdir().unwrap();
        let media = MediaStore::open(directory.path()).unwrap();
        let context = "11111111-1111-4111-8111-111111111111";
        insert_context(&database, context, "Project");
        for (id, session) in [
            (
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa3",
                "aaaaaaaa-3333-4333-8333-333333333333",
            ),
            (
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2",
                "aaaaaaaa-2222-4222-8222-222222222222",
            ),
            (
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1",
                "aaaaaaaa-1111-4111-8111-111111111111",
            ),
        ] {
            insert_text_capture(
                &database,
                id,
                session,
                context,
                "main",
                "2026-09-01T12:00:00Z",
                id,
            );
        }
        let service = LibraryService::new(database.connection(), &media);
        let mut request = input(LibraryScope::All, 2);

        let first = service.list(&request).unwrap();
        request.cursor = first.next_cursor.clone();
        let second = service.list(&request).unwrap();

        assert_eq!(first.items.len(), 2);
        assert!(first.next_cursor.is_some());
        assert_eq!(second.items.len(), 1);
        assert_eq!(second.next_cursor, None);
        assert_eq!(
            first
                .items
                .iter()
                .chain(&second.items)
                .map(|capture| capture.id)
                .collect::<Vec<CaptureId>>(),
            [
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa3",
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2",
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1",
            ]
            .map(|id| CaptureId::from_str(id).unwrap())
        );
    }

    #[test]
    fn kind_and_date_filters_are_combined_and_invalid_cursors_are_rejected() {
        let database = Database::open_in_memory().unwrap();
        let directory = tempdir().unwrap();
        let media = MediaStore::open(directory.path()).unwrap();
        let context = "11111111-1111-4111-8111-111111111111";
        insert_context(&database, context, "Project");
        insert_text_capture(
            &database,
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1",
            "aaaaaaaa-1111-4111-8111-111111111111",
            context,
            "main",
            "2026-09-01T10:00:00Z",
            "outside range",
        );
        insert_text_capture(
            &database,
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2",
            "aaaaaaaa-2222-4222-8222-222222222222",
            context,
            "main",
            "2026-09-01T11:00:00Z",
            "inside range",
        );
        database
            .connection()
            .execute(
                "INSERT INTO captures (
                id, session_id, context_id, kind, text_body, caption, caption_source,
                branch_name, source_app, source_window_title, captured_at, updated_at
             ) VALUES (?1, ?2, ?3, 'image', NULL, 'inside image', 'user',
                'main', NULL, NULL, ?4, ?4)",
                (
                    "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa3",
                    "aaaaaaaa-3333-4333-8333-333333333333",
                    context,
                    "2026-09-01T11:30:00Z",
                ),
            )
            .unwrap();
        let service = LibraryService::new(database.connection(), &media);
        let mut request = input(LibraryScope::All, 50);
        request.capture_kinds = vec![crate::contract::CaptureKind::Text];
        request.captured_from =
            serde_json::from_value(serde_json::json!("2026-09-01T10:30:00Z")).ok();
        request.captured_to =
            serde_json::from_value(serde_json::json!("2026-09-01T11:15:00Z")).ok();

        let page = service.list(&request).unwrap();

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].text_excerpt.as_deref(), Some("inside range"));
        request.cursor = Some("not-hex".to_owned());
        assert!(matches!(
            service.list(&request),
            Err(super::LibraryError::InvalidCursor)
        ));
    }

    #[test]
    fn detail_keeps_media_metadata_when_the_committed_file_is_missing() {
        let database = Database::open_in_memory().unwrap();
        let directory = tempdir().unwrap();
        let mut media = MediaStore::open(directory.path()).unwrap();
        let context = "11111111-1111-4111-8111-111111111111";
        let capture_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1";
        insert_context(&database, context, "Project");
        database
            .connection()
            .execute(
                "INSERT INTO captures (
                id, session_id, context_id, kind, text_body, caption, caption_source,
                branch_name, source_app, source_window_title, captured_at, updated_at
             ) VALUES (?1, ?2, ?3, 'image', NULL, 'Exact caption', 'user',
                'feature', 'Code', 'Editor', ?4, ?4)",
                (
                    capture_id,
                    "aaaaaaaa-1111-4111-8111-111111111111",
                    context,
                    "2026-09-01T12:00:00Z",
                ),
            )
            .unwrap();
        let staged = media
            .stage_bytes(
                CaptureSessionId::new(),
                MediaKind::Image,
                MediaMimeType::ImagePng,
                b"png bytes",
            )
            .unwrap();
        let finalized = media
            .finalize(
                staged.staged_media_id,
                CaptureId::from_str(capture_id).unwrap(),
                MediaKind::Image,
            )
            .unwrap();
        database
            .connection()
            .execute(
                "INSERT INTO media_assets (
                id, capture_id, kind, relative_path, mime_type, byte_size, checksum,
                duration_ms, width_px, height_px, created_at
             ) VALUES (?1, ?2, 'image', ?3, 'image/png', ?4, ?5, NULL, 640, 480,
                '2026-09-01T12:00:00Z')",
                (
                    finalized.media_id.to_string(),
                    capture_id,
                    &finalized.relative_path,
                    finalized.byte_size as i64,
                    &finalized.checksum,
                ),
            )
            .unwrap();
        media.remove_final(&finalized.relative_path).unwrap();

        let detail = LibraryService::new(database.connection(), &media)
            .get(CaptureId::from_str(capture_id).unwrap())
            .unwrap();

        let stored = detail.media.expect("media metadata remains visible");
        assert!(!stored.available);
        assert_eq!(stored.width_px, Some(640));
        assert_eq!(detail.caption.as_deref(), Some("Exact caption"));
        assert_eq!(detail.source_app.as_deref(), Some("Code"));
    }
}
