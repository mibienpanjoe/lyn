//! Management, status detection, and 1-click installation of local context integrations.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::contract::{
    InstallIntegrationInput, InstallIntegrationResult, IntegrationId, IntegrationStatus,
};

const EXTENSION_PACKAGE_JSON: &str = include_str!("../../../integrations/vscode/package.json");
const EXTENSION_CJS: &str = include_str!("../../../integrations/vscode/extension.cjs");
const OBSERVATION_CJS: &str = include_str!("../../../integrations/vscode/observation.cjs");
const EXTENSION_README: &str = include_str!("../../../integrations/vscode/README.md");

const KITTY_WATCHER_PY: &str = include_str!("../../../integrations/kitty/lyn_context_watcher.py");
const SHELL_BOOTSTRAP_SH: &str = include_str!("../../../integrations/shell/lyn-context.sh");

const BROWSER_MANIFEST_JSON: &str = include_str!("../../../integrations/browser/manifest.json");
const BROWSER_BACKGROUND_JS: &str = include_str!("../../../integrations/browser/background.js");
const BROWSER_SANITIZE_CJS: &str = include_str!("../../../integrations/browser/sanitize.cjs");

pub(crate) const CHROMIUM_EXTENSION_ID: &str = "aecihlceemkggejjmpphmnhpdcgnhife";
pub(crate) const FIREFOX_ADDON_ID: &str = "lyn-context-provider@mibienpanjoe.com";

const EXTENSION_FOLDER_NAME: &str = "mibienpanjoe.lyn-context-provider-0.1.1";
const NATIVE_HOST_NAME: &str = "com.mibienpanjoe.lyn.json";

pub(crate) fn chromium_manifest_content(host_bin_str: &str) -> String {
    format!(
        r#"{{
  "name": "com.mibienpanjoe.lyn",
  "description": "Lyn Desktop Browser Context Host",
  "path": "{}",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://{}/"
  ]
}}
"#,
        host_bin_str, CHROMIUM_EXTENSION_ID
    )
}

pub(crate) fn firefox_manifest_content(host_bin_str: &str) -> String {
    format!(
        r#"{{
  "name": "com.mibienpanjoe.lyn",
  "description": "Lyn Desktop Browser Context Host",
  "path": "{}",
  "type": "stdio",
  "allowed_extensions": [
    "{}"
  ]
}}
"#,
        host_bin_str, FIREFOX_ADDON_ID
    )
}

pub(crate) fn user_home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"))
}

fn command_exists(name: &str) -> bool {
    let Ok(path_var) = std::env::var("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path_var) {
        if dir.join(name).is_file() {
            return true;
        }
    }
    false
}

fn has_extension_installed(extensions_dir: &Path) -> bool {
    if !extensions_dir.is_dir() {
        return false;
    }
    let Ok(entries) = fs::read_dir(extensions_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("mibienpanjoe.lyn-context-provider") {
            let path = entry.path();
            if path.join("extension.cjs").is_file() || path.join("package.json").is_file() {
                return true;
            }
        }
    }
    false
}

fn install_vscode_style_extension(extensions_dir: &Path) -> Result<PathBuf, String> {
    let target_dir = extensions_dir.join(EXTENSION_FOLDER_NAME);
    fs::create_dir_all(&target_dir)
        .map_err(|e| format!("Could not create directory {}: {e}", target_dir.display()))?;

    fs::write(target_dir.join("package.json"), EXTENSION_PACKAGE_JSON)
        .map_err(|e| format!("Failed to write package.json: {e}"))?;
    fs::write(target_dir.join("extension.cjs"), EXTENSION_CJS)
        .map_err(|e| format!("Failed to write extension.cjs: {e}"))?;
    fs::write(target_dir.join("observation.cjs"), OBSERVATION_CJS)
        .map_err(|e| format!("Failed to write observation.cjs: {e}"))?;
    fs::write(target_dir.join("README.md"), EXTENSION_README)
        .map_err(|e| format!("Failed to write README.md: {e}"))?;

    Ok(target_dir)
}

pub(crate) fn list_integration_statuses(home: &Path) -> Vec<IntegrationStatus> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = home;
        Vec::new()
    }
    #[cfg(target_os = "linux")]
    vec![
        cursor_status(home),
        vscode_status(home),
        browser_status(home),
        kitty_status(home),
        shell_status(home),
    ]
}

pub(crate) fn cursor_status(home: &Path) -> IntegrationStatus {
    let cursor_dir = home.join(".cursor");
    let cursor_config = home.join(".config/Cursor");
    let detected = cursor_dir.is_dir() || cursor_config.is_dir() || command_exists("cursor");

    let extensions_dir = cursor_dir.join("extensions");
    let installed = has_extension_installed(&extensions_dir);

    let details = if installed {
        Some("Extension active in ~/.cursor/extensions/".to_owned())
    } else if detected {
        Some("Cursor detected on this system".to_owned())
    } else {
        None
    };

    IntegrationStatus {
        id: IntegrationId::Cursor,
        name: "Cursor IDE".to_owned(),
        description: "Reports the focused Cursor workspace folder to Lyn on capture.".to_owned(),
        detected,
        installed,
        details,
    }
}

pub(crate) fn vscode_status(home: &Path) -> IntegrationStatus {
    let vscode_dir = home.join(".vscode");
    let vscode_config = home.join(".config/Code");
    let detected = vscode_dir.is_dir() || vscode_config.is_dir() || command_exists("code");

    let extensions_dir = vscode_dir.join("extensions");
    let installed = has_extension_installed(&extensions_dir);

    let details = if installed {
        Some("Extension active in ~/.vscode/extensions/".to_owned())
    } else if detected {
        Some("VS Code detected on this system".to_owned())
    } else {
        None
    };

    IntegrationStatus {
        id: IntegrationId::Vscode,
        name: "Visual Studio Code".to_owned(),
        description: "Reports the focused VS Code workspace folder to Lyn on capture.".to_owned(),
        detected,
        installed,
        details,
    }
}

fn browser_manifest_paths(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".config/google-chrome/NativeMessagingHosts")
            .join(NATIVE_HOST_NAME),
        home.join(".config/chromium/NativeMessagingHosts")
            .join(NATIVE_HOST_NAME),
        home.join(".config/BraveSoftware/Brave-Browser/NativeMessagingHosts")
            .join(NATIVE_HOST_NAME),
        home.join(".config/microsoft-edge/NativeMessagingHosts")
            .join(NATIVE_HOST_NAME),
        home.join(".mozilla/native-messaging-hosts")
            .join(NATIVE_HOST_NAME),
    ]
}

pub(crate) fn browser_status(home: &Path) -> IntegrationStatus {
    let candidates = [
        home.join(".config/google-chrome"),
        home.join(".config/chromium"),
        home.join(".config/BraveSoftware/Brave-Browser"),
        home.join(".config/microsoft-edge"),
        home.join(".mozilla"),
    ];
    let detected = candidates.iter().any(|p| p.is_dir())
        || command_exists("google-chrome")
        || command_exists("chromium")
        || command_exists("brave-browser")
        || command_exists("microsoft-edge")
        || command_exists("firefox");

    let manifest_paths = browser_manifest_paths(home);
    let installed = manifest_paths.iter().any(|p| p.is_file());

    let details = if installed {
        Some("Native Messaging host registered for local browsers".to_owned())
    } else if detected {
        Some("Supported web browser detected".to_owned())
    } else {
        None
    };

    IntegrationStatus {
        id: IntegrationId::Browser,
        name: "Web Browser (Chrome, Brave, Edge, Firefox)".to_owned(),
        description: "Correlates active localhost development tabs with your repository context."
            .to_owned(),
        detected,
        installed,
        details,
    }
}

pub(crate) fn kitty_status(home: &Path) -> IntegrationStatus {
    let kitty_dir = home.join(".config/kitty");
    let detected = kitty_dir.is_dir() || command_exists("kitty");

    let conf_path = kitty_dir.join("kitty.conf");
    let installed = if conf_path.is_file() {
        fs::read_to_string(&conf_path)
            .map(|content| content.contains("lyn_context_watcher.py"))
            .unwrap_or(false)
    } else {
        false
    };

    let details = if installed {
        Some("Watcher configured in ~/.config/kitty/kitty.conf".to_owned())
    } else if detected {
        Some("Kitty detected on this system".to_owned())
    } else {
        None
    };

    IntegrationStatus {
        id: IntegrationId::Kitty,
        name: "Kitty Terminal".to_owned(),
        description: "Monitors exact focused terminal pane without inspecting commands or output."
            .to_owned(),
        detected,
        installed,
        details,
    }
}

pub(crate) fn shell_status(home: &Path) -> IntegrationStatus {
    let bashrc = home.join(".bashrc");
    let zshrc = home.join(".zshrc");

    let bash_has = bashrc.is_file()
        && fs::read_to_string(&bashrc)
            .map(|c| c.contains("lyn-context.sh"))
            .unwrap_or(false);
    let zsh_has = zshrc.is_file()
        && fs::read_to_string(&zshrc)
            .map(|c| c.contains("lyn-context.sh"))
            .unwrap_or(false);

    let installed = bash_has || zsh_has;

    let details = if installed {
        Some("Shell startup script configured".to_owned())
    } else {
        Some("Available for Bash and Zsh".to_owned())
    };

    IntegrationStatus {
        id: IntegrationId::Shell,
        name: "Terminal Shell (Bash / Zsh)".to_owned(),
        description: "Associates GNOME Terminal and external shells with current git repository."
            .to_owned(),
        detected: true,
        installed,
        details,
    }
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(path) {
            return metadata.permissions().mode() & 0o111 != 0;
        }
        false
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn locate_browser_host_binary(home: &Path) -> Option<PathBuf> {
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let candidate = parent.join("lyn-browser-host");
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    for standard in [
        home.join(".local/bin/lyn-browser-host"),
        PathBuf::from("/usr/bin/lyn-browser-host"),
        PathBuf::from("/usr/local/bin/lyn-browser-host"),
    ] {
        if is_executable_file(&standard) {
            return Some(standard);
        }
    }

    None
}

pub(crate) fn install_integration_by_id(
    home: &Path,
    input: InstallIntegrationInput,
) -> InstallIntegrationResult {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = home;
        InstallIntegrationResult {
            id: input.id,
            success: false,
            message: "Context integrations are only supported on Linux in this release.".to_owned(),
            installed: false,
        }
    }
    #[cfg(target_os = "linux")]
    match input.id {
        IntegrationId::Cursor => install_cursor(home),
        IntegrationId::Vscode => install_vscode(home),
        IntegrationId::Browser => install_browser(home),
        IntegrationId::Kitty => install_kitty(home),
        IntegrationId::Shell => install_shell(home),
    }
}

fn install_cursor(home: &Path) -> InstallIntegrationResult {
    let cursor_ext_dir = home.join(".cursor/extensions");
    match install_vscode_style_extension(&cursor_ext_dir) {
        Ok(path) => InstallIntegrationResult {
            id: IntegrationId::Cursor,
            success: true,
            message: format!(
                "Extension installed successfully to {}. Please reload your Cursor window to activate.",
                path.display()
            ),
            installed: true,
        },
        Err(err) => InstallIntegrationResult {
            id: IntegrationId::Cursor,
            success: false,
            message: err,
            installed: has_extension_installed(&cursor_ext_dir),
        },
    }
}

fn install_vscode(home: &Path) -> InstallIntegrationResult {
    let vscode_ext_dir = home.join(".vscode/extensions");
    match install_vscode_style_extension(&vscode_ext_dir) {
        Ok(path) => InstallIntegrationResult {
            id: IntegrationId::Vscode,
            success: true,
            message: format!(
                "Extension installed successfully to {}. Please reload your VS Code window to activate.",
                path.display()
            ),
            installed: true,
        },
        Err(err) => InstallIntegrationResult {
            id: IntegrationId::Vscode,
            success: false,
            message: err,
            installed: has_extension_installed(&vscode_ext_dir),
        },
    }
}

fn install_browser(home: &Path) -> InstallIntegrationResult {
    let Some(host_bin) = locate_browser_host_binary(home) else {
        return InstallIntegrationResult {
            id: IntegrationId::Browser,
            success: false,
            message: "Lyn browser host binary ('lyn-browser-host') not found or is not executable."
                .to_owned(),
            installed: false,
        };
    };
    let host_bin_str = host_bin.to_string_lossy();

    let chromium_manifest = chromium_manifest_content(&host_bin_str);
    let firefox_manifest = firefox_manifest_content(&host_bin_str);

    let chromium_target_dirs = [
        home.join(".config/google-chrome/NativeMessagingHosts"),
        home.join(".config/chromium/NativeMessagingHosts"),
        home.join(".config/BraveSoftware/Brave-Browser/NativeMessagingHosts"),
        home.join(".config/microsoft-edge/NativeMessagingHosts"),
    ];

    let firefox_target_dirs = [home.join(".mozilla/native-messaging-hosts")];

    let mut registered_count = 0;
    for dir in chromium_target_dirs {
        if let Some(parent) = dir.parent() {
            if parent.is_dir() || dir.is_dir() {
                if fs::create_dir_all(&dir).is_ok() {
                    let file_path = dir.join(NATIVE_HOST_NAME);
                    if fs::write(&file_path, &chromium_manifest).is_ok() {
                        registered_count += 1;
                    }
                }
            }
        }
    }

    for dir in firefox_target_dirs {
        if let Some(parent) = dir.parent() {
            if parent.is_dir() || dir.is_dir() {
                if fs::create_dir_all(&dir).is_ok() {
                    let file_path = dir.join(NATIVE_HOST_NAME);
                    if fs::write(&file_path, &firefox_manifest).is_ok() {
                        registered_count += 1;
                    }
                }
            }
        }
    }

    // If none existed, create default directories for both Chrome and Firefox
    if registered_count == 0 {
        let mut fallback_error = None;
        let chrome_fallback = home.join(".config/google-chrome/NativeMessagingHosts");
        match fs::create_dir_all(&chrome_fallback)
            .and_then(|_| fs::write(chrome_fallback.join(NATIVE_HOST_NAME), &chromium_manifest))
        {
            Ok(_) => registered_count += 1,
            Err(e) => fallback_error = Some(e),
        }
        let firefox_fallback = home.join(".mozilla/native-messaging-hosts");
        match fs::create_dir_all(&firefox_fallback)
            .and_then(|_| fs::write(firefox_fallback.join(NATIVE_HOST_NAME), &firefox_manifest))
        {
            Ok(_) => registered_count += 1,
            Err(e) => {
                if fallback_error.is_none() {
                    fallback_error = Some(e);
                }
            }
        }

        if registered_count == 0 {
            let detail = fallback_error.map(|e| format!(": {e}")).unwrap_or_default();
            return InstallIntegrationResult {
                id: IntegrationId::Browser,
                success: false,
                message: format!(
                    "Failed to register Native Messaging host manifest in browser configuration directories{detail}"
                ),
                installed: false,
            };
        }
    }

    // Write unpacked companion extension into ~/.local/share/lyn/integrations/browser/
    let unpacked_dir = home.join(".local/share/lyn/integrations/browser");
    if let Err(e) = fs::create_dir_all(&unpacked_dir) {
        return InstallIntegrationResult {
            id: IntegrationId::Browser,
            success: false,
            message: format!("Failed to create browser companion directory: {e}"),
            installed: false,
        };
    }
    if let Err(e) = fs::write(unpacked_dir.join("manifest.json"), BROWSER_MANIFEST_JSON) {
        return InstallIntegrationResult {
            id: IntegrationId::Browser,
            success: false,
            message: format!("Failed to write extension manifest: {e}"),
            installed: false,
        };
    }
    if let Err(e) = fs::write(unpacked_dir.join("background.js"), BROWSER_BACKGROUND_JS) {
        return InstallIntegrationResult {
            id: IntegrationId::Browser,
            success: false,
            message: format!("Failed to write extension background script: {e}"),
            installed: false,
        };
    }
    if let Err(e) = fs::write(unpacked_dir.join("sanitize.cjs"), BROWSER_SANITIZE_CJS) {
        return InstallIntegrationResult {
            id: IntegrationId::Browser,
            success: false,
            message: format!("Failed to write extension sanitize script: {e}"),
            installed: false,
        };
    }

    InstallIntegrationResult {
        id: IntegrationId::Browser,
        success: true,
        message: format!(
            "Native host manifest registered for {} browser location(s). Unpacked extension saved to {}.",
            registered_count,
            unpacked_dir.display()
        ),
        installed: true,
    }
}

fn install_kitty(home: &Path) -> InstallIntegrationResult {
    let kitty_dir = home.join(".config/kitty");
    if let Err(e) = fs::create_dir_all(&kitty_dir) {
        return InstallIntegrationResult {
            id: IntegrationId::Kitty,
            success: false,
            message: format!("Failed to create kitty config directory: {e}"),
            installed: false,
        };
    }

    let watcher_file = kitty_dir.join("lyn_context_watcher.py");
    if let Err(e) = fs::write(&watcher_file, KITTY_WATCHER_PY) {
        return InstallIntegrationResult {
            id: IntegrationId::Kitty,
            success: false,
            message: format!("Failed to write watcher script: {e}"),
            installed: false,
        };
    }

    let conf_file = kitty_dir.join("kitty.conf");
    let existing_content = fs::read_to_string(&conf_file).unwrap_or_default();
    if !existing_content.contains("lyn_context_watcher.py") {
        let addition = format!(
            "\n# Lyn Context Provider watcher\nwatcher {}\n",
            watcher_file.display()
        );
        let mut new_content = existing_content;
        new_content.push_str(&addition);
        if let Err(e) = fs::write(&conf_file, new_content) {
            return InstallIntegrationResult {
                id: IntegrationId::Kitty,
                success: false,
                message: format!("Failed to update kitty.conf: {e}"),
                installed: false,
            };
        }
    }

    InstallIntegrationResult {
        id: IntegrationId::Kitty,
        success: true,
        message:
            "Kitty watcher installed and added to ~/.config/kitty/kitty.conf. Restart Kitty to activate."
                .to_owned(),
        installed: true,
    }
}

fn install_shell(home: &Path) -> InstallIntegrationResult {
    let shell_share_dir = home.join(".local/share/lyn/shell");
    if let Err(e) = fs::create_dir_all(&shell_share_dir) {
        return InstallIntegrationResult {
            id: IntegrationId::Shell,
            success: false,
            message: format!("Failed to create shell integration directory: {e}"),
            installed: false,
        };
    }

    let script_file = shell_share_dir.join("lyn-context.sh");
    if let Err(e) = fs::write(&script_file, SHELL_BOOTSTRAP_SH) {
        return InstallIntegrationResult {
            id: IntegrationId::Shell,
            success: false,
            message: format!("Failed to write shell bootstrap script: {e}"),
            installed: false,
        };
    }

    let snippet = format!(
        "\n# Lyn Context Provider\n[ -f \"{}\" ] && source \"{}\"\n",
        script_file.display(),
        script_file.display()
    );

    let bashrc = home.join(".bashrc");
    let zshrc = home.join(".zshrc");

    let has_bashrc = bashrc.is_file();
    let has_zshrc = zshrc.is_file();

    let mut configured_targets = Vec::new();

    if has_bashrc || has_zshrc {
        if has_bashrc {
            let content = match fs::read_to_string(&bashrc) {
                Ok(c) => c,
                Err(e) => {
                    return InstallIntegrationResult {
                        id: IntegrationId::Shell,
                        success: false,
                        message: format!("Failed to read ~/.bashrc: {e}"),
                        installed: false,
                    };
                }
            };
            if !content.contains("lyn-context.sh") {
                let mut updated = content;
                updated.push_str(&snippet);
                if let Err(e) = fs::write(&bashrc, updated) {
                    return InstallIntegrationResult {
                        id: IntegrationId::Shell,
                        success: false,
                        message: format!("Failed to write ~/.bashrc: {e}"),
                        installed: false,
                    };
                }
            }
            configured_targets.push("~/.bashrc");
        }
        if has_zshrc {
            let content = match fs::read_to_string(&zshrc) {
                Ok(c) => c,
                Err(e) => {
                    return InstallIntegrationResult {
                        id: IntegrationId::Shell,
                        success: false,
                        message: format!("Failed to read ~/.zshrc: {e}"),
                        installed: false,
                    };
                }
            };
            if !content.contains("lyn-context.sh") {
                let mut updated = content;
                updated.push_str(&snippet);
                if let Err(e) = fs::write(&zshrc, updated) {
                    return InstallIntegrationResult {
                        id: IntegrationId::Shell,
                        success: false,
                        message: format!("Failed to write ~/.zshrc: {e}"),
                        installed: false,
                    };
                }
            }
            configured_targets.push("~/.zshrc");
        }
    } else {
        // Neither exists: create intended startup file (~/.bashrc)
        if let Err(e) = fs::write(&bashrc, &snippet) {
            return InstallIntegrationResult {
                id: IntegrationId::Shell,
                success: false,
                message: format!("Failed to create ~/.bashrc: {e}"),
                installed: false,
            };
        }
        configured_targets.push("~/.bashrc");
    }

    InstallIntegrationResult {
        id: IntegrationId::Shell,
        success: true,
        message: format!(
            "Shell integration saved to ~/.local/share/lyn/shell/ and configured in {}. Start a new terminal session to activate.",
            configured_targets.join(" and ")
        ),
        installed: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn cursor_and_vscode_can_be_installed_into_isolated_home() {
        let temp = tempdir().unwrap();
        let home = temp.path();

        let initial_cursor = cursor_status(home);
        assert!(!initial_cursor.installed);

        let res = install_cursor(home);
        assert!(res.success);
        assert!(res.installed);

        let after_cursor = cursor_status(home);
        assert!(after_cursor.installed);

        let res_vscode = install_vscode(home);
        assert!(res_vscode.success);
        assert!(res_vscode.installed);

        let after_vscode = vscode_status(home);
        assert!(after_vscode.installed);
    }

    #[test]
    fn kitty_and_shell_configure_isolated_home() {
        let temp = tempdir().unwrap();
        let home = temp.path();

        let initial_kitty = kitty_status(home);
        assert!(!initial_kitty.installed);

        let res_kitty = install_kitty(home);
        assert!(res_kitty.success);
        assert!(res_kitty.installed);

        let after_kitty = kitty_status(home);
        assert!(after_kitty.installed);

        // Create dummy .bashrc
        let bashrc = home.join(".bashrc");
        fs::write(&bashrc, "# User bashrc\n").unwrap();

        let res_shell = install_shell(home);
        assert!(res_shell.success);
        assert!(res_shell.installed);

        let after_shell = shell_status(home);
        assert!(after_shell.installed);
        assert!(
            fs::read_to_string(&bashrc)
                .unwrap()
                .contains("lyn-context.sh")
        );
    }

    #[test]
    fn browser_registers_manifest() {
        let temp = tempdir().unwrap();
        let home = temp.path();

        let initial_browser = browser_status(home);
        assert!(!initial_browser.installed);

        let bin_dir = home.join(".local/bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let dummy_host = bin_dir.join("lyn-browser-host");
        fs::write(&dummy_host, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dummy_host, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let res = install_browser(home);
        assert!(res.success);
        assert!(res.installed);

        let after_browser = browser_status(home);
        assert!(after_browser.installed);

        // Verify Chromium manifest output
        let chrome_manifest_file = home
            .join(".config/google-chrome/NativeMessagingHosts")
            .join(NATIVE_HOST_NAME);
        assert!(chrome_manifest_file.is_file());
        let chrome_json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&chrome_manifest_file).unwrap()).unwrap();
        assert_eq!(chrome_json["name"], "com.mibienpanjoe.lyn");
        assert_eq!(chrome_json["path"], dummy_host.to_string_lossy().as_ref());
        let origins = chrome_json["allowed_origins"].as_array().unwrap();
        assert_eq!(origins.len(), 1);
        assert_eq!(
            origins[0],
            format!("chrome-extension://{}/", CHROMIUM_EXTENSION_ID)
        );
        assert!(chrome_json.get("allowed_extensions").is_none());

        // Verify Firefox manifest output
        let firefox_manifest_file = home
            .join(".mozilla/native-messaging-hosts")
            .join(NATIVE_HOST_NAME);
        assert!(firefox_manifest_file.is_file());
        let firefox_json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&firefox_manifest_file).unwrap()).unwrap();
        assert_eq!(firefox_json["name"], "com.mibienpanjoe.lyn");
        assert_eq!(firefox_json["path"], dummy_host.to_string_lossy().as_ref());
        let extensions = firefox_json["allowed_extensions"].as_array().unwrap();
        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0], FIREFOX_ADDON_ID);
        assert!(firefox_json.get("allowed_origins").is_none());

        // Verify unpacked extension manifest consistency
        let unpacked_manifest_file = home
            .join(".local/share/lyn/integrations/browser")
            .join("manifest.json");
        assert!(unpacked_manifest_file.is_file());
        let manifest_json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&unpacked_manifest_file).unwrap()).unwrap();
        assert_eq!(
            manifest_json["browser_specific_settings"]["gecko"]["id"],
            FIREFOX_ADDON_ID
        );
        assert!(manifest_json["key"].is_string());
    }

    #[test]
    fn browser_install_fails_when_host_binary_missing_or_not_executable() {
        let temp = tempdir().unwrap();
        let home = temp.path();

        // Host binary completely missing
        let res = install_browser(home);
        assert!(!res.success);
        assert!(!res.installed);

        let after_browser = browser_status(home);
        assert!(!after_browser.installed);

        // Host binary present but not executable
        let bin_dir = home.join(".local/bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let dummy_host = bin_dir.join("lyn-browser-host");
        fs::write(&dummy_host, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dummy_host, fs::Permissions::from_mode(0o644)).unwrap();
        }

        let res_non_exec = install_browser(home);
        assert!(!res_non_exec.success);
        assert!(!res_non_exec.installed);
    }

    #[test]
    fn shell_install_creates_bashrc_when_neither_exists() {
        let temp = tempdir().unwrap();
        let home = temp.path();

        let initial_shell = shell_status(home);
        assert!(!initial_shell.installed);

        let res = install_shell(home);
        assert!(res.success);
        assert!(res.installed);

        let bashrc = home.join(".bashrc");
        assert!(bashrc.is_file());
        let content = fs::read_to_string(&bashrc).unwrap();
        assert!(content.contains("lyn-context.sh"));

        let after_shell = shell_status(home);
        assert!(after_shell.installed);
    }

    #[test]
    #[cfg(unix)]
    fn shell_install_fails_when_startup_file_unwritable() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let home = temp.path();

        let bashrc = home.join(".bashrc");
        fs::write(&bashrc, "# existing bashrc\n").unwrap();
        fs::set_permissions(&bashrc, fs::Permissions::from_mode(0o444)).unwrap();

        let res = install_shell(home);
        assert!(!res.success);
        assert!(!res.installed);

        let after_shell = shell_status(home);
        assert!(!after_shell.installed);
    }

    #[test]
    #[cfg(unix)]
    fn browser_install_fails_when_companion_write_unwritable() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let home = temp.path();

        let bin_dir = home.join(".local/bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let dummy_host = bin_dir.join("lyn-browser-host");
        fs::write(&dummy_host, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&dummy_host, fs::Permissions::from_mode(0o755)).unwrap();

        // Make companion unpacked directory unwritable
        let unpacked_dir = home.join(".local/share/lyn/integrations/browser");
        fs::create_dir_all(&unpacked_dir).unwrap();
        fs::set_permissions(&unpacked_dir, fs::Permissions::from_mode(0o555)).unwrap();

        let res = install_browser(home);
        assert!(!res.success);
        assert!(!res.installed);
    }
}
