//! Process-local reversible update. Persistent interruption recovery is separate.

mod cleanup;
mod precommit;
pub(super) use cleanup::{CLEANUP_PENDING, recover_cleanup};
pub(super) use precommit::{
    COMPENSATION_PENDING, EXCHANGE_NAMES, PRECOMMIT_PENDING, recover_early,
};

use super::super::{
    RenderedActivation,
    managed::{OwnedFile, PublishFailure},
    ownership::Receipt,
};
use super::{ActivationTransaction, Manifest, Phase, Place, refused};
use std::{io, os::unix::fs::MetadataExt};

struct Backup {
    role: Option<usize>,
    place: Place,
    file: OwnedFile,
}

#[derive(Default)]
pub(super) struct PendingUpdate {
    backups: Vec<Backup>,
    candidates: Vec<(Place, OwnedFile)>,
    ready: bool,
    committing: bool,
    manifest_refreshed: bool,
    cleanup_record: Option<OwnedFile>,
    cleanup_record_retired: bool,
    precommit_record: Option<OwnedFile>,
    precommit_retired: bool,
    exchange_records: [Option<OwnedFile>; 3],
    compensation_record: Option<OwnedFile>,
    rolling_back: bool,
}

impl ActivationTransaction<'_> {
    fn preflight_update(&self) -> io::Result<()> {
        self.lock.check()?;
        self.state.require_absent(super::removal::PENDING_REMOVAL)?;
        for (place, file) in &self.files {
            self.directory(*place).verify(file)?;
        }
        if let Some(update) = &self.update {
            if let Some(record) = &update.compensation_record {
                self.state.verify(record)?;
            } else {
                self.state.require_absent(COMPENSATION_PENDING)?;
            }
            if let Some(record) = &update.precommit_record {
                self.state.verify(record)?;
            }
            for (role, record) in update.exchange_records.iter().enumerate() {
                if let Some(record) = record {
                    self.state.verify(record)?;
                } else {
                    self.state.require_absent(EXCHANGE_NAMES[role])?;
                }
            }
            for backup in &update.backups {
                self.directory(backup.place).verify(&backup.file)?;
            }
            for (place, file) in &update.candidates {
                self.directory(*place).verify(file)?;
            }
        }
        Ok(())
    }

    fn candidate(&mut self, place: Place, bytes: &[u8], executable: bool) -> io::Result<()> {
        self.lock.check()?;
        let name = format!(".lyn-update-{}", uuid::Uuid::new_v4());
        match self.directory(place).publish(&name, bytes, executable) {
            Ok(file) => {
                self.update
                    .as_mut()
                    .ok_or_else(refused)?
                    .candidates
                    .push((place, file));
                Ok(())
            }
            Err(PublishFailure::Unpublished(error)) => Err(error),
            Err(PublishFailure::Published { error, owned }) => {
                self.update
                    .as_mut()
                    .ok_or_else(refused)?
                    .candidates
                    .push((place, owned));
                Err(error)
            }
            Err(PublishFailure::Staged {
                error,
                cleanup_error,
                stage,
            }) => {
                self.stages.push((place, stage));
                Err(io::Error::other(format!(
                    "{error}; staging cleanup failed: {cleanup_error}"
                )))
            }
        }
    }

    fn replace_role(
        &mut self,
        role: usize,
        place: Place,
        bytes: &[u8],
        executable: bool,
    ) -> io::Result<()> {
        let compensating = self.update.as_ref().ok_or_else(refused)?.rolling_back;
        if compensating && role != 2 {
            return Err(refused());
        }
        if compensating
            && self
                .update
                .as_ref()
                .ok_or_else(refused)?
                .compensation_record
                .is_some()
        {
            self.check_compensation_candidate(bytes)?;
            self.state.sync_directory()?;
        } else {
            self.candidate(place, bytes, executable)?;
            if compensating {
                self.record_compensation()?;
            }
        }
        if !compensating {
            self.record_exchange(role, place)?;
            #[cfg(test)]
            if role == 2 {
                self.precommit_checkpoint(11)?;
            }
        }
        #[cfg(test)]
        if compensating {
            self.precommit_checkpoint(9)?;
        }
        let directory = match place {
            Place::State => self.state,
            Place::DropIns => self.drop_ins,
        };
        let update = self.update.as_mut().ok_or_else(refused)?;
        let candidate = &mut update.candidates.last_mut().ok_or_else(refused)?.1;
        let exchanged = directory.exchange(&mut self.files[role].1, candidate)?;
        let (_, candidate) = update.candidates.pop().ok_or_else(refused)?;
        let previous = std::mem::replace(&mut self.files[role].1, candidate);
        update.backups.push(Backup {
            role: Some(role),
            place,
            file: previous,
        });
        #[cfg(test)]
        if compensating {
            self.precommit_checkpoint(10)?;
        }
        // Record the physical effect before reporting a durability failure.
        exchanged.durability
    }

    fn manifest_bytes(&self) -> io::Result<Vec<u8>> {
        let manifest = Manifest {
            version: 1,
            launcher: Receipt::record(self.state, &self.files[0].1)?,
            drop_in: Receipt::record(self.drop_ins, &self.files[1].1)?,
        };
        serde_json::to_vec(&manifest).map_err(io::Error::other)
    }

    pub(in crate::integrations::gnome) fn update(
        &mut self,
        rendered: RenderedActivation,
    ) -> io::Result<()> {
        if self.phase != Phase::Complete
            || self.update.is_some()
            || self.files.len() != 3
            || !self.stages.is_empty()
        {
            return Err(refused());
        }
        self.preflight_update()?;
        self.state.require_absent(CLEANUP_PENDING)?;
        self.state.require_absent(PRECOMMIT_PENDING)?;
        self.state.require_absent(COMPENSATION_PENDING)?;
        for name in EXCHANGE_NAMES {
            self.state.require_absent(name)?;
        }
        self.update = Some(PendingUpdate::default());
        self.phase = Phase::Updating;
        self.prepare_precommit()?;
        #[cfg(test)]
        self.precommit_checkpoint(5)?;
        self.replace_role(0, Place::State, rendered.launcher.as_bytes(), true)?;
        #[cfg(test)]
        self.precommit_checkpoint(6)?;
        self.replace_role(1, Place::DropIns, rendered.drop_in.as_bytes(), false)?;
        #[cfg(test)]
        self.precommit_checkpoint(7)?;
        let bytes = self.manifest_bytes()?;
        self.replace_role(2, Place::State, &bytes, false)?;
        #[cfg(test)]
        self.precommit_checkpoint(8)?;
        self.update.as_mut().ok_or_else(refused)?.ready = true;
        Ok(())
    }

    fn clean_candidates(&mut self) -> io::Result<()> {
        while let Some((place, stage)) = self.stages.last() {
            self.lock.check()?;
            self.directory(*place).discard_stage(stage)?;
            self.stages.pop();
        }
        while let Some((place, file)) = self.update.as_ref().ok_or_else(refused)?.candidates.last()
        {
            self.lock.check()?;
            let result = self.directory(*place).remove(file);
            let removed = result.is_ok() || file.file.metadata()?.nlink() == 0;
            if removed {
                self.update.as_mut().ok_or_else(refused)?.candidates.pop();
            }
            result?;
        }
        Ok(())
    }

    fn clean_backups(&mut self) -> io::Result<()> {
        while let Some(backup) = self.update.as_ref().ok_or_else(refused)?.backups.last() {
            self.lock.check()?;
            let result = self.directory(backup.place).remove(&backup.file);
            let removed = result.is_ok() || backup.file.file.metadata()?.nlink() == 0;
            if removed {
                self.update.as_mut().ok_or_else(refused)?.backups.pop();
            }
            result?;
            #[cfg(test)]
            if self.update.as_ref().ok_or_else(refused)?.committing {
                let remaining = self.update.as_ref().ok_or_else(refused)?.backups.len();
                self.cleanup_checkpoint((4 - remaining) as u8)?;
            }
        }
        Ok(())
    }

    pub(in crate::integrations::gnome) fn finish_update(&mut self) -> io::Result<()> {
        if self.phase != Phase::Updating || !self.update.as_ref().is_some_and(|update| update.ready)
        {
            return Err(refused());
        }
        self.preflight_update()?;
        // Persist the explicit irreversible decision before deleting backups.
        self.prepare_cleanup_record()?;
        #[cfg(test)]
        self.cleanup_checkpoint(1)?;
        self.clean_candidates()?;
        self.clean_backups()?;
        self.sync_update_directories()?;
        self.retire_precommit()?;
        self.retire_cleanup_record()?;
        self.update = None;
        self.phase = Phase::Complete;
        Ok(())
    }

    pub(in crate::integrations::gnome) fn rollback_update(&mut self) -> io::Result<()> {
        if self.phase != Phase::Updating
            || !self
                .update
                .as_ref()
                .is_some_and(|update| !update.committing)
        {
            return Err(refused());
        }
        self.preflight_update()?;
        if self
            .update
            .as_ref()
            .is_some_and(|update| update.precommit_record.is_none())
        {
            self.state.require_absent(PRECOMMIT_PENDING)?;
            // Failed preparation cannot have changed any active role.
            self.clean_candidates()?;
            self.sync_update_directories()?;
            self.update = None;
            self.phase = Phase::Complete;
            return Ok(());
        }
        self.update.as_mut().ok_or_else(refused)?.ready = false;
        self.update.as_mut().ok_or_else(refused)?.rolling_back = true;
        // Never restore an old manifest with receipts invalidated by rename.
        for backup in &mut self.update.as_mut().ok_or_else(refused)?.backups {
            if backup.role == Some(2) {
                backup.role = None;
            }
        }
        for role in [1, 0] {
            let update = self.update.as_mut().ok_or_else(refused)?;
            if let Some(position) = update
                .backups
                .iter()
                .rposition(|backup| backup.role == Some(role))
            {
                let backup = &mut update.backups[position];
                let directory = match backup.place {
                    Place::State => self.state,
                    Place::DropIns => self.drop_ins,
                };
                let exchanged = directory.exchange(&mut self.files[role].1, &mut backup.file)?;
                std::mem::swap(&mut self.files[role].1, &mut backup.file);
                backup.role = None;
                exchanged.durability?;
            }
        }
        if self
            .update
            .as_ref()
            .ok_or_else(refused)?
            .compensation_record
            .is_none()
        {
            self.clean_candidates()?;
        }
        if !self.update.as_ref().ok_or_else(refused)?.manifest_refreshed {
            let bytes = self.manifest_bytes()?;
            let result = self.replace_role(2, Place::State, &bytes, false);
            let update = self.update.as_mut().ok_or_else(refused)?;
            if let Some(backup) = update
                .backups
                .iter_mut()
                .find(|backup| backup.role == Some(2))
            {
                backup.role = None;
                update.manifest_refreshed = true;
            }
            result?;
        }
        self.sync_update_directories()?;
        self.clean_backups()?;
        self.sync_update_directories()?;
        self.retire_precommit()?;
        self.update = None;
        self.phase = Phase::Complete;
        Ok(())
    }

    fn sync_update_directories(&self) -> io::Result<()> {
        // Also covers a prior unlink which succeeded before fsync failed and
        // whose lease was retired. No completion may forget that directory.
        self.state.sync_directory()?;
        self.drop_ins.sync_directory()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ActivationTransaction, DROP_IN, LAUNCHER, MANIFEST, tests::fixture};
    use std::fs;

    fn version_two(root: &std::path::Path) -> crate::integrations::gnome::RenderedActivation {
        let mut rendered = crate::integrations::gnome::render_activation(
            &root.join("native-v2.so"),
            &root.join("state").join(LAUNCHER),
        )
        .unwrap();
        rendered.drop_in.push_str("# fixture version two\n");
        rendered
    }

    #[test]
    fn updated_files_can_be_rolled_back_and_reopened_with_fresh_receipts() {
        let (root, state, target, rendered) = fixture();
        let old_launcher = rendered.launcher.clone();
        let old_drop_in = rendered.drop_in.clone();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        transaction.update(version_two(root.path())).unwrap();
        assert!(
            fs::read_to_string(root.path().join("state").join(LAUNCHER))
                .unwrap()
                .contains("native-v2.so")
        );
        assert!(transaction.rollback().is_err());
        transaction.rollback_update().unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
            old_launcher
        );
        assert_eq!(
            fs::read_to_string(root.path().join("drop-ins").join(DROP_IN)).unwrap(),
            old_drop_in
        );
        drop(transaction);
        let mut resumed = ActivationTransaction::resume(&state, &target).unwrap();
        resumed.rollback().unwrap();
    }

    #[test]
    fn finished_update_reopens_and_uninstalls_without_old_candidates() {
        let (root, state, target, rendered) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        transaction.update(version_two(root.path())).unwrap();
        transaction.finish_update().unwrap();
        assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 3);
        assert_eq!(
            fs::read_dir(root.path().join("drop-ins")).unwrap().count(),
            1
        );
        drop(transaction);
        let mut resumed = ActivationTransaction::resume(&state, &target).unwrap();
        resumed.rollback().unwrap();
        assert!(!root.path().join("state").join(MANIFEST).exists());
    }

    #[test]
    fn interrupted_update_keeps_enough_ownership_to_restore_the_old_installation() {
        let (root, state, target, rendered) = fixture();
        let old_launcher = rendered.launcher.clone();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        target.fail_directory_sync(true);
        assert!(transaction.update(version_two(root.path())).is_err());
        assert!(transaction.finish_update().is_err());
        target.fail_directory_sync(false);
        transaction.rollback_update().unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
            old_launcher
        );
        assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 3);
        assert_eq!(
            fs::read_dir(root.path().join("drop-ins")).unwrap().count(),
            1
        );
        drop(transaction);
        ActivationTransaction::resume(&state, &target).unwrap();
    }

    #[test]
    fn failed_commit_cleanup_can_only_resume_commit_not_restore_a_partial_old_version() {
        let (root, state, target, rendered) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        transaction.update(version_two(root.path())).unwrap();
        target.fail_directory_sync(true);
        assert!(transaction.finish_update().is_err());
        target.fail_directory_sync(false);
        assert!(transaction.rollback_update().is_err());
        assert!(
            fs::read_to_string(root.path().join("state").join(LAUNCHER))
                .unwrap()
                .contains("native-v2.so")
        );
        assert!(
            fs::read_to_string(root.path().join("drop-ins").join(DROP_IN))
                .unwrap()
                .contains("fixture version two")
        );
        transaction.finish_update().unwrap();
        drop(transaction);
        ActivationTransaction::resume(&state, &target).unwrap();
    }

    #[test]
    fn commit_retry_must_flush_the_directory_of_a_backup_already_unlinked() {
        let (root, state, target, rendered) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        transaction.update(version_two(root.path())).unwrap();
        target.fail_directory_sync(true);
        assert!(transaction.finish_update().is_err());
        // Its drop-in backup is now unlinked. A retry must still flush that
        // directory, even if all remaining backup files are under state.
        assert!(transaction.finish_update().is_err());
        target.fail_directory_sync(false);
        transaction.finish_update().unwrap();
        drop(transaction);
        ActivationTransaction::resume(&state, &target).unwrap();
    }
}
