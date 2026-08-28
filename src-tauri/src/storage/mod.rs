//! SQLite ownership and repository implementations.

use std::{error::Error, fmt, fs, path::Path};

use rusqlite::{Connection, TransactionBehavior, params};

const INITIAL_SCHEMA: &str = include_str!("../../migrations/0001_initial.sql");
const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "initial_schema",
    sql: INITIAL_SCHEMA,
}];

#[derive(Debug, Clone, Copy)]
struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

#[derive(Debug)]
pub(crate) enum StorageError {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    InvalidMigrationPlan,
    MigrationHistoryMismatch,
    SchemaTooNew { found: i64, latest: i64 },
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(_) => formatter.write_str("storage directory is unavailable"),
            Self::Sql(_) => formatter.write_str("SQLite storage operation failed"),
            Self::InvalidMigrationPlan => {
                formatter.write_str("migration plan is not strictly ordered")
            }
            Self::MigrationHistoryMismatch => {
                formatter.write_str("database migration history does not match this build")
            }
            Self::SchemaTooNew { found, latest } => write!(
                formatter,
                "database schema version {found} is newer than supported version {latest}"
            ),
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Sql(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}

pub(crate) struct Database {
    // Tauri owns this connection for the application lifetime; repositories
    // begin borrowing it in the next storage slice.
    #[allow(dead_code, reason = "kept alive as managed application state")]
    connection: Connection,
}

impl Database {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        Self::initialize(Connection::open(path)?)
    }

    #[cfg(test)]
    fn open_in_memory() -> Result<Self, StorageError> {
        Self::initialize(Connection::open_in_memory()?)
    }

    fn initialize(mut connection: Connection) -> Result<Self, StorageError> {
        connection.pragma_update(None, "foreign_keys", true)?;
        let foreign_keys: i64 =
            connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
        if foreign_keys != 1 {
            return Err(StorageError::Sql(rusqlite::Error::ExecuteReturnedResults));
        }
        apply_migrations(&mut connection, MIGRATIONS)?;
        Ok(Self { connection })
    }

    #[cfg(test)]
    fn connection(&self) -> &Connection {
        &self.connection
    }
}

fn apply_migrations(
    connection: &mut Connection,
    migrations: &[Migration],
) -> Result<(), StorageError> {
    if migrations.iter().any(|migration| migration.version <= 0)
        || migrations
            .windows(2)
            .any(|pair| pair[0].version >= pair[1].version)
    {
        return Err(StorageError::InvalidMigrationPlan);
    }

    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (\n            version INTEGER PRIMARY KEY NOT NULL,\n            name TEXT NOT NULL,\n            applied_at TEXT NOT NULL\n        );",
    )?;

    let applied: Vec<(i64, String)> = connection
        .prepare("SELECT version, name FROM schema_migrations ORDER BY version")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let latest = migrations.last().map_or(0, |migration| migration.version);
    if let Some((found, _)) = applied.last().filter(|(version, _)| *version > latest) {
        return Err(StorageError::SchemaTooNew {
            found: *found,
            latest,
        });
    }
    if applied
        .iter()
        .zip(migrations)
        .any(|((version, name), migration)| *version != migration.version || name != migration.name)
    {
        return Err(StorageError::MigrationHistoryMismatch);
    }

    for migration in migrations.iter().skip(applied.len()) {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(migration.sql)?;
        transaction.execute(
            "INSERT INTO schema_migrations (version, name, applied_at)\n             VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![migration.version, migration.name],
        )?;
        transaction.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use tempfile::tempdir;

    use super::{Database, Migration, StorageError, apply_migrations};

    #[test]
    fn fresh_database_has_canonical_schema_and_foreign_keys() {
        let database = Database::open_in_memory().expect("database initializes");
        let connection = database.connection();

        let foreign_keys: i64 = connection
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        assert_eq!(foreign_keys, 1);

        for table in [
            "contexts",
            "captures",
            "media_assets",
            "enrichment_jobs",
            "captures_fts",
            "settings",
            "schema_migrations",
        ] {
            let exists: bool = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = ?1)",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(exists, "missing {table}");
        }
    }

    #[test]
    fn reopening_preserves_data_and_does_not_repeat_migrations() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("lyn.db");
        {
            let database = Database::open(&path).unwrap();
            database
                .connection()
                .execute(
                    "INSERT INTO settings (key, value_json, updated_at) VALUES (?1, ?2, ?3)",
                    ("theme", "\"dark\"", "2026-08-28T10:30:00Z"),
                )
                .unwrap();
        }

        let reopened = Database::open(&path).unwrap();
        let migration_count: i64 = reopened
            .connection()
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let theme: String = reopened
            .connection()
            .query_row(
                "SELECT value_json FROM settings WHERE key = 'theme'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(migration_count, 1);
        assert_eq!(theme, "\"dark\"");
    }

    #[test]
    fn ordered_upgrade_applies_each_version_once() {
        let mut connection = Connection::open_in_memory().unwrap();
        let migrations = [
            Migration {
                version: 1,
                name: "first",
                sql: "CREATE TABLE first (id INTEGER PRIMARY KEY);",
            },
            Migration {
                version: 2,
                name: "second",
                sql: "CREATE TABLE second (id INTEGER PRIMARY KEY);",
            },
        ];

        apply_migrations(&mut connection, &migrations).unwrap();
        apply_migrations(&mut connection, &migrations).unwrap();

        let versions: Vec<i64> = connection
            .prepare("SELECT version FROM schema_migrations ORDER BY version")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(versions, vec![1, 2]);
    }

    #[test]
    fn failed_migration_rolls_back_schema_and_version_record() {
        let mut connection = Connection::open_in_memory().unwrap();
        let migrations = [
            Migration {
                version: 1,
                name: "durable",
                sql: "CREATE TABLE durable (value TEXT); INSERT INTO durable VALUES ('preserved');",
            },
            Migration {
                version: 2,
                name: "broken",
                sql: "CREATE TABLE should_rollback (id INTEGER); INVALID SQL;",
            },
        ];

        assert!(apply_migrations(&mut connection, &migrations).is_err());
        let table_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'should_rollback')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let migration_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(!table_exists);
        let preserved: String = connection
            .query_row("SELECT value FROM durable", [], |row| row.get(0))
            .unwrap();
        assert_eq!(migration_count, 1);
        assert_eq!(preserved, "preserved");
    }

    #[test]
    fn fts_projection_tracks_canonical_capture_text() {
        let database = Database::open_in_memory().unwrap();
        let connection = database.connection();
        connection
            .execute(
                "INSERT INTO contexts (id, kind, name, project_key, project_path, created_at, updated_at)\n                 VALUES (?1, 'standalone', 'Notes', NULL, NULL, ?2, ?2)",
                ("11111111-1111-4111-8111-111111111111", "2026-08-28T10:30:00Z"),
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO captures (id, session_id, context_id, kind, text_body, caption, caption_source,\n                    caption_revision, branch_name, source_app, source_window_title, captured_at, updated_at)\n                 VALUES (?1, ?2, ?3, 'text', 'alpha needle', NULL, NULL, 0, NULL, NULL, NULL, ?4, ?4)",
                (
                    "22222222-2222-4222-8222-222222222222",
                    "33333333-3333-4333-8333-333333333333",
                    "11111111-1111-4111-8111-111111111111",
                    "2026-08-28T10:30:00Z",
                ),
            )
            .unwrap();

        let initial_matches: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM captures_fts WHERE captures_fts MATCH 'needle'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        connection
            .execute(
                "UPDATE captures SET text_body = 'beta replacement', updated_at = ?2 WHERE id = ?1",
                (
                    "22222222-2222-4222-8222-222222222222",
                    "2026-08-28T10:31:00Z",
                ),
            )
            .unwrap();
        let old_matches: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM captures_fts WHERE captures_fts MATCH 'needle'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let new_matches: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM captures_fts WHERE captures_fts MATCH 'replacement'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(initial_matches, 1);
        assert_eq!(old_matches, 0);
        assert_eq!(new_matches, 1);
    }

    #[test]
    fn schema_constraints_reject_invalid_canonical_rows() {
        let database = Database::open_in_memory().unwrap();
        let connection = database.connection();

        let standalone_with_path = connection.execute(
            "INSERT INTO contexts (id, kind, name, project_key, project_path, created_at, updated_at)\n             VALUES (?1, 'standalone', 'Invalid', NULL, '/private/work', ?2, ?2)",
            ("11111111-1111-4111-8111-111111111111", "2026-08-28T10:30:00Z"),
        );
        assert!(standalone_with_path.is_err());

        let capture_without_context = connection.execute(
            "INSERT INTO captures (id, session_id, context_id, kind, text_body, caption_revision, captured_at, updated_at)\n             VALUES (?1, ?2, ?3, 'text', 'draft', 0, ?4, ?4)",
            (
                "22222222-2222-4222-8222-222222222222",
                "33333333-3333-4333-8333-333333333333",
                "44444444-4444-4444-8444-444444444444",
                "2026-08-28T10:30:00Z",
            ),
        );
        assert!(capture_without_context.is_err());

        connection
            .execute(
                "INSERT INTO contexts (id, kind, name, created_at, updated_at)\n                 VALUES (?1, 'standalone', 'Notes', ?2, ?2)",
                ("55555555-5555-4555-8555-555555555555", "2026-08-28T10:30:00Z"),
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO captures (id, session_id, context_id, kind, text_body, caption_revision, captured_at, updated_at)\n                 VALUES (?1, ?2, ?3, 'image', NULL, 0, ?4, ?4)",
                (
                    "66666666-6666-4666-8666-666666666666",
                    "77777777-7777-4777-8777-777777777777",
                    "55555555-5555-4555-8555-555555555555",
                    "2026-08-28T10:30:00Z",
                ),
            )
            .unwrap();
        let mismatched_media = connection.execute(
            "INSERT INTO media_assets (id, capture_id, kind, relative_path, mime_type, byte_size, checksum, duration_ms, created_at)\n             VALUES (?1, ?2, 'audio', 'audio/example.wav', 'audio/wav', 512, 'checksum', 20, ?3)",
            (
                "88888888-8888-4888-8888-888888888888",
                "66666666-6666-4666-8666-666666666666",
                "2026-08-28T10:30:00Z",
            ),
        );
        assert!(mismatched_media.is_err());
    }

    #[test]
    fn schema_newer_than_binary_is_rejected() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL);\n                 INSERT INTO schema_migrations VALUES (99, 'future', '2026-08-28T10:30:00Z');",
            )
            .unwrap();
        let migrations = [Migration {
            version: 1,
            name: "known",
            sql: "CREATE TABLE known (id INTEGER);",
        }];

        assert!(matches!(
            apply_migrations(&mut connection, &migrations),
            Err(StorageError::SchemaTooNew {
                found: 99,
                latest: 1
            })
        ));
    }
}
