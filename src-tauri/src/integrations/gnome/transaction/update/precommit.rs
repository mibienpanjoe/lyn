//! Persistent refusal marker: prepared does not mean committed after restart.

mod early;
pub(in crate::integrations::gnome::transaction) use early::recover_early;

pub(in crate::integrations::gnome::transaction) const EXCHANGE_NAMES: [&str; 2] = [
    "update-launcher-exchange-v1.json",
    "update-drop-in-exchange-v1.json",
];

pub(in crate::integrations::gnome::transaction) const PRECOMMIT_PENDING: &str =
    "update-precommit-pending-v1.json";

use super::super::{ActivationTransaction, Place, refused};
use crate::integrations::gnome::{managed::PublishFailure, ownership::Receipt};
use serde::{Deserialize, Serialize};
use std::{io, os::unix::fs::MetadataExt};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedUpdate {
    version: u8,
    nonce: uuid::Uuid,
    previous_launcher: Receipt,
    previous_drop_in: Receipt,
    previous_manifest: Receipt,
}

impl PreparedUpdate {
    fn validate(&self) -> io::Result<()> {
        self.previous_launcher.validate()?;
        self.previous_drop_in.validate()?;
        self.previous_manifest.validate()?;
        if self.version != 1
            || !self.previous_launcher.is_for(super::super::LAUNCHER, 0o700)
            || !self.previous_drop_in.is_for(super::super::DROP_IN, 0o600)
            || !self.previous_manifest.is_for(super::super::MANIFEST, 0o600)
        {
            return Err(refused());
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExchangeRecord {
    version: u8,
    role: u8,
    precommit: Receipt,
    previous: Receipt,
    candidate: Receipt,
}

impl ActivationTransaction<'_> {
    pub(super) fn record_exchange(&mut self, role: usize, place: Place) -> io::Result<()> {
        self.lock.check()?;
        self.state.require_absent(EXCHANGE_NAMES[role])?;
        let update = self.update.as_ref().ok_or_else(refused)?;
        let header = update.precommit_record.as_ref().ok_or_else(refused)?;
        let candidate = &update.candidates.last().ok_or_else(refused)?.1;
        let body = ExchangeRecord {
            version: 1,
            role: role as u8,
            precommit: Receipt::record(self.state, header)?,
            previous: Receipt::record(self.directory(place), &self.files[role].1)?,
            candidate: Receipt::record(self.directory(place), candidate)?,
        };
        let prepared: PreparedUpdate =
            serde_json::from_slice(&header.content).map_err(|_| refused())?;
        prepared.validate()?;
        let expected = if role == 0 {
            &prepared.previous_launcher
        } else {
            &prepared.previous_drop_in
        };
        if &body.previous != expected {
            return Err(refused());
        }
        let bytes = serde_json::to_vec(&body).map_err(io::Error::other)?;
        let (record, result) = match self.state.publish(EXCHANGE_NAMES[role], &bytes, false) {
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
        self.update.as_mut().ok_or_else(refused)?.exchange_records[role] = Some(record);
        result
    }

    fn retire_exchanges(&mut self) -> io::Result<()> {
        for role in (0..2).rev() {
            self.lock.check()?;
            let update = self.update.as_mut().ok_or_else(refused)?;
            if let Some(record) = &update.exchange_records[role] {
                let result = self.state.remove(record);
                if result.is_ok() || record.file.metadata()?.nlink() == 0 {
                    update.exchange_records[role] = None;
                }
                result?;
            } else {
                self.state.require_absent(EXCHANGE_NAMES[role])?;
                self.state.sync_directory()?;
            }
        }
        Ok(())
    }
    pub(super) fn prepare_precommit(&mut self) -> io::Result<()> {
        self.lock.check()?;
        self.state.require_absent(PRECOMMIT_PENDING)?;
        let marker = PreparedUpdate {
            version: 1,
            nonce: uuid::Uuid::new_v4(),
            previous_launcher: Receipt::record(self.state, &self.files[0].1)?,
            previous_drop_in: Receipt::record(self.drop_ins, &self.files[1].1)?,
            previous_manifest: Receipt::record(self.state, &self.files[2].1)?,
        };
        let bytes = serde_json::to_vec(&marker).map_err(io::Error::other)?;
        let (record, result) = match self.state.publish(PRECOMMIT_PENDING, &bytes, false) {
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
        self.update.as_mut().ok_or_else(refused)?.precommit_record = Some(record);
        result
    }

    pub(super) fn retire_precommit(&mut self) -> io::Result<()> {
        self.retire_exchanges()?;
        self.lock.check()?;
        let update = self.update.as_mut().ok_or_else(refused)?;
        if update.precommit_retired || update.precommit_record.is_none() {
            self.state.require_absent(PRECOMMIT_PENDING)?;
            self.state.sync_directory()?;
            update.precommit_retired = true;
            return Ok(());
        }
        let record = update.precommit_record.as_ref().ok_or_else(refused)?;
        let result = self.state.remove(record);
        if result.is_ok() || record.file.metadata()?.nlink() == 0 {
            update.precommit_record = None;
            update.precommit_retired = true;
        }
        result
    }

    #[cfg(test)]
    pub(super) fn precommit_checkpoint(&self, checkpoint: u8) -> io::Result<()> {
        if let Some((point, abrupt)) = self.stop_after {
            if point == checkpoint {
                if abrupt {
                    std::process::exit(77);
                }
                return Err(io::Error::other("private early update checkpoint"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::{ActivationTransaction, DROP_IN, LAUNCHER, tests::fixture};
    use super::*;
    use std::fs;

    #[test]
    fn closing_a_prepared_update_must_not_implicitly_commit_it_on_resume() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = crate::integrations::gnome::render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        transaction.update(updated).unwrap();
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
    }

    #[test]
    fn marker_publication_sync_error_precedes_active_changes_and_can_be_cancelled() {
        let (root, state, target, initial) = fixture();
        let old_launcher = initial.launcher.clone();
        let old_drop_in = initial.drop_in.clone();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = crate::integrations::gnome::render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        state.fail_directory_sync(true);
        assert!(transaction.update(updated).is_err());
        assert_eq!(
            fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
            old_launcher
        );
        assert_eq!(
            fs::read_to_string(root.path().join("drop-ins").join(DROP_IN)).unwrap(),
            old_drop_in
        );
        state.fail_directory_sync(false);
        transaction.rollback_update().unwrap();
        assert!(!root.path().join("state").join(PRECOMMIT_PENDING).exists());
        drop(transaction);
        ActivationTransaction::resume(&state, &target).unwrap();
    }

    #[test]
    fn foreign_marker_refuses_update_without_creating_candidates() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        fs::write(
            root.path().join("state").join(PRECOMMIT_PENDING),
            b"foreign marker",
        )
        .unwrap();
        let before_state = fs::read_dir(root.path().join("state")).unwrap().count();
        let before_target = fs::read_dir(root.path().join("drop-ins")).unwrap().count();
        let updated = crate::integrations::gnome::render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        assert!(transaction.update(updated).is_err());
        assert_eq!(
            fs::read_dir(root.path().join("state")).unwrap().count(),
            before_state
        );
        assert_eq!(
            fs::read_dir(root.path().join("drop-ins")).unwrap().count(),
            before_target
        );
        assert_eq!(
            fs::read(root.path().join("state").join(PRECOMMIT_PENDING)).unwrap(),
            b"foreign marker"
        );
    }

    #[test]
    fn edited_marker_refuses_committed_recovery_before_backup_deletion() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = crate::integrations::gnome::render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        transaction.update(updated).unwrap();
        transaction.stop_after = Some((1, false));
        assert!(transaction.finish_update().is_err());
        drop(transaction);
        fs::write(
            root.path().join("state").join(PRECOMMIT_PENDING),
            b"user edit",
        )
        .unwrap();
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
        assert_eq!(
            fs::read(root.path().join("state").join(PRECOMMIT_PENDING)).unwrap(),
            b"user edit"
        );
    }

    #[test]
    fn legacy_cleanup_record_cannot_retire_a_new_precommit_marker() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = crate::integrations::gnome::render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        transaction.update(updated).unwrap();
        transaction.stop_after = Some((1, false));
        assert!(transaction.finish_update().is_err());
        drop(transaction);
        let path = root
            .path()
            .join("state")
            .join(super::super::CLEANUP_PENDING);
        let mut body: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        body.as_object_mut().unwrap().remove("precommit");
        fs::write(path, serde_json::to_vec(&body).unwrap()).unwrap();
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
    }
}
