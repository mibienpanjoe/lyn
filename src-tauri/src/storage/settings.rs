use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::contract::{AppSettings, ContextProviderKind};

use super::StorageError;

const GLOBAL_SHORTCUT: &str = "global_shortcut";
const PROVIDER_ORDER: &str = "provider_tie_break_order";
const THEME: &str = "theme";
const LOCAL_SPEECH: &str = "local_speech_enabled";

pub(crate) struct SettingsRepository<'a> {
    connection: &'a Connection,
}

impl<'a> SettingsRepository<'a> {
    pub(crate) fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    pub(crate) fn get(&self) -> Result<AppSettings, StorageError> {
        load(self.connection)
    }
}

pub(crate) fn load(connection: &Connection) -> Result<AppSettings, StorageError> {
    let defaults = AppSettings::default();
    Ok(AppSettings {
        global_shortcut: read(connection, GLOBAL_SHORTCUT)?.unwrap_or(defaults.global_shortcut),
        provider_tie_break_order: read(connection, PROVIDER_ORDER)?
            .unwrap_or(defaults.provider_tie_break_order),
        theme: read(connection, THEME)?.unwrap_or(defaults.theme),
        local_speech_enabled: read(connection, LOCAL_SPEECH)?
            .unwrap_or(defaults.local_speech_enabled),
    })
}

pub(crate) fn save(
    transaction: &Transaction<'_>,
    settings: &AppSettings,
) -> Result<(), StorageError> {
    write(transaction, GLOBAL_SHORTCUT, &settings.global_shortcut)?;
    write(
        transaction,
        PROVIDER_ORDER,
        &settings.provider_tie_break_order,
    )?;
    write(transaction, THEME, &settings.theme)?;
    write(transaction, LOCAL_SPEECH, &settings.local_speech_enabled)?;
    Ok(())
}

fn read<T: serde::de::DeserializeOwned>(
    connection: &Connection,
    key: &str,
) -> Result<Option<T>, StorageError> {
    let encoded = connection
        .query_row(
            "SELECT value_json FROM settings WHERE key = ?1",
            [key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    encoded
        .map(|value| {
            serde_json::from_str(&value).map_err(|_| {
                StorageError::Sql(rusqlite::Error::InvalidColumnType(
                    1,
                    key.to_owned(),
                    rusqlite::types::Type::Text,
                ))
            })
        })
        .transpose()
}

fn write<T: serde::Serialize>(
    transaction: &Transaction<'_>,
    key: &str,
    value: &T,
) -> Result<(), StorageError> {
    let encoded = serde_json::to_string(value)
        .map_err(|_| StorageError::Sql(rusqlite::Error::InvalidQuery))?;
    transaction.execute(
        "INSERT INTO settings (key, value_json, updated_at)
         VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(key) DO UPDATE SET
            value_json = excluded.value_json,
            updated_at = excluded.updated_at",
        params![key, encoded],
    )?;
    Ok(())
}

pub(crate) fn valid_provider_order(order: &[ContextProviderKind]) -> bool {
    order.len() == 3
        && [
            ContextProviderKind::Vscode,
            ContextProviderKind::Shell,
            ContextProviderKind::ForegroundWindow,
        ]
        .into_iter()
        .all(|provider| {
            order
                .iter()
                .filter(|candidate| **candidate == provider)
                .count()
                == 1
        })
        && !order.contains(&ContextProviderKind::Manual)
}

pub(crate) fn valid_shortcut(shortcut: &str) -> bool {
    !shortcut.is_empty()
        && shortcut.trim() == shortcut
        && shortcut.chars().count() <= 100
        && !shortcut.chars().any(char::is_control)
        && shortcut.split('+').all(|part| !part.is_empty())
}

#[cfg(test)]
mod tests {
    use crate::{
        contract::{AppSettings, ContextProviderKind, ThemeSetting},
        storage::Database,
    };

    use super::{SettingsRepository, save, valid_provider_order, valid_shortcut};

    #[test]
    fn defaults_are_local_safe_and_round_trip_transactionally() {
        let mut database = Database::open_in_memory().unwrap();
        let defaults = SettingsRepository::new(database.connection())
            .get()
            .unwrap();
        assert!(!defaults.local_speech_enabled);
        assert_eq!(defaults.theme, ThemeSetting::System);

        let updated = AppSettings {
            global_shortcut: "Control+Alt+L".to_owned(),
            provider_tie_break_order: vec![
                ContextProviderKind::Shell,
                ContextProviderKind::Vscode,
                ContextProviderKind::ForegroundWindow,
            ],
            theme: ThemeSetting::Dark,
            local_speech_enabled: true,
        };
        let transaction = database.connection_mut().transaction().unwrap();
        save(&transaction, &updated).unwrap();
        transaction.commit().unwrap();

        assert_eq!(
            SettingsRepository::new(database.connection())
                .get()
                .unwrap(),
            updated
        );
    }

    #[test]
    fn validates_shortcuts_and_exact_provider_permutations() {
        assert!(valid_shortcut("Control+Shift+Space"));
        assert!(!valid_shortcut(" Control+Space"));
        assert!(!valid_shortcut("Control++Space"));
        assert!(valid_provider_order(&[
            ContextProviderKind::Shell,
            ContextProviderKind::ForegroundWindow,
            ContextProviderKind::Vscode,
        ]));
        assert!(!valid_provider_order(&[
            ContextProviderKind::Shell,
            ContextProviderKind::Shell,
            ContextProviderKind::Vscode,
        ]));
    }
}
