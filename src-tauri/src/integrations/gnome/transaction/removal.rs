//! Persistent authority for explicit removal of a complete installation.

use super::{ActivationTransaction, DROP_IN, LAUNCHER, MANIFEST, Manifest, Phase, Place, refused};
use crate::integrations::gnome::{
    RenderedActivation,
    artifact::format,
    lock::InstallationLock,
    managed::{ManagedDirectory, OwnedFile, PublishFailure},
    ownership::Receipt,
    render_activation_with_previous,
};
use serde::{Deserialize, Serialize};
use std::{
    io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

pub(super) const PENDING_REMOVAL: &str = "uninstall-pending-v1.json";
pub(in crate::integrations::gnome) const RETIRED_REMOVAL: &str = "uninstall-retired-v1.json";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivationReference {
    digest: String,
    previous: Option<String>,
}

impl ActivationReference {
    fn validate(&self) -> io::Result<()> {
        let digest = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        if !digest(&self.digest)
            || self
                .previous
                .as_ref()
                .is_some_and(|previous| !digest(previous) || previous == &self.digest)
        {
            return Err(refused());
        }
        Ok(())
    }

    fn render(&self, state_path: &Path) -> io::Result<RenderedActivation> {
        self.validate()?;
        let module = state_path
            .join("modules")
            .join(&self.digest)
            .join(format::MODULE_NAME);
        render_activation_with_previous(
            &module,
            &state_path.join(LAUNCHER),
            self.previous.as_deref(),
        )
        .map_err(|_| refused())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Removal {
    version: u8,
    launcher: Receipt,
    drop_in: Receipt,
    manifest: Receipt,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    activation: Option<ActivationReference>,
}

impl Removal {
    fn validate(&self) -> io::Result<()> {
        self.launcher.validate()?;
        self.drop_in.validate()?;
        self.manifest.validate()?;
        if !matches!((self.version, &self.activation), (1, None) | (2, Some(_)))
            || !self.launcher.is_for(LAUNCHER, 0o700)
            || !self.drop_in.is_for(DROP_IN, 0o600)
            || !self.manifest.is_for(MANIFEST, 0o600)
        {
            return Err(refused());
        }
        if let Some(reference) = &self.activation {
            reference.validate()?;
        }
        Ok(())
    }

    fn render_retired(&self, state_path: &Path) -> io::Result<RenderedActivation> {
        let rendered = self
            .activation
            .as_ref()
            .ok_or_else(refused)?
            .render(state_path)?;
        self.launcher.check_content(rendered.launcher.as_bytes())?;
        self.drop_in.check_content(rendered.drop_in.as_bytes())?;
        Ok(rendered)
    }

    fn check_manifest(&self, file: &OwnedFile) -> io::Result<()> {
        let body: Manifest = serde_json::from_slice(&file.content).map_err(|_| refused())?;
        if body.version != 1 || body.launcher != self.launcher || body.drop_in != self.drop_in {
            return Err(refused());
        }
        Ok(())
    }
}

fn no_other_intents(state: &ManagedDirectory) -> io::Result<()> {
    for name in [
        super::recovery::PENDING_INSTALL,
        super::update::CLEANUP_PENDING,
        super::update::PRECOMMIT_PENDING,
        super::update::COMPENSATION_PENDING,
    ] {
        state.require_absent(name)?;
    }
    for name in super::update::EXCHANGE_NAMES {
        state.require_absent(name)?;
    }
    Ok(())
}

pub(in crate::integrations::gnome) struct RetirementSnapshot {
    record: OwnedFile,
    rendered: RenderedActivation,
    state_path: PathBuf,
}

pub(in crate::integrations::gnome) struct RemovalSnapshot {
    journal: OwnedFile,
    rendered: RenderedActivation,
    state_path: PathBuf,
    survivors: [Option<OwnedFile>; 3],
}

impl RemovalSnapshot {
    pub(in crate::integrations::gnome) fn open(
        state: &ManagedDirectory,
        target: &ManagedDirectory,
        state_path: &Path,
    ) -> io::Result<Self> {
        no_other_intents(state)?;
        state.require_absent(RETIRED_REMOVAL)?;
        let journal = state.read_state(PENDING_REMOVAL)?;
        let body: Removal = serde_json::from_slice(&journal.content).map_err(|_| refused())?;
        body.validate()?;
        if body.version != 2 {
            return Err(refused());
        }
        body.launcher.check_directory(state)?;
        body.drop_in.check_directory(target)?;
        body.manifest.check_directory(state)?;
        let rendered = body.render_retired(state_path)?;
        let reopen =
            |receipt: &Receipt, directory: &ManagedDirectory| match receipt.reopen(directory) {
                Ok(file) => Ok(Some(file)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error),
            };
        let survivors = [
            reopen(&body.launcher, state)?,
            reopen(&body.drop_in, target)?,
            reopen(&body.manifest, state)?,
        ];
        if let Some(manifest) = &survivors[2] {
            body.check_manifest(manifest)?;
        }
        Ok(Self {
            journal,
            rendered,
            state_path: state_path.into(),
            survivors,
        })
    }

    pub(in crate::integrations::gnome) fn contents(&self) -> (&[u8], &[u8]) {
        (
            self.rendered.launcher.as_bytes(),
            self.rendered.drop_in.as_bytes(),
        )
    }

    pub(in crate::integrations::gnome) fn drop_in_metadata(
        &self,
    ) -> io::Result<Option<std::fs::Metadata>> {
        self.survivors[1]
            .as_ref()
            .map(|file| file.file.metadata())
            .transpose()
    }

    pub(in crate::integrations::gnome) fn recheck(
        &self,
        state: &ManagedDirectory,
        target: &ManagedDirectory,
    ) -> io::Result<()> {
        state.verify(&self.journal)?;
        let current = Self::open(state, target, &self.state_path)?;
        for ((before, after), directory) in self
            .survivors
            .iter()
            .zip(&current.survivors)
            .zip([state, target, state])
        {
            match (before, after) {
                (Some(file), Some(_)) => directory.verify(file)?,
                (None, None) => {}
                _ => return Err(refused()),
            }
        }
        state.verify(&self.journal)
    }
}

impl<'a> ActivationTransaction<'a> {
    pub(in crate::integrations::gnome) fn resume_retirement_checked(
        state: &'a ManagedDirectory,
        target: &'a ManagedDirectory,
        state_path: &Path,
        check: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<Self> {
        let lock = InstallationLock::acquire(state)?;
        if super::pending(state, PENDING_REMOVAL)? {
            let mut snapshot = RemovalSnapshot::open(state, target, state_path)?;
            check()?;
            snapshot.recheck(state, target)?;
            finish(state, target, &lock, &mut snapshot.journal, |_| Ok(()))?;
        } else {
            let snapshot = RetirementSnapshot::open(state, target, state_path)?;
            check()?;
            snapshot.recheck(state, target)?;
            // A previous rename may have succeeded before its sync failed.
            state.sync_directory()?;
            target.sync_directory()?;
        }
        lock.check()?;
        Ok(Self {
            state,
            drop_ins: target,
            lock,
            rendered: None,
            phase: Phase::Removed,
            files: Vec::new(),
            stages: Vec::new(),
            update: None,
            removal_record: None,
            #[cfg(test)]
            stop_after: None,
        })
    }
}

impl RetirementSnapshot {
    pub(in crate::integrations::gnome) fn open(
        state: &ManagedDirectory,
        target: &ManagedDirectory,
        state_path: &Path,
    ) -> io::Result<Self> {
        no_other_intents(state)?;
        state.require_absent(PENDING_REMOVAL)?;
        state.require_absent(LAUNCHER)?;
        state.require_absent(MANIFEST)?;
        target.require_absent(DROP_IN)?;
        let record = state.read_state(RETIRED_REMOVAL)?;
        let body: Removal = serde_json::from_slice(&record.content).map_err(|_| refused())?;
        body.validate()?;
        if body.version != 2 {
            return Err(refused());
        }
        body.launcher.check_directory(state)?;
        body.manifest.check_directory(state)?;
        body.drop_in.check_directory(target)?;
        let rendered = body.render_retired(state_path)?;
        Ok(Self {
            record,
            rendered,
            state_path: state_path.into(),
        })
    }

    pub(in crate::integrations::gnome) fn contents(&self) -> (&[u8], &[u8]) {
        (
            self.rendered.launcher.as_bytes(),
            self.rendered.drop_in.as_bytes(),
        )
    }

    pub(in crate::integrations::gnome) fn recheck(
        &self,
        state: &ManagedDirectory,
        target: &ManagedDirectory,
    ) -> io::Result<()> {
        state.verify(&self.record)?;
        Self::open(state, target, &self.state_path)?;
        state.verify(&self.record)
    }
}

fn finish(
    state: &ManagedDirectory,
    target: &ManagedDirectory,
    lock: &InstallationLock<'_>,
    journal: &mut OwnedFile,
    checkpoint: impl Fn(u8) -> io::Result<()>,
) -> io::Result<()> {
    lock.check()?;
    no_other_intents(state)?;
    state.verify(journal)?;
    let body: Removal = serde_json::from_slice(&journal.content).map_err(|_| refused())?;
    body.validate()?;
    if journal.name.to_bytes() == RETIRED_REMOVAL.as_bytes() {
        if body.version != 2 {
            return Err(refused());
        }
        state.require_absent(PENDING_REMOVAL)?;
        state.require_absent(LAUNCHER)?;
        state.require_absent(MANIFEST)?;
        target.require_absent(DROP_IN)?;
        state.sync_directory()?;
        return target.sync_directory();
    }
    state.require_absent(RETIRED_REMOVAL)?;
    // Missing files may have been removed only after this durable authority.
    // Every remaining name is checked before any subsequent deletion.
    let mut files = Vec::new();
    for (directory, receipt) in [
        (target, &body.drop_in),
        (state, &body.launcher),
        (state, &body.manifest),
    ] {
        let file = match receipt.reopen(directory) {
            Ok(file) => Some(file),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        files.push((directory, file));
    }
    if let Some(file) = &files[2].1 {
        body.check_manifest(file)?;
    }
    state.sync_directory()?;
    // Remove the active override before its launcher, retaining the manifest
    // until last. This is file-state ordering, not a manager reload.
    for (index, (directory, file)) in files.into_iter().enumerate() {
        lock.check()?;
        if let Some(file) = file {
            directory.remove(&file)?;
        }
        checkpoint(13 + index as u8)?;
    }
    state.sync_directory()?;
    target.sync_directory()?;
    lock.check()?;
    if body.activation.is_some() {
        state
            .move_to_unoccupied(journal, RETIRED_REMOVAL)?
            .durability
    } else {
        state.remove(journal)
    }
}

pub(super) fn recover_removal(
    state: &ManagedDirectory,
    target: &ManagedDirectory,
    lock: &InstallationLock<'_>,
) -> io::Result<bool> {
    lock.check()?;
    let mut journal = match state.read_state(PENDING_REMOVAL) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if super::pending(state, LAUNCHER)?
                || super::pending(state, MANIFEST)?
                || super::pending(target, DROP_IN)?
            {
                return Ok(false);
            }
            match no_other_intents(state) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
                Err(error) => return Err(error),
            }
            // Empty is also the settled uninstalled state. Flush both parents
            // so a fresh retry cannot forget a failed final-journal sync.
            state.sync_directory()?;
            target.sync_directory()?;
            return Ok(true);
        }
        Err(error) => return Err(error),
    };
    let body: Removal = serde_json::from_slice(&journal.content).map_err(|_| refused())?;
    body.validate()?;
    // V2 needs the internally resolved factory path and hash reconstruction.
    // The generic filesystem resume API has no such authority.
    if body.version != 1 {
        return Err(refused());
    }
    finish(state, target, lock, &mut journal, |_| Ok(()))?;
    Ok(true)
}

impl ActivationTransaction<'_> {
    pub(super) fn remove_complete(&mut self) -> io::Result<()> {
        self.remove_complete_kind(None)
    }

    pub(in crate::integrations::gnome) fn retire_activation(
        &mut self,
        state_path: &Path,
        digest: &str,
        previous: Option<&str>,
    ) -> io::Result<()> {
        let reference = ActivationReference {
            digest: digest.into(),
            previous: previous.map(str::to_owned),
        };
        self.remove_complete_kind(Some((state_path, &reference)))
    }

    fn remove_complete_kind(
        &mut self,
        retain: Option<(&Path, &ActivationReference)>,
    ) -> io::Result<()> {
        self.lock.check()?;
        if self.phase == Phase::Complete {
            if self.files.len() != 3 {
                return Err(refused());
            }
            no_other_intents(self.state)?;
            self.state.require_absent(RETIRED_REMOVAL)?;
            self.state.require_absent(PENDING_REMOVAL)?;
            for (place, file) in &self.files {
                self.directory(*place).verify(file)?;
            }
            while let Some((place, stage)) = self.stages.last() {
                self.directory(*place).discard_stage(stage)?;
                self.stages.pop();
            }
            let body = Removal {
                version: if retain.is_some() { 2 } else { 1 },
                launcher: Receipt::record(self.state, &self.files[0].1)?,
                drop_in: Receipt::record(self.drop_ins, &self.files[1].1)?,
                manifest: Receipt::record(self.state, &self.files[2].1)?,
                activation: retain.map(|(_, reference)| reference.clone()),
            };
            body.validate()?;
            if let Some((state_path, _)) = retain {
                body.render_retired(state_path)?;
            }
            body.check_manifest(&self.files[2].1)?;
            let bytes = serde_json::to_vec(&body).map_err(io::Error::other)?;
            let (journal, result) = match self.state.publish(PENDING_REMOVAL, &bytes, false) {
                Ok(file) => (file, Ok(())),
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
            self.removal_record = Some(journal);
            self.phase = Phase::Removing;
            result?;
            self.removal_checkpoint(12)?;
        }
        #[cfg(test)]
        let stop_after = self.stop_after;
        let journal = self.removal_record.as_mut().ok_or_else(refused)?;
        if journal.file.metadata()?.nlink() == 0 {
            let body: Removal = serde_json::from_slice(&journal.content).map_err(|_| refused())?;
            body.validate()?;
            if body.version != 1 {
                return Err(refused());
            }
            // Retirement may have unlinked successfully but failed to sync.
            self.state.require_absent(PENDING_REMOVAL)?;
            no_other_intents(self.state)?;
            self.state.require_absent(LAUNCHER)?;
            self.state.require_absent(MANIFEST)?;
            self.drop_ins.require_absent(DROP_IN)?;
            self.state.sync_directory()?;
            self.drop_ins.sync_directory()?;
        } else {
            finish(self.state, self.drop_ins, &self.lock, journal, |point| {
                #[cfg(test)]
                if let Some((stop, abrupt)) = stop_after {
                    if point == stop {
                        if abrupt {
                            std::process::exit(77);
                        }
                        return Err(io::Error::other("private removal checkpoint"));
                    }
                }
                let _ = point;
                Ok(())
            })?;
        }
        self.removal_record = None;
        self.files.clear();
        self.phase = Phase::Removed;
        Ok(())
    }

    fn removal_checkpoint(&self, _point: u8) -> io::Result<()> {
        #[cfg(test)]
        if let Some((point, abrupt)) = self.stop_after {
            if point == _point {
                if abrupt {
                    std::process::exit(77);
                }
                return Err(io::Error::other("private removal checkpoint"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::fixture;
    use super::*;
    use std::fs;

    fn retirement_fixture() -> (
        tempfile::TempDir,
        ManagedDirectory,
        ManagedDirectory,
        RenderedActivation,
    ) {
        let (root, state, target, _) = fixture();
        let reference = ActivationReference {
            digest: "a".repeat(64),
            previous: None,
        };
        let rendered = reference.render(&root.path().join("state")).unwrap();
        (root, state, target, rendered)
    }

    fn retire(transaction: &mut ActivationTransaction<'_>, root: &Path) -> io::Result<()> {
        transaction.retire_activation(&root.join("state"), &"a".repeat(64), None)
    }

    #[test]
    fn retirement_preserves_the_same_journal_inode_and_rechecks_absence() {
        let (root, state, target, rendered) = retirement_fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        transaction.stop_after = Some((12, false));
        assert!(retire(&mut transaction, root.path()).is_err());
        let original_inode = fs::metadata(root.path().join("state").join(PENDING_REMOVAL))
            .unwrap()
            .ino();
        transaction.stop_after = None;
        retire(&mut transaction, root.path()).unwrap();
        let retired =
            RetirementSnapshot::open(&state, &target, &root.path().join("state")).unwrap();
        assert_eq!(
            retired.record.file.metadata().unwrap().ino(),
            original_inode
        );
        retired.recheck(&state, &target).unwrap();
        assert!(!root.path().join("state").join(PENDING_REMOVAL).exists());
        fs::write(root.path().join("drop-ins").join(DROP_IN), b"foreign").unwrap();
        assert!(retired.recheck(&state, &target).is_err());
    }

    #[test]
    fn retirement_rename_sync_failure_retries_the_moved_lease() {
        let (root, state, target, rendered) = retirement_fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        transaction.stop_after = Some((15, false));
        assert!(retire(&mut transaction, root.path()).is_err());
        transaction.stop_after = None;
        state.fail_directory_sync_after(2);
        assert!(retire(&mut transaction, root.path()).is_err());
        assert!(root.path().join("state").join(RETIRED_REMOVAL).is_file());
        assert!(!root.path().join("state").join(PENDING_REMOVAL).exists());
        state.fail_directory_sync(false);
        retire(&mut transaction, root.path()).unwrap();
        RetirementSnapshot::open(&state, &target, &root.path().join("state"))
            .unwrap()
            .recheck(&state, &target)
            .unwrap();
    }

    #[test]
    fn retirement_rename_sync_failure_reopens_as_guarded_retired_retry() {
        let (root, state, target, rendered) = retirement_fixture();
        let state_path = root.path().join("state");
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        transaction.stop_after = Some((15, false));
        assert!(retire(&mut transaction, root.path()).is_err());
        transaction.stop_after = None;
        state.fail_directory_sync_after(2);
        assert!(retire(&mut transaction, root.path()).is_err());
        drop(transaction);
        state.fail_directory_sync(false);
        let original_inode = fs::metadata(state_path.join(RETIRED_REMOVAL))
            .unwrap()
            .ino();
        let recovered =
            ActivationTransaction::resume_retirement_checked(&state, &target, &state_path, || {
                Ok(())
            })
            .unwrap();
        assert!(recovered.phase == Phase::Removed);
        assert_eq!(
            fs::metadata(state_path.join(RETIRED_REMOVAL))
                .unwrap()
                .ino(),
            original_inode
        );
        RetirementSnapshot::open(&state, &target, &state_path)
            .unwrap()
            .recheck(&state, &target)
            .unwrap();
    }

    #[test]
    fn retirement_refuses_an_occupied_record_before_removing_activation() {
        let (root, state, target, rendered) = retirement_fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        fs::write(root.path().join("state").join(RETIRED_REMOVAL), b"unknown").unwrap();
        assert!(retire(&mut transaction, root.path()).is_err());
        assert!(root.path().join("state").join(LAUNCHER).exists());
        assert!(root.path().join("drop-ins").join(DROP_IN).exists());
        assert!(!root.path().join("state").join(PENDING_REMOVAL).exists());
    }

    #[test]
    fn retirement_cannot_accept_an_unlinked_authority_as_a_finished_move() {
        let (root, state, target, rendered) = retirement_fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        transaction.stop_after = Some((15, false));
        assert!(retire(&mut transaction, root.path()).is_err());
        fs::remove_file(root.path().join("state").join(PENDING_REMOVAL)).unwrap();
        transaction.stop_after = None;
        assert!(retire(&mut transaction, root.path()).is_err());
        assert!(!root.path().join("state").join(RETIRED_REMOVAL).exists());
    }

    #[test]
    fn retirement_generic_resume_requires_reconstruction_context_before_deleting() {
        for edit in [false, true] {
            let (root, state, target, rendered) = retirement_fixture();
            let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
            transaction.apply().unwrap();
            transaction.stop_after = Some((12, false));
            assert!(retire(&mut transaction, root.path()).is_err());
            drop(transaction);
            if edit {
                let path = root.path().join("state").join(PENDING_REMOVAL);
                let mut body: serde_json::Value =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                body["activation"]["digest"] = "b".repeat(64).into();
                fs::write(path, serde_json::to_vec(&body).unwrap()).unwrap();
            }
            assert!(ActivationTransaction::resume(&state, &target).is_err());
            assert!(root.path().join("state").join(LAUNCHER).is_file());
            assert!(root.path().join("state").join(MANIFEST).is_file());
            assert!(root.path().join("drop-ins").join(DROP_IN).is_file());
            assert!(!root.path().join("state").join(RETIRED_REMOVAL).exists());
        }
    }

    #[test]
    fn interrupted_uninstall_resumes_after_a_successful_unlink_with_failed_sync() {
        let (root, state, target, rendered) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        target.fail_directory_sync(true);
        assert!(transaction.rollback().is_err());
        drop(transaction);
        target.fail_directory_sync(false);
        let mut removed = ActivationTransaction::resume(&state, &target).unwrap();
        assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 1);
        assert_eq!(
            fs::read_dir(root.path().join("drop-ins")).unwrap().count(),
            0
        );
        removed.rollback().unwrap();
    }

    #[test]
    fn abrupt_uninstall_child() {
        abrupt_removal_child(false);
    }

    #[test]
    fn abrupt_retirement_child() {
        abrupt_removal_child(true);
    }

    fn abrupt_removal_child(retain: bool) {
        use std::path::Path;
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
        assert!((12..=15).contains(&point));
        let state_path = root.join("state");
        let state =
            crate::integrations::gnome::directories::open_private(&state_path, false).unwrap();
        let target =
            crate::integrations::gnome::directories::open_drop_ins(&root.join("drop-ins"), false)
                .unwrap();
        let rendered = if retain {
            ActivationReference {
                digest: "a".repeat(64),
                previous: None,
            }
            .render(&state_path)
            .unwrap()
        } else {
            crate::integrations::gnome::render_activation(
                &root.join("native.so"),
                &state_path.join(LAUNCHER),
            )
            .unwrap()
        };
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        transaction.stop_after = Some((point, true));
        if retain {
            transaction
                .retire_activation(&state_path, &"a".repeat(64), None)
                .unwrap();
        } else {
            transaction.rollback().unwrap();
        }
        panic!("removal checkpoint did not exit");
    }

    #[test]
    fn four_retirement_process_exits_recover_with_exact_factory_authority() {
        use std::os::unix::fs::PermissionsExt;
        for point in 12..=15 {
            let root = tempfile::Builder::new()
                .prefix("lyn-install-recovery-")
                .tempdir()
                .unwrap();
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
            let state_path = root.path().join("state");
            let state =
                crate::integrations::gnome::directories::open_private(&state_path, true).unwrap();
            let target_path = root.path().join("drop-ins");
            let target =
                crate::integrations::gnome::directories::open_drop_ins(&target_path, true).unwrap();
            fs::write(target_path.join("preferences.txt"), b"user settings").unwrap();
            super::super::recovery::tests::abrupt_exit(
                root.path(),
                point,
                "integrations::gnome::transaction::removal::tests::abrupt_retirement_child",
            );
            let journal_inode = fs::metadata(state_path.join(PENDING_REMOVAL))
                .unwrap()
                .ino();
            let recovered = ActivationTransaction::resume_retirement_checked(
                &state,
                &target,
                &state_path,
                || Ok(()),
            )
            .unwrap();
            assert!(recovered.phase == Phase::Removed);
            let retired = RetirementSnapshot::open(&state, &target, &state_path).unwrap();
            assert_eq!(retired.record.file.metadata().unwrap().ino(), journal_inode);
            retired.recheck(&state, &target).unwrap();
            assert_eq!(
                fs::read(target_path.join("preferences.txt")).unwrap(),
                b"user settings"
            );
            assert!(!state_path.join(PENDING_REMOVAL).exists());
        }
    }

    #[test]
    fn four_uninstall_process_exits_remove_only_owned_files_and_allow_reinstall() {
        use std::os::unix::fs::PermissionsExt;
        for point in 12..=15 {
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
            fs::write(target_path.join("user.conf"), b"preserved").unwrap();
            super::super::recovery::tests::abrupt_exit(
                root.path(),
                point,
                "integrations::gnome::transaction::removal::tests::abrupt_uninstall_child",
            );
            let removed = ActivationTransaction::resume(&state, &target).unwrap();
            assert!(removed.phase == Phase::Removed);
            drop(removed);
            assert_eq!(fs::read_dir(&state_path).unwrap().count(), 1);
            assert_eq!(fs::read_dir(&target_path).unwrap().count(), 1);
            assert_eq!(
                fs::read(target_path.join("user.conf")).unwrap(),
                b"preserved"
            );
            assert_eq!(fs::metadata(&target_path).unwrap().mode() & 0o7777, 0o755);
            let rendered = crate::integrations::gnome::render_activation(
                &root.path().join("native.so"),
                &state_path.join(LAUNCHER),
            )
            .unwrap();
            let mut reinstalled = ActivationTransaction::begin(&state, &target, rendered).unwrap();
            reinstalled.apply().unwrap();
            reinstalled.rollback().unwrap();
        }
    }

    #[test]
    fn uninstall_sync_failures_retry_live_and_after_reopening() {
        for reopen in [false, true] {
            for successes in 0..=8 {
                let (root, state, target, rendered) = fixture();
                let mut transaction =
                    ActivationTransaction::begin(&state, &target, rendered).unwrap();
                transaction.apply().unwrap();
                state.fail_directory_sync_after(successes);
                let result = transaction.rollback();
                if successes == 0 {
                    assert!(transaction.phase == Phase::Removing);
                    assert!(root.path().join("state").join(LAUNCHER).exists());
                    assert!(root.path().join("state").join(MANIFEST).exists());
                    assert!(root.path().join("drop-ins").join(DROP_IN).exists());
                }
                if reopen {
                    drop(transaction);
                    state.fail_directory_sync(false);
                    let removed = ActivationTransaction::resume(&state, &target).unwrap();
                    assert!(removed.phase == Phase::Removed);
                } else {
                    state.fail_directory_sync(false);
                    if result.is_err() {
                        transaction.rollback().unwrap();
                    }
                    assert!(transaction.phase == Phase::Removed);
                }
                assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 1);
                assert_eq!(
                    fs::read_dir(root.path().join("drop-ins")).unwrap().count(),
                    0
                );
            }
        }
    }

    #[test]
    fn replaced_remaining_file_refuses_before_any_other_removal() {
        use std::os::unix::fs::PermissionsExt;
        let (root, state, target, rendered) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        transaction.stop_after = Some((13, false));
        assert!(transaction.rollback().is_err());
        drop(transaction);
        let launcher = root.path().join("state").join(LAUNCHER);
        fs::rename(&launcher, root.path().join("state").join("retained")).unwrap();
        fs::write(&launcher, b"user file").unwrap();
        fs::set_permissions(&launcher, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        assert_eq!(fs::read(&launcher).unwrap(), b"user file");
        assert!(root.path().join("state").join(MANIFEST).exists());
        assert!(root.path().join("state").join(PENDING_REMOVAL).exists());
    }

    #[test]
    fn edited_or_conflicting_uninstall_authority_refuses_without_deleting_files() {
        for conflict in [false, true] {
            let (root, state, target, rendered) = fixture();
            let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
            transaction.apply().unwrap();
            transaction.stop_after = Some((12, false));
            assert!(transaction.rollback().is_err());
            drop(transaction);
            if conflict {
                fs::write(
                    root.path()
                        .join("state")
                        .join(super::super::update::PRECOMMIT_PENDING),
                    b"other intent",
                )
                .unwrap();
            } else {
                fs::write(root.path().join("state").join(PENDING_REMOVAL), b"edited").unwrap();
            }
            assert!(ActivationTransaction::resume(&state, &target).is_err());
            assert!(root.path().join("state").join(LAUNCHER).exists());
            assert!(root.path().join("drop-ins").join(DROP_IN).exists());
            assert!(root.path().join("state").join(MANIFEST).exists());
        }
    }

    #[test]
    fn foreign_uninstall_intent_refuses_live_update_and_uninstall() {
        let (root, state, target, rendered) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        let state_path = root.path().join("state");
        let before = fs::read(state_path.join(LAUNCHER)).unwrap();
        fs::write(state_path.join(PENDING_REMOVAL), b"foreign").unwrap();
        let new = crate::integrations::gnome::render_activation(
            &root.path().join("native-v2.so"),
            &state_path.join(LAUNCHER),
        )
        .unwrap();
        assert!(transaction.update(new).is_err());
        assert!(transaction.rollback().is_err());
        assert_eq!(fs::read(state_path.join(LAUNCHER)).unwrap(), before);
        assert_eq!(
            fs::read(state_path.join(PENDING_REMOVAL)).unwrap(),
            b"foreign"
        );
        assert_eq!(fs::read_dir(state_path).unwrap().count(), 4);
        assert_eq!(
            fs::read_dir(root.path().join("drop-ins")).unwrap().count(),
            1
        );
    }

    #[test]
    fn empty_uninstalled_state_still_syncs_both_parents_on_fresh_resume() {
        let (_root, state, target, rendered) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        transaction.rollback().unwrap();
        drop(transaction);
        target.fail_directory_sync(true);
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        target.fail_directory_sync(false);
        assert!(
            ActivationTransaction::resume(&state, &target)
                .unwrap()
                .phase
                == Phase::Removed
        );
    }
}
