//! Read-only status for the Library. No recovery, installation or service call.

use super::{
    directories,
    service::OwnedActivation,
    transaction::{ActivationFilesState, inspect_activation_files},
};
use crate::{
    context::gnome_broker::NativeConnectionStatus,
    contract::{
        GnomeActivationEvidence, GnomeActivationFilesStatus, GnomeActivationPhase,
        GnomeConnectionStatus, GnomeIntegrationEvidence, IntegrationId, IntegrationStatus,
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

fn phase(phase: GnomeActivationPhase) -> GnomeActivationEvidence {
    GnomeActivationEvidence {
        phase,
        revision: None,
        actions: None,
    }
}

pub(super) fn activation(
    state: &Path,
    target: &Path,
    files: ActivationFilesState,
) -> GnomeActivationEvidence {
    use GnomeActivationPhase as Phase;
    fn owned(
        phase: Phase,
        result: io::Result<OwnedActivation>,
    ) -> io::Result<GnomeActivationEvidence> {
        let owned = result?;
        let actions = super::availability::actions(&owned, phase);
        Ok(GnomeActivationEvidence {
            phase,
            revision: Some(owned.management_revision()?),
            actions: Some(actions),
        })
    }
    let result = match files {
        ActivationFilesState::Unavailable => return phase(Phase::Unavailable),
        ActivationFilesState::Changed => return phase(Phase::Changed),
        ActivationFilesState::Owned => {
            owned(Phase::Configured, OwnedActivation::open(state, target))
        }
        ActivationFilesState::Absent => {
            let directory = match directories::open_private(state, false) {
                Ok(directory) => directory,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return phase(Phase::Absent);
                }
                Err(_) => return phase(Phase::Unavailable),
            };
            match directory.require_absent(super::transaction::RETIRED_REMOVAL) {
                Ok(()) => return phase(Phase::Absent),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => owned(
                    Phase::AwaitingRemovalConfirmation,
                    OwnedActivation::open_retired(state, target),
                ),
                Err(_) => return phase(Phase::Unavailable),
            }
        }
        ActivationFilesState::Pending => {
            type Open = fn(&Path, &Path) -> io::Result<OwnedActivation>;
            let candidates: [(Phase, Open); 4] = [
                (Phase::Installing, OwnedActivation::open_pending_install),
                (
                    Phase::FinishingUpdate,
                    OwnedActivation::open_committed_update,
                ),
                (
                    Phase::RestoringUpdate,
                    OwnedActivation::open_uncommitted_update,
                ),
                (Phase::Removing, OwnedActivation::open_pending_removal),
            ];
            let mut unavailable = false;
            for (phase, open) in candidates {
                match owned(phase, open(state, target)) {
                    Ok(evidence) => return evidence,
                    Err(error) => {
                        unavailable |= !matches!(
                            error.kind(),
                            io::ErrorKind::NotFound
                                | io::ErrorKind::PermissionDenied
                                | io::ErrorKind::AlreadyExists
                                | io::ErrorKind::InvalidData
                        )
                    }
                }
            }
            return phase(if unavailable {
                Phase::Unavailable
            } else {
                Phase::Changed
            });
        }
    };
    result.unwrap_or_else(|error| {
        phase(
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound
                    | io::ErrorKind::PermissionDenied
                    | io::ErrorKind::AlreadyExists
                    | io::ErrorKind::InvalidData
            ) {
                Phase::Changed
            } else {
                Phase::Unavailable
            },
        )
    })
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
            activation: None,
        }),
    }
}

pub(crate) fn observe(home: &Path, connection: NativeConnectionStatus) -> IntegrationStatus {
    let data = std::env::var_os("XDG_DATA_HOME");
    let config = std::env::var_os("XDG_CONFIG_HOME");
    let (files, activation) = locations(home, data.as_deref(), config.as_deref()).map_or(
        (
            ActivationFilesState::Unavailable,
            phase(GnomeActivationPhase::Unavailable),
        ),
        |(state, target)| {
            let files = files(&state, &target);
            (files, activation(&state, &target, files))
        },
    );
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
    let mut status = compose(files, connection, detected);
    status
        .native
        .as_mut()
        .expect("GNOME evidence is always present")
        .activation = Some(activation);
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_inspection_distinguishes_owned_intents_without_advancing_them() {
        use super::super::{
            render_activation,
            transaction::{ActivationTransaction, LAUNCHER},
        };
        use std::{fs, os::unix::fs::PermissionsExt};
        for phase in [
            "configured",
            "installing",
            "finishing_update",
            "restoring_update",
            "removing",
            "awaiting_removal_confirmation",
        ] {
            let root = tempfile::tempdir().unwrap();
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
            let state_path = root.path().join("state");
            let target_path = root.path().join("units/gnome-terminal-server.service.d");
            let state = directories::open_private(&state_path, true).unwrap();
            let target = directories::open_drop_ins(&target_path, true).unwrap();
            let digest = "0".repeat(64);
            let rendered = render_activation(
                &state_path
                    .join("modules")
                    .join(&digest)
                    .join("liblyn-gnome-native.so"),
                &state_path.join(LAUNCHER),
            )
            .unwrap();
            let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
            if phase == "installing" {
                transaction.interrupt_install_after(1);
                assert!(transaction.apply().is_err());
            } else {
                transaction.apply().unwrap();
                if matches!(phase, "finishing_update" | "restoring_update") {
                    let next = render_activation(
                        &state_path
                            .join("modules")
                            .join("1".repeat(64))
                            .join("liblyn-gnome-native.so"),
                        &state_path.join(LAUNCHER),
                    )
                    .unwrap();
                    if phase == "restoring_update" {
                        transaction.interrupt_uncommitted_update_after(8, false);
                        assert!(transaction.update(next).is_err());
                    } else {
                        transaction.update(next).unwrap();
                        transaction.interrupt_committed_update_after(1, false);
                        assert!(transaction.finish_update().is_err());
                    }
                } else if matches!(phase, "removing" | "awaiting_removal_confirmation") {
                    if phase == "removing" {
                        transaction.interrupt_removal_after(13, false);
                    }
                    let result = transaction.retire_activation(&state_path, &digest, None);
                    assert_eq!(result.is_err(), phase == "removing");
                }
            }
            drop(transaction);
            let before = (
                fs::read_dir(&state_path).unwrap().count(),
                fs::read_dir(&target_path).unwrap().count(),
            );
            let observed = activation(&state_path, &target_path, files(&state_path, &target_path));
            let payload = serde_json::to_value(observed).unwrap();
            assert_eq!(payload["phase"], phase);
            assert_eq!(payload["revision"].as_str().unwrap().len(), 64);
            assert_eq!(
                before,
                (
                    fs::read_dir(&state_path).unwrap().count(),
                    fs::read_dir(&target_path).unwrap().count()
                )
            );
            if phase == "configured" {
                let retired = state_path.join(super::super::transaction::RETIRED_REMOVAL);
                fs::write(&retired, b"unknown retirement authority").unwrap();
                fs::set_permissions(&retired, fs::Permissions::from_mode(0o600)).unwrap();
                let conflicted =
                    activation(&state_path, &target_path, files(&state_path, &target_path));
                assert_eq!(conflicted, super::phase(GnomeActivationPhase::Changed));
            }
        }
    }

    #[test]
    fn missing_or_unrecognized_activation_has_no_revision_and_does_not_create_state() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let state = root.path().join("state");
        let target = root.path().join("target");
        assert_eq!(
            activation(&state, &target, files(&state, &target)),
            phase(GnomeActivationPhase::Absent)
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        directories::open_private(&state, true).unwrap();
        directories::open_drop_ins(&target, true).unwrap();
        let pending = state.join("update-precommit-pending-v1.json");
        fs::write(&pending, b"unrecognized").unwrap();
        fs::set_permissions(&pending, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            activation(&state, &target, files(&state, &target)),
            phase(GnomeActivationPhase::Changed)
        );
        assert_eq!(fs::read(&pending).unwrap(), b"unrecognized");
    }

    #[test]
    fn retirement_revision_refuses_live_replacement_and_changes_for_identical_fresh_records() {
        use super::super::{
            render_activation,
            transaction::{ActivationTransaction, LAUNCHER, RETIRED_REMOVAL},
        };
        use std::{fs, os::unix::fs::PermissionsExt};
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let state_path = root.path().join("state");
        let target_path = root.path().join("target");
        let state = directories::open_private(&state_path, true).unwrap();
        let target = directories::open_drop_ins(&target_path, true).unwrap();
        let digest = "0".repeat(64);
        let rendered = render_activation(
            &state_path
                .join("modules")
                .join(&digest)
                .join("liblyn-gnome-native.so"),
            &state_path.join(LAUNCHER),
        )
        .unwrap();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        transaction
            .retire_activation(&state_path, &digest, None)
            .unwrap();
        drop(transaction);
        let live = OwnedActivation::open_retired(&state_path, &target_path).unwrap();
        let before = live.management_revision().unwrap();
        let record = state_path.join(RETIRED_REMOVAL);
        let content = fs::read(&record).unwrap();
        fs::rename(&record, state_path.join("preserved-record")).unwrap();
        fs::write(&record, content).unwrap();
        fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(live.management_revision().is_err());
        let fresh = activation(&state_path, &target_path, files(&state_path, &target_path));
        assert_eq!(
            fresh.phase,
            GnomeActivationPhase::AwaitingRemovalConfirmation
        );
        assert_ne!(fresh.revision.as_deref(), Some(before.as_str()));
        let payload = serde_json::to_string(&fresh).unwrap();
        assert!(!payload.contains(root.path().to_str().unwrap()));
    }

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
