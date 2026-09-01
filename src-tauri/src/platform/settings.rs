use tauri::AppHandle;

use crate::{
    contract::{AppSettings, ThemeSetting},
    settings::SettingsPlatform,
};

#[cfg(desktop)]
use tauri_plugin_global_shortcut::GlobalShortcutExt;

pub(crate) struct NativeSettingsPlatform {
    app: AppHandle,
}

impl NativeSettingsPlatform {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl SettingsPlatform for NativeSettingsPlatform {
    fn replace_shortcut(&mut self, current: &str, next: &str) -> Result<(), ()> {
        #[cfg(desktop)]
        {
            self.app
                .global_shortcut()
                .unregister(current)
                .map_err(|_| ())?;
            if self.app.global_shortcut().register(next).is_err() {
                let _ = self.app.global_shortcut().register(current);
                return Err(());
            }
            Ok(())
        }
        #[cfg(not(desktop))]
        {
            let _ = (current, next);
            Err(())
        }
    }

    fn apply_theme(&mut self, settings: &AppSettings) {
        let theme = match settings.theme {
            ThemeSetting::System => None,
            ThemeSetting::Light => Some(tauri::Theme::Light),
            ThemeSetting::Dark => Some(tauri::Theme::Dark),
        };
        self.app.set_theme(theme);
    }
}
