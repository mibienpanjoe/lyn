//! Persistent authority for cleanup after an explicitly committed update.

use super::super::{ActivationTransaction, DROP_IN, LAUNCHER, MANIFEST, Manifest, Place, refused};
use super::{EXCHANGE_NAMES, PRECOMMIT_PENDING};
use crate::integrations::gnome::{
    lock::InstallationLock,
    managed::{ManagedDirectory, OwnedFile, PublishFailure},
    ownership::Receipt,
};
use serde::{Deserialize, Serialize};
use std::{io, os::unix::fs::MetadataExt};

pub(in crate::integrations::gnome::transaction) const CLEANUP_PENDING: &str =
    "update-cleanup-pending-v1.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSet {
    launcher: Receipt,
    drop_in: Receipt,
    manifest: Receipt,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CleanupRecord {
    version: u8,
    current: FileSet,
    backups: FileSet,
    #[serde(default)]
    precommit: Option<Receipt>,
    #[serde(default)]
    exchanges: Vec<Option<Receipt>>,
}

pub(super) fn candidate(receipt: &Receipt, mode: u32) -> bool {
    let name = receipt.filename();
    receipt.is_for(name, mode)
        && name.strip_prefix(".lyn-update-").is_some_and(|suffix| {
            uuid::Uuid::parse_str(suffix).is_ok_and(|nonce| nonce.to_string() == suffix)
        })
}

impl CleanupRecord {
    fn validate(&self) -> io::Result<()> {
        if self.version != 1
            || ![0, 2, 3].contains(&self.exchanges.len())
            || !self.current.launcher.is_for(LAUNCHER, 0o700)
            || !self.current.drop_in.is_for(DROP_IN, 0o600)
            || !self.current.manifest.is_for(MANIFEST, 0o600)
            || !candidate(&self.backups.launcher, 0o700)
            || !candidate(&self.backups.drop_in, 0o600)
            || !candidate(&self.backups.manifest, 0o600)
            || self.backups.launcher.filename() == self.backups.drop_in.filename()
            || self.backups.launcher.filename() == self.backups.manifest.filename()
            || self.backups.drop_in.filename() == self.backups.manifest.filename()
            || self
                .precommit
                .as_ref()
                .is_some_and(|receipt| !receipt.is_for(PRECOMMIT_PENDING, 0o600))
        {
            return Err(refused());
        }
        for (role, receipt) in self.exchanges.iter().enumerate() {
            if receipt.as_ref().is_some_and(|receipt| {
                self.precommit.is_none() || !receipt.is_for(EXCHANGE_NAMES[role], 0o600)
            }) {
                return Err(refused());
            }
        }
        Ok(())
    }
}

fn validate_manifest(
    state: &ManagedDirectory,
    drop_ins: &ManagedDirectory,
    file: &OwnedFile,
) -> io::Result<()> {
    let manifest: Manifest = serde_json::from_slice(&file.content).map_err(|_| refused())?;
    if manifest.version != 1
        || !manifest.launcher.is_for(LAUNCHER, 0o700)
        || !manifest.drop_in.is_for(DROP_IN, 0o600)
    {
        return Err(refused());
    }
    let _launcher = manifest.launcher.reopen(state)?;
    let _drop_in = manifest.drop_in.reopen(drop_ins)?;
    Ok(())
}

impl ActivationTransaction<'_> {
    pub(super) fn prepare_cleanup_record(&mut self) -> io::Result<()> {
        self.lock.check()?;
        self.state.require_absent(super::COMPENSATION_PENDING)?;
        if self.files.len() != 3 {
            return Err(refused());
        }
        validate_manifest(self.state, self.drop_ins, &self.files[2].1)?;
        let update = self.update.as_ref().ok_or_else(refused)?;
        if update.cleanup_record_retired {
            self.state.require_absent(CLEANUP_PENDING)?;
            return self.state.sync_directory();
        }
        if let Some(record) = &update.cleanup_record {
            self.state.verify(record)?;
            return self.state.sync_directory();
        }
        if update.committing || update.backups.len() != 3 || self.files.len() != 3 {
            return Err(refused());
        }
        let backup = |role: usize| -> io::Result<Receipt> {
            let backup = update
                .backups
                .iter()
                .find(|backup| backup.role == Some(role))
                .ok_or_else(refused)?;
            Receipt::record(self.directory(backup.place), &backup.file)
        };
        let snapshot = CleanupRecord {
            version: 1,
            current: FileSet {
                launcher: Receipt::record(self.state, &self.files[0].1)?,
                drop_in: Receipt::record(self.drop_ins, &self.files[1].1)?,
                manifest: Receipt::record(self.state, &self.files[2].1)?,
            },
            backups: FileSet {
                launcher: backup(0)?,
                drop_in: backup(1)?,
                manifest: backup(2)?,
            },
            precommit: Some(Receipt::record(
                self.state,
                update.precommit_record.as_ref().ok_or_else(refused)?,
            )?),
            exchanges: vec![
                Some(Receipt::record(
                    self.state,
                    update.exchange_records[0].as_ref().ok_or_else(refused)?,
                )?),
                Some(Receipt::record(
                    self.state,
                    update.exchange_records[1].as_ref().ok_or_else(refused)?,
                )?),
                Some(Receipt::record(
                    self.state,
                    update.exchange_records[2].as_ref().ok_or_else(refused)?,
                )?),
            ],
        };
        snapshot.validate()?;
        let bytes = serde_json::to_vec(&snapshot).map_err(io::Error::other)?;
        self.clean_candidates()?;
        let (record, result) = match self.state.publish(CLEANUP_PENDING, &bytes, false) {
            Ok(record) => (record, Ok(())),
            Err(PublishFailure::Published { error, owned }) => (owned, Err(error)),
            Err(PublishFailure::Unpublished(error)) => return Err(error),
            Err(PublishFailure::Staged {
                error,
                cleanup_error,
                stage,
            }) => {
                self.stages.push((Place::State, stage));
                return Err(io::Error::other(format!(
                    "{error}; staging cleanup failed: {cleanup_error}"
                )));
            }
        };
        // The decision is physical even if its directory sync failed. It must
        // never permit rollback after a restart may observe this cleanup intent.
        let update = self.update.as_mut().ok_or_else(refused)?;
        update.cleanup_record = Some(record);
        update.committing = true;
        result
    }

    pub(super) fn retire_cleanup_record(&mut self) -> io::Result<()> {
        self.lock.check()?;
        let update = self.update.as_mut().ok_or_else(refused)?;
        if !update.precommit_retired || update.precommit_record.is_some() {
            return Err(refused());
        }
        if update.cleanup_record_retired {
            return Ok(());
        }
        let record = update.cleanup_record.as_ref().ok_or_else(refused)?;
        let result = self.state.remove(record);
        if result.is_ok() || record.file.metadata()?.nlink() == 0 {
            update.cleanup_record = None;
            update.cleanup_record_retired = true;
        }
        result
    }

    #[cfg(test)]
    pub(super) fn cleanup_checkpoint(&self, checkpoint: u8) -> io::Result<()> {
        if let Some((point, abrupt)) = self.stop_after {
            if point == checkpoint {
                if abrupt {
                    std::process::exit(77);
                }
                return Err(io::Error::other("private update cleanup checkpoint"));
            }
        }
        Ok(())
    }
}

pub(in crate::integrations::gnome::transaction) fn recover_cleanup(
    state: &ManagedDirectory,
    drop_ins: &ManagedDirectory,
    lock: &InstallationLock<'_>,
) -> io::Result<()> {
    lock.check()?;
    let journal = match state.read_state(CLEANUP_PENDING) {
        Ok(record) => record,
        // A prior retirement may have unlinked the record before its directory
        // sync failed. No in-memory retired flag survives a fresh resume.
        Err(error) if error.kind() == io::ErrorKind::NotFound => return state.sync_directory(),
        Err(error) => return Err(error),
    };
    let snapshot: CleanupRecord =
        serde_json::from_slice(&journal.content).map_err(|_| refused())?;
    state.require_absent(super::COMPENSATION_PENDING)?;
    snapshot.validate()?;
    // Hold all current inodes and preflight every extant backup before removing
    // any. Missing backups are allowed only under this durable cleanup decision.
    let _launcher = snapshot.current.launcher.reopen(state)?;
    let _drop_in = snapshot.current.drop_in.reopen(drop_ins)?;
    let manifest_file = snapshot.current.manifest.reopen(state)?;
    validate_manifest(state, drop_ins, &manifest_file)?;
    let precommit = if let Some(receipt) = &snapshot.precommit {
        match receipt.reopen(state) {
            Ok(file) => Some(file),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        }
    } else {
        // Old cleanup records carry no authority over a new refusal marker.
        state.require_absent(PRECOMMIT_PENDING)?;
        None
    };
    let mut backups: Vec<(&ManagedDirectory, OwnedFile)> = Vec::new();
    let mut exchange_records = Vec::new();
    for (role, name) in EXCHANGE_NAMES.into_iter().enumerate() {
        let receipt = snapshot.exchanges.get(role).and_then(Option::as_ref);
        if let Some(receipt) = receipt {
            match receipt.reopen(state) {
                Ok(file) => exchange_records.push(file),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        } else {
            state.require_absent(name)?;
        }
    }
    for (directory, receipt) in [
        (state, &snapshot.backups.manifest),
        (drop_ins, &snapshot.backups.drop_in),
        (state, &snapshot.backups.launcher),
    ] {
        match receipt.reopen(directory) {
            Ok(file) => backups.push((directory, file)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    state.verify(&journal)?;
    // Publication may have succeeded before its directory sync failed. Establish
    // the cleanup authority's durability before the first destructive action.
    state.sync_directory()?;
    for (directory, file) in backups {
        lock.check()?;
        directory.remove(&file)?;
    }
    state.sync_directory()?;
    drop_ins.sync_directory()?;
    for record in exchange_records.into_iter().rev() {
        lock.check()?;
        state.remove(&record)?;
    }
    if let Some(precommit) = precommit {
        lock.check()?;
        state.remove(&precommit)?;
    }
    lock.check()?;
    state.remove(&journal)
}

#[cfg(test)]
mod tests {
    use super::super::super::{ActivationTransaction, DROP_IN, LAUNCHER, tests::fixture};
    use super::*;
    use crate::integrations::gnome::{directories, render_activation};
    use std::{
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt},
        path::Path,
    };

    #[test]
    fn abrupt_cleanup_child() {
        let Some(root) = std::env::var_os("LYN_PRIVATE_INSTALL_RECOVERY_ROOT") else {
            return;
        };
        let root = Path::new(&root);
        let metadata = fs::symlink_metadata(root).unwrap();
        assert!(
            metadata.is_dir()
                && metadata.uid() == unsafe { libc::geteuid() }
                && metadata.mode() & 0o7777 == 0o700
        );
        assert_eq!(root.parent().unwrap(), std::env::temp_dir());
        assert!(
            root.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("lyn-install-recovery-")
        );
        let checkpoint: u8 = std::env::var("LYN_PRIVATE_INSTALL_RECOVERY_POINT")
            .unwrap()
            .parse()
            .unwrap();
        assert!((1..=4).contains(&checkpoint));
        let state_path = root.join("state");
        let state = directories::open_private(&state_path, false).unwrap();
        let target = directories::open_drop_ins(&root.join("drop-ins"), false).unwrap();
        let initial =
            render_activation(&root.join("native.so"), &state_path.join(LAUNCHER)).unwrap();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated =
            render_activation(&root.join("native-v2.so"), &state_path.join(LAUNCHER)).unwrap();
        transaction.update(updated).unwrap();
        transaction.stop_after = Some((checkpoint, true));
        transaction.finish_update().unwrap();
        panic!("cleanup checkpoint did not exit");
    }

    #[test]
    fn four_cleanup_process_exits_preserve_new_version_and_remove_only_old_backups() {
        for checkpoint in 1..=4 {
            let root = tempfile::Builder::new()
                .prefix("lyn-install-recovery-")
                .tempdir()
                .unwrap();
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
            let state_path = root.path().join("state");
            let target_path = root.path().join("drop-ins");
            let state = directories::open_private(&state_path, true).unwrap();
            let target = directories::open_drop_ins(&target_path, true).unwrap();
            fs::write(target_path.join("user.conf"), b"user settings").unwrap();
            super::super::super::recovery::tests::abrupt_exit(
                root.path(),
                checkpoint,
                "integrations::gnome::transaction::update::cleanup::tests::abrupt_cleanup_child",
            );
            let mut resumed = ActivationTransaction::resume(&state, &target).unwrap();
            let expected = render_activation(
                &root.path().join("native-v2.so"),
                &state_path.join(LAUNCHER),
            )
            .unwrap();
            assert_eq!(
                fs::read_to_string(state_path.join(LAUNCHER)).unwrap(),
                expected.launcher
            );
            assert_eq!(
                fs::read_to_string(target_path.join(DROP_IN)).unwrap(),
                expected.drop_in
            );
            assert!(!state_path.join(CLEANUP_PENDING).exists());
            assert_eq!(fs::read_dir(&state_path).unwrap().count(), 3);
            assert_eq!(fs::read_dir(&target_path).unwrap().count(), 2);
            assert_eq!(
                fs::read(target_path.join("user.conf")).unwrap(),
                b"user settings"
            );
            resumed.rollback().unwrap();
        }
    }

    #[test]
    fn edited_new_file_refuses_recovery_without_deleting_remaining_backups() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        transaction.update(updated).unwrap();
        transaction.stop_after = Some((1, false));
        assert!(transaction.finish_update().is_err());
        drop(transaction);
        let before_state = fs::read_dir(root.path().join("state")).unwrap().count();
        let before_target = fs::read_dir(root.path().join("drop-ins")).unwrap().count();
        fs::write(root.path().join("drop-ins").join(DROP_IN), b"user edit").unwrap();
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        assert_eq!(
            fs::read_dir(root.path().join("state")).unwrap().count(),
            before_state
        );
        assert_eq!(
            fs::read_dir(root.path().join("drop-ins")).unwrap().count(),
            before_target
        );
        assert_eq!(
            fs::read(root.path().join("drop-ins").join(DROP_IN)).unwrap(),
            b"user edit"
        );
    }

    #[test]
    fn replaced_backup_refuses_recovery_before_any_other_backup_is_removed() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        transaction.update(updated).unwrap();
        transaction.stop_after = Some((1, false));
        assert!(transaction.finish_update().is_err());
        drop(transaction);
        let journal = state.read_state(CLEANUP_PENDING).unwrap();
        let snapshot: CleanupRecord = serde_json::from_slice(&journal.content).unwrap();
        let path = root
            .path()
            .join("drop-ins")
            .join(snapshot.backups.drop_in.filename());
        fs::rename(&path, root.path().join("drop-ins/user-backup")).unwrap();
        fs::write(&path, b"foreign file").unwrap();
        let before_state = fs::read_dir(root.path().join("state")).unwrap().count();
        let before_target = fs::read_dir(root.path().join("drop-ins")).unwrap().count();
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        assert_eq!(
            fs::read_dir(root.path().join("state")).unwrap().count(),
            before_state
        );
        assert_eq!(
            fs::read_dir(root.path().join("drop-ins")).unwrap().count(),
            before_target
        );
        assert_eq!(fs::read(path).unwrap(), b"foreign file");
    }

    #[test]
    fn published_intent_sync_error_keeps_the_commit_decision_and_recovers_on_reopen() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        transaction.update(updated).unwrap();
        state.fail_directory_sync(true);
        assert!(transaction.finish_update().is_err());
        assert!(transaction.rollback_update().is_err());
        assert!(root.path().join("state").join(CLEANUP_PENDING).is_file());
        state.fail_directory_sync(false);
        drop(transaction);
        ActivationTransaction::resume(&state, &target).unwrap();
        assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 3);
        assert!(!root.path().join("state").join(CLEANUP_PENDING).exists());
    }

    #[test]
    fn retired_intent_sync_error_can_retry_without_recreating_a_journal() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        transaction.update(updated).unwrap();
        transaction.prepare_cleanup_record().unwrap();
        transaction.clean_backups().unwrap();
        transaction.retire_precommit().unwrap();
        state.fail_directory_sync(true);
        assert!(transaction.retire_cleanup_record().is_err());
        assert!(!root.path().join("state").join(CLEANUP_PENDING).exists());
        assert!(transaction.finish_update().is_err());
        state.fail_directory_sync(false);
        transaction.finish_update().unwrap();
        assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 3);
    }

    #[test]
    fn recovery_retries_failed_retirement_sync_even_after_the_name_is_gone() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        transaction.update(updated).unwrap();
        transaction.prepare_cleanup_record().unwrap();
        transaction.clean_backups().unwrap();
        drop(transaction);
        // First flush makes the intent durable; the second precedes retirement.
        state.fail_directory_sync_after(6);
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        assert!(!root.path().join("state").join(CLEANUP_PENDING).exists());
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        state.fail_directory_sync(false);
        ActivationTransaction::resume(&state, &target).unwrap();
    }

    #[test]
    fn recovery_must_sync_the_intent_before_unlinking_any_backup() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        transaction.update(updated).unwrap();
        state.fail_directory_sync(true);
        assert!(transaction.finish_update().is_err());
        drop(transaction);
        let before_state = fs::read_dir(root.path().join("state")).unwrap().count();
        let before_target = fs::read_dir(root.path().join("drop-ins")).unwrap().count();
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        assert_eq!(
            fs::read_dir(root.path().join("state")).unwrap().count(),
            before_state
        );
        assert_eq!(
            fs::read_dir(root.path().join("drop-ins")).unwrap().count(),
            before_target
        );
        state.fail_directory_sync(false);
        ActivationTransaction::resume(&state, &target).unwrap();
    }

    #[test]
    fn live_finish_refuses_stale_embedded_receipts_before_backup_deletion() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        let launcher = updated.launcher.clone();
        transaction.update(updated).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(root.path().join("state").join(LAUNCHER), launcher).unwrap();
        let before_state = fs::read_dir(root.path().join("state")).unwrap().count();
        let before_target = fs::read_dir(root.path().join("drop-ins")).unwrap().count();
        assert!(transaction.finish_update().is_err());
        assert_eq!(
            fs::read_dir(root.path().join("state")).unwrap().count(),
            before_state
        );
        assert_eq!(
            fs::read_dir(root.path().join("drop-ins")).unwrap().count(),
            before_target
        );
    }

    #[test]
    fn cleanup_preparation_without_installed_roles_refuses_without_panicking() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        assert!(transaction.prepare_cleanup_record().is_err());
        assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 1);
        assert_eq!(
            fs::read_dir(root.path().join("drop-ins")).unwrap().count(),
            0
        );
    }
}
