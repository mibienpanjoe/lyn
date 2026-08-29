use std::{error::Error, fmt, str::FromStr};

use rusqlite::{Connection, OptionalExtension, Row, params, types::Type};

use crate::{
    contract::{ContextId, ContextKind, ContextRef},
    storage::StorageError,
};

pub(crate) struct ContextRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> ContextRepository<'connection> {
    pub(crate) fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub(crate) fn create_standalone(&self, name: &str) -> Result<ContextRef, StorageError> {
        let context = ContextRef {
            id: ContextId::new(),
            kind: ContextKind::Standalone,
            name: name.to_owned(),
        };
        self.connection.execute(
            "INSERT INTO contexts (id, kind, name, project_key, project_path, created_at, updated_at)\n             VALUES (?1, 'standalone', ?2, NULL, NULL, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![context.id.to_string(), context.name],
        )?;
        Ok(context)
    }

    pub(crate) fn create_project(
        &self,
        name: &str,
        project_key: Option<&str>,
        project_path: &str,
    ) -> Result<ContextRef, StorageError> {
        let existing = if let Some(project_key) = project_key {
            self.connection
                .query_row(
                    "SELECT id, kind, name FROM contexts WHERE project_key = ?1",
                    [project_key],
                    decode_context,
                )
                .optional()?
        } else {
            self.connection
                .query_row(
                    "SELECT id, kind, name FROM contexts WHERE project_path = ?1",
                    [project_path],
                    decode_context,
                )
                .optional()?
        };
        if let Some(context) = existing {
            return Ok(context);
        }

        let context = ContextRef {
            id: ContextId::new(),
            kind: ContextKind::Project,
            name: name.to_owned(),
        };
        self.connection.execute(
            "INSERT INTO contexts (id, kind, name, project_key, project_path, created_at, updated_at)\n             VALUES (?1, 'project', ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![context.id.to_string(), context.name, project_key, project_path],
        )?;
        Ok(context)
    }

    pub(crate) fn get(&self, id: ContextId) -> Result<Option<ContextRef>, StorageError> {
        Ok(self
            .connection
            .query_row(
                "SELECT id, kind, name FROM contexts WHERE id = ?1",
                [id.to_string()],
                decode_context,
            )
            .optional()?)
    }

    pub(crate) fn list(
        &self,
        kind: Option<ContextKind>,
        query: Option<&str>,
        limit: u16,
    ) -> Result<Vec<ContextRef>, StorageError> {
        let kind = kind.map(context_kind_name);
        let query = query.map(escaped_like_pattern);
        let mut statement = self.connection.prepare(
            "SELECT id, kind, name\n             FROM contexts\n             WHERE (?1 IS NULL OR kind = ?1)\n               AND (?2 IS NULL OR name LIKE ?2 ESCAPE '\\')\n             ORDER BY name COLLATE NOCASE ASC, id ASC\n             LIMIT ?3",
        )?;
        let contexts = statement
            .query_map(params![kind, query, i64::from(limit)], decode_context)?
            .collect::<Result<_, _>>()?;
        Ok(contexts)
    }
}

fn context_kind_name(kind: ContextKind) -> &'static str {
    match kind {
        ContextKind::Project => "project",
        ContextKind::Standalone => "standalone",
    }
}

fn escaped_like_pattern(query: &str) -> String {
    let mut escaped = String::with_capacity(query.len() + 2);
    escaped.push('%');
    for character in query.chars() {
        if matches!(character, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped.push('%');
    escaped
}

fn decode_context(row: &Row<'_>) -> rusqlite::Result<ContextRef> {
    let id = ContextId::from_str(&row.get::<_, String>(0)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error))
    })?;
    let kind_value: String = row.get(1)?;
    let kind = match kind_value.as_str() {
        "project" => ContextKind::Project,
        "standalone" => ContextKind::Standalone,
        _ => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                1,
                Type::Text,
                Box::new(InvalidContextKind(kind_value)),
            ));
        }
    };
    Ok(ContextRef {
        id,
        kind,
        name: row.get(2)?,
    })
}

#[derive(Debug)]
struct InvalidContextKind(String);

impl fmt::Display for InvalidContextKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid stored context kind: {}", self.0)
    }
}

impl Error for InvalidContextKind {}

#[cfg(test)]
mod tests {
    use crate::{
        contract::ContextKind,
        storage::{Database, contexts::ContextRepository},
    };

    #[test]
    fn standalone_context_round_trips_without_project_data() {
        let database = Database::open_in_memory().unwrap();
        let repository = ContextRepository::new(database.connection());

        let created = repository.create_standalone("Personal notes").unwrap();
        let listed = repository.list(None, None, 100).unwrap();
        let (project_key, project_path): (Option<String>, Option<String>) = database
            .connection()
            .query_row(
                "SELECT project_key, project_path FROM contexts WHERE id = ?1",
                [created.id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(created.kind, ContextKind::Standalone);
        assert_eq!(listed, vec![created]);
        assert_eq!((project_key, project_path), (None, None));
    }

    #[test]
    fn project_identity_reuses_one_context() {
        let database = Database::open_in_memory().unwrap();
        let repository = ContextRepository::new(database.connection());

        let first = repository
            .create_project("Lyn", Some("stable-key"), "/work/lyn")
            .unwrap();
        let second = repository
            .create_project("Renamed attempt", Some("stable-key"), "/work/lyn-copy")
            .unwrap();

        assert_eq!(second, first);
        assert_eq!(
            repository
                .list(Some(ContextKind::Project), None, 100)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn list_filters_query_and_limit_with_stable_order() {
        let database = Database::open_in_memory().unwrap();
        let repository = ContextRepository::new(database.connection());
        repository.create_standalone("Zulu").unwrap();
        repository.create_standalone("Alpha's notes").unwrap();
        repository.create_standalone("Budget %").unwrap();
        repository
            .create_project("Alpha project", None, "/work/alpha")
            .unwrap();

        let standalone = repository
            .list(Some(ContextKind::Standalone), Some("Alpha's"), 10)
            .unwrap();
        let limited = repository.list(None, Some("Alpha"), 1).unwrap();
        let literal_wildcard = repository.list(None, Some("%"), 10).unwrap();

        assert_eq!(standalone.len(), 1);
        assert_eq!(standalone[0].name, "Alpha's notes");
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].name, "Alpha project");
        assert_eq!(literal_wildcard.len(), 1);
        assert_eq!(literal_wildcard[0].name, "Budget %");
    }
}
