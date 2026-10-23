//! Ownership of the manifest displaced by a live rollback compensation.

use super::{ExchangeRecord, PRECOMMIT_PENDING, PreparedUpdate};
use crate::integrations::gnome::{
    managed::{ManagedDirectory, PublishFailure},
    ownership::Receipt,
    transaction::{ActivationTransaction, MANIFEST, Place, refused},
};
use std::{io, os::unix::fs::MetadataExt};

pub(in crate::integrations::gnome::transaction) const COMPENSATION_PENDING: &str =
    "update-restore-manifest-v1.json";

pub(super) fn validate(
    state: &ManagedDirectory,
    body: &ExchangeRecord,
    prepared: &PreparedUpdate,
    forward: Option<&ExchangeRecord>,
) -> io::Result<()> {
    body.previous.validate()?;
    body.candidate.validate()?;
    let previous_known = body.previous == prepared.previous_manifest
        || forward.is_some_and(|forward| body.previous.same_renamed_payload(&forward.candidate));
    if body.version != 1
        || body.role != 3
        || !body.precommit.is_for(PRECOMMIT_PENDING, 0o600)
        || !body.previous.is_for(MANIFEST, 0o600)
        || !super::super::cleanup::candidate(&body.candidate, 0o600)
        || !previous_known
    {
        return Err(refused());
    }
    body.precommit.reopen(state)?;
    Ok(())
}

impl ActivationTransaction<'_> {
    pub(in crate::integrations::gnome::transaction::update) fn record_compensation(
        &mut self,
    ) -> io::Result<()> {
        self.lock.check()?;
        self.state.require_absent(COMPENSATION_PENDING)?;
        let update = self.update.as_ref().ok_or_else(refused)?;
        let header = update.precommit_record.as_ref().ok_or_else(refused)?;
        let prepared: PreparedUpdate =
            serde_json::from_slice(&header.content).map_err(|_| refused())?;
        prepared.validate()?;
        let forward: Option<ExchangeRecord> = update.exchange_records[2]
            .as_ref()
            .map(|file| serde_json::from_slice(&file.content).map_err(|_| refused()))
            .transpose()?;
        let body = ExchangeRecord {
            version: 1,
            role: 3,
            precommit: Receipt::record(self.state, header)?,
            previous: Receipt::record(self.state, &self.files[2].1)?,
            candidate: Receipt::record(
                self.state,
                &update.candidates.last().ok_or_else(refused)?.1,
            )?,
        };
        validate(self.state, &body, &prepared, forward.as_ref())?;
        let bytes = serde_json::to_vec(&body).map_err(io::Error::other)?;
        let (record, result) = match self.state.publish(COMPENSATION_PENDING, &bytes, false) {
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
        self.update
            .as_mut()
            .ok_or_else(refused)?
            .compensation_record = Some(record);
        result
    }

    pub(in crate::integrations::gnome::transaction::update) fn check_compensation_candidate(
        &self,
        bytes: &[u8],
    ) -> io::Result<()> {
        let update = self.update.as_ref().ok_or_else(refused)?;
        let record = update.compensation_record.as_ref().ok_or_else(refused)?;
        self.state.verify(record)?;
        let body: ExchangeRecord =
            serde_json::from_slice(&record.content).map_err(|_| refused())?;
        let candidate = &update.candidates.last().ok_or_else(refused)?.1;
        if candidate.content != bytes
            || Receipt::record(self.state, candidate)? != body.candidate
            || Receipt::record(self.state, &self.files[2].1)? != body.previous
        {
            return Err(refused());
        }
        Ok(())
    }

    pub(super) fn retire_compensation(&mut self) -> io::Result<()> {
        self.lock.check()?;
        let update = self.update.as_mut().ok_or_else(refused)?;
        if let Some(record) = &update.compensation_record {
            let result = self.state.remove(record);
            if result.is_ok() || record.file.metadata()?.nlink() == 0 {
                update.compensation_record = None;
            }
            result
        } else {
            // No compensation was part of an ordinary committed update.
            self.state.require_absent(COMPENSATION_PENDING)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::COMPENSATION_PENDING;
    use crate::integrations::gnome::{
        render_activation,
        transaction::{ActivationTransaction, DROP_IN, LAUNCHER, tests::fixture},
    };
    use std::fs;

    #[test]
    fn compensation_authority_sync_failure_can_retry_without_another_candidate() {
        let (root, state, target, initial) = fixture();
        let old = initial.launcher.clone();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        transaction.update(updated).unwrap();
        state.fail_directory_sync_after(2);
        assert!(transaction.rollback_update().is_err());
        assert!(
            transaction
                .update
                .as_ref()
                .unwrap()
                .compensation_record
                .is_some()
        );
        assert!(!transaction.update.as_ref().unwrap().manifest_refreshed);
        state.fail_directory_sync(false);
        transaction.rollback_update().unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
            old
        );
        assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 3);
        drop(transaction);
        ActivationTransaction::resume(&state, &target)
            .unwrap()
            .rollback()
            .unwrap();
    }

    #[test]
    fn edited_compensation_authority_refuses_before_cleanup() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        transaction.update(updated).unwrap();
        transaction.stop_after = Some((10, false));
        assert!(transaction.rollback_update().is_err());
        drop(transaction);
        fs::write(
            root.path().join("state").join(COMPENSATION_PENDING),
            b"foreign",
        )
        .unwrap();
        let before = fs::read_dir(root.path().join("state")).unwrap().count();
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        assert_eq!(
            fs::read_dir(root.path().join("state")).unwrap().count(),
            before
        );
        assert_eq!(
            fs::read(root.path().join("state").join(COMPENSATION_PENDING)).unwrap(),
            b"foreign"
        );
    }

    #[test]
    fn recorded_but_unexchanged_forward_manifest_can_be_compensated() {
        let (root, state, target, initial) = fixture();
        let old = initial.launcher.clone();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        transaction.stop_after = Some((11, false));
        let updated = render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        assert!(transaction.update(updated).is_err());
        transaction.stop_after = Some((10, false));
        assert!(transaction.rollback_update().is_err());
        drop(transaction);
        let mut restored = ActivationTransaction::resume(&state, &target).unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
            old
        );
        assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 3);
        restored.rollback().unwrap();
    }

    #[test]
    fn compensation_sync_boundaries_can_retry_without_forgetting_authority() {
        for successes in 0..=14 {
            let (root, state, target, initial) = fixture();
            let old = initial.launcher.clone();
            let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
            transaction.apply().unwrap();
            let updated = render_activation(
                &root.path().join("native-v2.so"),
                &root.path().join("state").join(LAUNCHER),
            )
            .unwrap();
            transaction.update(updated).unwrap();
            state.fail_directory_sync_after(successes);
            let first = transaction.rollback_update();
            state.fail_directory_sync(false);
            if first.is_err() {
                transaction.rollback_update().unwrap();
            }
            drop(transaction);
            let mut restored = ActivationTransaction::resume(&state, &target).unwrap();
            assert_eq!(
                fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
                old
            );
            assert_eq!(
                fs::read_dir(root.path().join("state")).unwrap().count(),
                3,
                "sync boundary {successes}"
            );
            assert_eq!(
                fs::read_dir(root.path().join("drop-ins")).unwrap().count(),
                1
            );
            restored.rollback().unwrap();
        }
    }

    #[test]
    fn modified_compensation_candidate_refuses_before_recovery_mutation() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        transaction.update(updated).unwrap();
        transaction.stop_after = Some((9, false));
        assert!(transaction.rollback_update().is_err());
        let candidate = transaction
            .update
            .as_ref()
            .unwrap()
            .candidates
            .last()
            .unwrap()
            .1
            .name
            .to_str()
            .unwrap()
            .to_owned();
        drop(transaction);
        fs::write(root.path().join("state").join(&candidate), b"edited").unwrap();
        let before = fs::read_dir(root.path().join("state")).unwrap().count();
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        assert_eq!(
            fs::read_dir(root.path().join("state")).unwrap().count(),
            before
        );
        assert_eq!(
            fs::read(root.path().join("state").join(&candidate)).unwrap(),
            b"edited"
        );
    }

    #[test]
    fn identical_unknown_active_manifest_is_not_compensation_authority() {
        use crate::integrations::gnome::transaction::MANIFEST;
        use std::os::unix::fs::PermissionsExt;
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        transaction.update(updated).unwrap();
        transaction.stop_after = Some((10, false));
        assert!(transaction.rollback_update().is_err());
        drop(transaction);
        let active = root.path().join("state").join(MANIFEST);
        let bytes = fs::read(&active).unwrap();
        fs::rename(&active, root.path().join("state").join("foreign-retained")).unwrap();
        fs::write(&active, &bytes).unwrap();
        fs::set_permissions(&active, fs::Permissions::from_mode(0o600)).unwrap();
        let before = fs::read_dir(root.path().join("state")).unwrap().count();
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        assert_eq!(fs::read(&active).unwrap(), bytes);
        assert_eq!(
            fs::read_dir(root.path().join("state")).unwrap().count(),
            before
        );
    }

    #[test]
    fn compensation_recovery_sync_boundaries_can_resume_again() {
        for successes in 0..=14 {
            let (root, state, target, initial) = fixture();
            let old = initial.launcher.clone();
            let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
            transaction.apply().unwrap();
            let updated = render_activation(
                &root.path().join("native-v2.so"),
                &root.path().join("state").join(LAUNCHER),
            )
            .unwrap();
            transaction.update(updated).unwrap();
            transaction.stop_after = Some((9, false));
            assert!(transaction.rollback_update().is_err());
            drop(transaction);
            state.fail_directory_sync_after(successes);
            drop(ActivationTransaction::resume(&state, &target));
            state.fail_directory_sync(false);
            let mut restored = ActivationTransaction::resume(&state, &target).unwrap();
            assert_eq!(
                fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
                old
            );
            assert_eq!(
                fs::read_dir(root.path().join("state")).unwrap().count(),
                3,
                "recovery sync boundary {successes}"
            );
            restored.rollback().unwrap();
        }
    }

    #[test]
    fn abrupt_compensation_child() {
        use std::{os::unix::fs::MetadataExt, path::Path};
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
        let point: u8 = std::env::var("LYN_PRIVATE_INSTALL_RECOVERY_POINT")
            .unwrap()
            .parse()
            .unwrap();
        assert!((9..=16).contains(&point));
        let state_path = root.join("state");
        let state =
            crate::integrations::gnome::directories::open_private(&state_path, false).unwrap();
        let target =
            crate::integrations::gnome::directories::open_drop_ins(&root.join("drop-ins"), false)
                .unwrap();
        let initial =
            render_activation(&root.join("native.so"), &state_path.join(LAUNCHER)).unwrap();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        transaction.stop_after = Some((5 + (point - 9) / 2, false));
        let updated =
            render_activation(&root.join("native-v2.so"), &state_path.join(LAUNCHER)).unwrap();
        assert!(transaction.update(updated).is_err());
        transaction.stop_after = Some((9 + (point - 9) % 2, true));
        transaction.rollback_update().unwrap();
        panic!("compensation checkpoint did not exit");
    }

    #[test]
    fn eight_compensation_process_exits_restore_exact_old_files_without_orphans() {
        use std::os::unix::fs::PermissionsExt;
        for point in 9..=16 {
            let root = tempfile::Builder::new()
                .prefix("lyn-install-recovery-")
                .tempdir()
                .unwrap();
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
            let state_path = root.path().join("state");
            let target_path = root.path().join("drop-ins");
            let state =
                crate::integrations::gnome::directories::open_private(&state_path, true).unwrap();
            drop(
                crate::integrations::gnome::directories::open_drop_ins(&target_path, true).unwrap(),
            );
            fs::set_permissions(&target_path, fs::Permissions::from_mode(0o755)).unwrap();
            let target =
                crate::integrations::gnome::directories::open_drop_ins(&target_path, false)
                    .unwrap();
            fs::write(target_path.join("user.conf"), b"untouched").unwrap();
            crate::integrations::gnome::transaction::recovery::tests::abrupt_exit(
                root.path(),
                point,
                "integrations::gnome::transaction::update::precommit::compensation::tests::abrupt_compensation_child",
            );
            let expected =
                render_activation(&root.path().join("native.so"), &state_path.join(LAUNCHER))
                    .unwrap();
            let mut restored = ActivationTransaction::resume(&state, &target).unwrap();
            assert_eq!(
                fs::read_to_string(state_path.join(LAUNCHER)).unwrap(),
                expected.launcher
            );
            assert_eq!(
                fs::read_to_string(target_path.join(DROP_IN)).unwrap(),
                expected.drop_in
            );
            assert_eq!(
                fs::read(target_path.join("user.conf")).unwrap(),
                b"untouched"
            );
            assert_eq!(
                fs::metadata(&target_path).unwrap().permissions().mode() & 0o7777,
                0o755
            );
            assert_eq!(
                fs::read_dir(&state_path).unwrap().count(),
                3,
                "point {point}"
            );
            assert_eq!(fs::read_dir(&target_path).unwrap().count(), 2);
            restored.rollback().unwrap();
        }
    }

    #[test]
    fn interrupted_live_compensation_recovers_without_leaving_its_displaced_manifest() {
        let (root, state, target, initial) = fixture();
        let old_launcher = initial.launcher.clone();
        let old_drop = initial.drop_in.clone();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        transaction.update(updated).unwrap();
        state.fail_directory_sync_after(3);
        assert!(transaction.rollback_update().is_err());
        assert!(transaction.update.as_ref().unwrap().manifest_refreshed);
        drop(transaction);
        state.fail_directory_sync(false);
        let mut restored = ActivationTransaction::resume(&state, &target).unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
            old_launcher
        );
        assert_eq!(
            fs::read_to_string(root.path().join("drop-ins").join(DROP_IN)).unwrap(),
            old_drop
        );
        assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 3);
        assert_eq!(
            fs::read_dir(root.path().join("drop-ins")).unwrap().count(),
            1
        );
        restored.rollback().unwrap();
    }
}
