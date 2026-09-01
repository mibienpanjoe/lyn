use std::{error::Error, fmt};

use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{
    contract::{CaptureKind, Page, SearchCapturesInput, SearchMatchedField, SearchResultItem},
    media::staging::MediaStore,
};

use super::service::{
    LibraryCursor, LibraryError, capture_kind_name, decode_capture_row, decode_cursor,
    encode_cursor, select_capture_fields,
};

const MAX_QUERY_CHARS: usize = 200;
const MAX_QUERY_TERMS: usize = 16;
const SNIPPET_CHARS: usize = 240;

#[derive(Debug)]
pub(crate) enum SearchError {
    InvalidQuery,
    InvalidCursor,
    ContextNotFound,
    Storage(rusqlite::Error),
}

impl fmt::Display for SearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidQuery => formatter.write_str("invalid search query"),
            Self::InvalidCursor => formatter.write_str("invalid search cursor"),
            Self::ContextNotFound => formatter.write_str("search context not found"),
            Self::Storage(_) => formatter.write_str("search index is unavailable"),
        }
    }
}

impl Error for SearchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for SearchError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error)
    }
}

pub(crate) struct SearchService<'a> {
    connection: &'a Connection,
    media_store: &'a MediaStore,
}

impl<'a> SearchService<'a> {
    pub(crate) fn new(connection: &'a Connection, media_store: &'a MediaStore) -> Self {
        Self {
            connection,
            media_store,
        }
    }

    pub(crate) fn search(
        &self,
        input: &SearchCapturesInput,
    ) -> Result<Page<SearchResultItem>, SearchError> {
        let expression = compile_plain_query(&input.query)?;
        if let Some(context_id) = input.context_id
            && !self.context_exists(context_id)?
        {
            return Err(SearchError::ContextNotFound);
        }
        let cursor = input
            .cursor
            .as_deref()
            .map(decode_cursor)
            .transpose()
            .map_err(|_| SearchError::InvalidCursor)?;
        let mut sql = format!(
            "{} WHERE captures_fts MATCH ?",
            select_capture_fields(
                "FROM captures_fts JOIN captures c ON c.id = captures_fts.capture_id",
            )
        );
        let mut parameters = vec![Value::Text(expression)];

        if let Some(context_id) = input.context_id {
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
                let decoded = decode_capture_row(row, self.media_store)?;
                let (matched_field, source) = match decoded.summary.kind {
                    CaptureKind::Text => (
                        SearchMatchedField::TextBody,
                        decoded.detail.text_body.as_deref().unwrap_or_default(),
                    ),
                    CaptureKind::Image | CaptureKind::Audio => (
                        SearchMatchedField::Caption,
                        decoded.detail.caption.as_deref().unwrap_or_default(),
                    ),
                };
                Ok(SearchResultItem {
                    capture: decoded.summary,
                    matched_field,
                    snippet: bounded_snippet(source),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let has_more = items.len() > usize::from(input.limit);
        if has_more {
            items.truncate(usize::from(input.limit));
        }
        let next_cursor = has_more
            .then(|| items.last())
            .flatten()
            .map(|item| {
                encode_cursor(&LibraryCursor {
                    captured_at: item.capture.captured_at.clone(),
                    capture_id: item.capture.id,
                })
            })
            .transpose()
            .map_err(|error| match error {
                LibraryError::InvalidCursor => SearchError::InvalidCursor,
                LibraryError::Storage(error) => SearchError::Storage(error),
                LibraryError::ContextNotFound | LibraryError::CaptureNotFound => {
                    SearchError::Storage(rusqlite::Error::InvalidQuery)
                }
            })?;
        Ok(Page { items, next_cursor })
    }

    pub(crate) fn rebuild(&self) -> Result<(), SearchError> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute("DELETE FROM captures_fts", [])?;
        transaction.execute(
            "INSERT INTO captures_fts (capture_id, search_text)
             SELECT id, COALESCE(text_body, caption, '') FROM captures",
            [],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn context_exists(&self, context_id: crate::contract::ContextId) -> Result<bool, SearchError> {
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM contexts WHERE id = ?1)",
                [context_id.to_string()],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }
}

fn compile_plain_query(query: &str) -> Result<String, SearchError> {
    if query.trim().is_empty()
        || query.trim() != query
        || query.chars().count() > MAX_QUERY_CHARS
        || query.chars().any(char::is_control)
    {
        return Err(SearchError::InvalidQuery);
    }
    let terms = query
        .split_whitespace()
        .filter(|term| term.chars().any(char::is_alphanumeric))
        .collect::<Vec<_>>();
    if terms.is_empty() || terms.len() > MAX_QUERY_TERMS {
        return Err(SearchError::InvalidQuery);
    }
    Ok(terms
        .into_iter()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND "))
}

fn bounded_snippet(source: &str) -> String {
    let mut snippet = source.chars().take(SNIPPET_CHARS + 1).collect::<String>();
    if snippet.chars().count() > SNIPPET_CHARS {
        snippet.pop();
        snippet.push('…');
    }
    snippet
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::{
        contract::{CaptureKind, SearchCapturesInput},
        media::staging::MediaStore,
        storage::Database,
    };

    use super::{SearchError, SearchService, compile_plain_query};

    fn fixture() -> (Database, MediaStore) {
        let database = Database::open_in_memory().unwrap();
        database.connection().execute_batch(
            "INSERT INTO contexts (id, kind, name, created_at, updated_at)
             VALUES ('11111111-1111-4111-8111-111111111111', 'project', 'Secret Project',
               '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z');
             INSERT INTO captures (id, session_id, context_id, kind, text_body, caption,
               caption_source, branch_name, source_app, source_window_title, captured_at, updated_at)
             VALUES
               ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa3', 'aaaaaaaa-3333-4333-8333-333333333333',
                '11111111-1111-4111-8111-111111111111', 'text', 'alpha OR beta', NULL, NULL,
                'private-branch', 'Hidden App', 'Hidden Window', '2026-09-01T12:00:00Z', '2026-09-01T12:00:00Z'),
               ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2', 'aaaaaaaa-2222-4222-8222-222222222222',
                '11111111-1111-4111-8111-111111111111', 'image', NULL, 'alpha screenshot', 'user',
                'main', NULL, NULL, '2026-09-01T11:00:00Z', '2026-09-01T11:00:00Z'),
               ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1', 'aaaaaaaa-1111-4111-8111-111111111111',
                '11111111-1111-4111-8111-111111111111', 'text', 'beta only', NULL, NULL,
                'main', NULL, NULL, '2026-09-01T10:00:00Z', '2026-09-01T10:00:00Z');",
        ).unwrap();
        let media = MediaStore::open(tempdir().unwrap().keep()).unwrap();
        (database, media)
    }

    fn input(query: &str) -> SearchCapturesInput {
        SearchCapturesInput {
            query: query.to_owned(),
            context_id: None,
            branch_name: None,
            capture_kinds: vec![],
            captured_from: None,
            captured_to: None,
            cursor: None,
            limit: 50,
        }
    }

    #[test]
    fn compiles_plain_terms_without_exposing_fts_operators() {
        assert_eq!(
            compile_plain_query("alpha OR beta").unwrap(),
            "\"alpha\" AND \"OR\" AND \"beta\""
        );
        assert!(matches!(
            compile_plain_query("---"),
            Err(SearchError::InvalidQuery)
        ));
        assert!(matches!(
            compile_plain_query(" alpha"),
            Err(SearchError::InvalidQuery)
        ));

        let (database, media) = fixture();
        let results = SearchService::new(database.connection(), &media)
            .search(&input("alpha OR beta"))
            .unwrap();
        assert_eq!(results.items.len(), 1);
        assert_eq!(results.items[0].snippet, "alpha OR beta");
    }

    #[test]
    fn indexes_only_text_bodies_and_user_visible_captions() {
        let (database, media) = fixture();
        let service = SearchService::new(database.connection(), &media);

        assert_eq!(service.search(&input("alpha")).unwrap().items.len(), 2);
        for private_value in ["Secret", "private-branch", "Hidden"] {
            assert!(
                service
                    .search(&input(private_value))
                    .unwrap()
                    .items
                    .is_empty()
            );
        }
        let mut filtered = input("alpha");
        filtered.capture_kinds = vec![CaptureKind::Image];
        assert_eq!(service.search(&filtered).unwrap().items.len(), 1);
    }

    #[test]
    fn rebuild_restores_equivalent_results_from_canonical_rows() {
        let (database, media) = fixture();
        let service = SearchService::new(database.connection(), &media);
        let before = service.search(&input("alpha")).unwrap();
        database
            .connection()
            .execute("DELETE FROM captures_fts", [])
            .unwrap();
        assert!(service.search(&input("alpha")).unwrap().items.is_empty());

        service.rebuild().unwrap();
        let after = service.search(&input("alpha")).unwrap();

        assert_eq!(before, after);
    }

    #[test]
    fn ten_thousand_matches_still_return_only_the_requested_page() {
        let (database, media) = fixture();
        database
            .connection()
            .execute_batch("BEGIN IMMEDIATE")
            .unwrap();
        {
            let mut insert = database.connection().prepare(
                "INSERT INTO captures (id, session_id, context_id, kind, text_body, caption,
                 caption_source, branch_name, source_app, source_window_title, captured_at, updated_at)
                 VALUES (?1, ?2, '11111111-1111-4111-8111-111111111111', 'text',
                 'bounded needle', NULL, NULL, 'main', NULL, NULL,
                 '2026-08-01T00:00:00Z', '2026-08-01T00:00:00Z')",
            ).unwrap();
            for value in 0..10_000_u32 {
                let id = format!("00000000-0000-4000-8000-{value:012}");
                let session = format!("10000000-0000-4000-8000-{value:012}");
                insert.execute((id, session)).unwrap();
            }
        }
        database.connection().execute_batch("COMMIT").unwrap();
        let mut request = input("needle");
        request.limit = 25;

        let page = SearchService::new(database.connection(), &media)
            .search(&request)
            .unwrap();

        assert_eq!(page.items.len(), 25);
        assert!(page.next_cursor.is_some());
    }
}
