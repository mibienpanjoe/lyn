//! Read-only status for the Library. No recovery, installation or service call.

use super::{
    directories,
    transaction::{ActivationFilesState, inspect_activation_files},
};
use crate::{
    context::gnome_broker::NativeConnectionStatus,
    contract::{
        GnomeActivationFilesStatus, GnomeConnectionStatus, GnomeIntegrationEvidence, IntegrationId,
        IntegrationStatus,
    },
};
use std::{
    ffi::OsStr,
    io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

pub(super) fn locations(
    home: &Path,
    data: Option<&OsStr>,
    config: Option<&OsStr>,
) -> Option<(PathBuf, PathBuf)> {
    let data = data
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let config = config
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    if !home.is_absolute() || !data.is_absolute() || !config.is_absolute() {
        return None;
    }
    Some((
        data.join("lyn/gnome-native"),
        config.join("systemd/user/gnome-terminal-server.service.d"),
    ))
}

fn files(state: &Path, target: &Path) -> ActivationFilesState {
    let state = match directories::open_private(state, false) {
        Ok(directory) => Some(directory),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(_) => return ActivationFilesState::Unavailable,
    };
    let target = match directories::open_drop_ins(target, false) {
        Ok(directory) => Some(directory),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(_) => return ActivationFilesState::Unavailable,
    };
    inspect_activation_files(state.as_ref(), target.as_ref())
}

fn compose(
    files: ActivationFilesState,
    connection: NativeConnectionStatus,
    detected: bool,
) -> IntegrationStatus {
    IntegrationStatus {
        id: IntegrationId::Gnome,
        name: "GNOME Terminal".to_owned(),
        description: "Associates captures with the active local GNOME terminal project.".to_owned(),
        detected,
        // These observations alone cannot prove that a packaged native module
        // is installed. Distribution and installer wiring are still separate.
        installed: false,
        details: None,
        native: Some(GnomeIntegrationEvidence {
            files: match files {
                ActivationFilesState::Absent => GnomeActivationFilesStatus::Absent,
                ActivationFilesState::Owned => GnomeActivationFilesStatus::Owned,
                ActivationFilesState::Changed => GnomeActivationFilesStatus::Changed,
                ActivationFilesState::Pending => GnomeActivationFilesStatus::Pending,
                ActivationFilesState::Unavailable => GnomeActivationFilesStatus::Unavailable,
            },
            connection: match connection {
                NativeConnectionStatus::Connected => GnomeConnectionStatus::Connected,
                NativeConnectionStatus::Disconnected => GnomeConnectionStatus::Disconnected,
                NativeConnectionStatus::Busy => GnomeConnectionStatus::Busy,
                NativeConnectionStatus::Unavailable => GnomeConnectionStatus::Unavailable,
            },
            can_install: false,
        }),
    }
}

pub(crate) fn observe(home: &Path, connection: NativeConnectionStatus) -> IntegrationStatus {
    let data = std::env::var_os("XDG_DATA_HOME");
    let config = std::env::var_os("XDG_CONFIG_HOME");
    let files = locations(home, data.as_deref(), config.as_deref())
        .map_or(ActivationFilesState::Unavailable, |(state, target)| {
            files(&state, &target)
        });
    let detected = connection == NativeConnectionStatus::Connected
        || [
            "/usr/libexec/gnome-terminal-server",
            "/usr/lib/gnome-terminal/gnome-terminal-server",
        ]
        .iter()
        .any(|path| {
            std::fs::metadata(path)
                .is_ok_and(|file| file.is_file() && file.uid() == 0 && file.mode() & 0o022 == 0)
        });
    compose(files, connection, detected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_locations_are_internal_and_relative_roots_refuse() {
        let home = Path::new("/home/test");
        assert_eq!(
            locations(home, None, None),
            Some((
                home.join(".local/share/lyn/gnome-native"),
                home.join(".config/systemd/user/gnome-terminal-server.service.d")
            ))
        );
        assert_eq!(
            locations(home, Some(OsStr::new("")), Some(OsStr::new(""))),
            locations(home, None, None)
        );
        assert_eq!(locations(home, Some(OsStr::new("relative")), None), None);
        assert_eq!(locations(home, None, Some(OsStr::new("relative"))), None);
    }

    #[test]
    fn inspecting_missing_locations_creates_nothing() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let state = root.path().join("state");
        let target = root.path().join("target");
        assert_eq!(files(&state, &target), ActivationFilesState::Absent);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn file_ownership_or_connection_does_not_claim_installation_or_capture() {
        for files in [
            ActivationFilesState::Absent,
            ActivationFilesState::Owned,
            ActivationFilesState::Changed,
            ActivationFilesState::Pending,
            ActivationFilesState::Unavailable,
        ] {
            for connection in [
                NativeConnectionStatus::Connected,
                NativeConnectionStatus::Disconnected,
                NativeConnectionStatus::Busy,
                NativeConnectionStatus::Unavailable,
            ] {
                let status = compose(files, connection, true);
                assert!(!status.installed && !status.native.as_ref().unwrap().can_install);
                let payload = serde_json::to_string(&status).unwrap();
                assert!(
                    !payload.contains("/home/")
                        && !payload.contains("sourceId")
                        && !payload.contains("serverPid")
                );
            }
        }
    }
}
