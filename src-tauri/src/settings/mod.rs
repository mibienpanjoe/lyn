use crate::{
    contract::{AppSettings, SettingsPatch},
    storage::{Database, settings},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsError {
    InvalidShortcut,
    InvalidProviderOrder,
    ShortcutConflict,
    Storage,
}

pub(crate) trait SettingsPlatform {
    fn replace_shortcut(&mut self, current: &str, next: &str) -> Result<(), ()>;
    fn apply_theme(&mut self, settings: &AppSettings);
}

pub(crate) fn update(
    database: &mut Database,
    patch: SettingsPatch,
    platform: &mut impl SettingsPlatform,
) -> Result<AppSettings, SettingsError> {
    let current = settings::load(database.connection()).map_err(|_| SettingsError::Storage)?;
    let next = AppSettings {
        global_shortcut: patch
            .global_shortcut
            .unwrap_or_else(|| current.global_shortcut.clone()),
        provider_tie_break_order: patch
            .provider_tie_break_order
            .unwrap_or_else(|| current.provider_tie_break_order.clone()),
        theme: patch.theme.unwrap_or(current.theme),
        local_speech_enabled: patch
            .local_speech_enabled
            .unwrap_or(current.local_speech_enabled),
    };
    if !settings::valid_shortcut(&next.global_shortcut) {
        return Err(SettingsError::InvalidShortcut);
    }
    if !settings::valid_provider_order(&next.provider_tie_break_order) {
        return Err(SettingsError::InvalidProviderOrder);
    }

    let shortcut_changed = current.global_shortcut != next.global_shortcut;
    let transaction = database
        .connection_mut()
        .transaction()
        .map_err(|_| SettingsError::Storage)?;
    if shortcut_changed
        && platform
            .replace_shortcut(&current.global_shortcut, &next.global_shortcut)
            .is_err()
    {
        return Err(SettingsError::ShortcutConflict);
    }
    if settings::save(&transaction, &next).is_err() || transaction.commit().is_err() {
        if shortcut_changed {
            let _ = platform.replace_shortcut(&next.global_shortcut, &current.global_shortcut);
        }
        return Err(SettingsError::Storage);
    }
    platform.apply_theme(&next);
    Ok(next)
}

#[cfg(test)]
mod tests {
    use crate::{
        contract::{ContextProviderKind, SettingsPatch, ThemeSetting},
        storage::{Database, settings::SettingsRepository},
    };

    use super::{SettingsError, SettingsPlatform, update};

    struct FakePlatform {
        shortcut: String,
        reject_next: bool,
        applied_theme: Option<ThemeSetting>,
    }

    impl SettingsPlatform for FakePlatform {
        fn replace_shortcut(&mut self, current: &str, next: &str) -> Result<(), ()> {
            assert_eq!(self.shortcut, current);
            if self.reject_next {
                self.reject_next = false;
                return Err(());
            }
            self.shortcut = next.to_owned();
            Ok(())
        }

        fn apply_theme(&mut self, settings: &crate::contract::AppSettings) {
            self.applied_theme = Some(settings.theme);
        }
    }

    fn platform(reject_next: bool) -> FakePlatform {
        FakePlatform {
            shortcut: "Control+Shift+Space".to_owned(),
            reject_next,
            applied_theme: None,
        }
    }

    #[test]
    fn conflicting_shortcut_keeps_the_last_working_configuration() {
        let mut database = Database::open_in_memory().unwrap();
        let mut platform = platform(true);

        let result = update(
            &mut database,
            SettingsPatch {
                global_shortcut: Some("Control+Alt+L".to_owned()),
                ..SettingsPatch::default()
            },
            &mut platform,
        );

        assert_eq!(result, Err(SettingsError::ShortcutConflict));
        assert_eq!(platform.shortcut, "Control+Shift+Space");
        assert_eq!(
            SettingsRepository::new(database.connection())
                .get()
                .unwrap()
                .global_shortcut,
            "Control+Shift+Space"
        );
    }

    #[test]
    fn valid_patch_persists_and_applies_theme_without_network_state() {
        let mut database = Database::open_in_memory().unwrap();
        let mut platform = platform(false);
        let order = vec![
            ContextProviderKind::Shell,
            ContextProviderKind::Vscode,
            ContextProviderKind::ForegroundWindow,
        ];

        let updated = update(
            &mut database,
            SettingsPatch {
                global_shortcut: Some("Control+Alt+L".to_owned()),
                provider_tie_break_order: Some(order.clone()),
                theme: Some(ThemeSetting::Dark),
                local_speech_enabled: Some(true),
            },
            &mut platform,
        )
        .unwrap();

        assert_eq!(updated.provider_tie_break_order, order);
        assert!(updated.local_speech_enabled);
        assert_eq!(platform.shortcut, "Control+Alt+L");
        assert_eq!(platform.applied_theme, Some(ThemeSetting::Dark));
    }
}
