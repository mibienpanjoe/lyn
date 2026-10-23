//! Private synchronous activation-file transaction. Never reloads a manager or
//! restarts GNOME. Drop releases the lock, never silently removes files.

mod recovery;
mod update;

use super::{
    RenderedActivation,
    lock::InstallationLock,
    managed::{ManagedDirectory, OwnedFile, PublishFailure, StagedFile},
    ownership::Receipt,
};
use serde::{Deserialize, Serialize};
use std::{io, os::unix::fs::MetadataExt};

const LAUNCHER: &str = "launcher-v1";
const DROP_IN: &str = "90-lyn-native.conf";
const MANIFEST: &str = "installation-v1.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u8,
    launcher: Receipt,
    drop_in: Receipt,
}

#[derive(Clone, Copy)]
enum Place {
    State,
    DropIns,
}

#[derive(PartialEq, Eq)]
enum Phase {
    New,
    Partial,
    Complete,
    Updating,
    Removed,
}

pub(super) struct ActivationTransaction<'a> {
    state: &'a ManagedDirectory,
    drop_ins: &'a ManagedDirectory,
    lock: InstallationLock<'a>,
    rendered: Option<RenderedActivation>,
    phase: Phase,
    files: Vec<(Place, OwnedFile)>,
    stages: Vec<(Place, StagedFile)>,
    update: Option<update::PendingUpdate>,
    #[cfg(test)]
    stop_after: Option<(u8, bool)>,
}

fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "activation transaction refused",
    )
}

fn pending(directory: &ManagedDirectory, name: &str) -> io::Result<bool> {
    match directory.require_absent(name) {
        Ok(()) => Ok(false),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(true),
        Err(error) => Err(error),
    }
}

impl<'a> ActivationTransaction<'a> {
    /// The internal caller renders the launcher with state/launcher-v1 as its
    /// exact destination. No user payload or UI-provided path is accepted here.
    pub(super) fn begin(
        state: &'a ManagedDirectory,
        drop_ins: &'a ManagedDirectory,
        rendered: RenderedActivation,
    ) -> io::Result<Self> {
        let lock = InstallationLock::acquire(state)?;
        state.require_absent(LAUNCHER)?;
        state.require_absent(MANIFEST)?;
        state.require_absent(recovery::PENDING_INSTALL)?;
        state.require_absent(update::CLEANUP_PENDING)?;
        drop_ins.require_absent(DROP_IN)?;
        Ok(Self {
            state,
            drop_ins,
            lock,
            rendered: Some(rendered),
            phase: Phase::New,
            files: Vec::new(),
            stages: Vec::new(),
            update: None,
            #[cfg(test)]
            stop_after: None,
        })
    }

    pub(super) fn resume(
        state: &'a ManagedDirectory,
        drop_ins: &'a ManagedDirectory,
    ) -> io::Result<Self> {
        let lock = InstallationLock::acquire(state)?;
        if pending(state, recovery::PENDING_INSTALL)? && pending(state, update::CLEANUP_PENDING)? {
            return Err(refused());
        }
        recovery::recover_pending(state, drop_ins, &lock)?;
        update::recover_cleanup(state, drop_ins, &lock)?;
        let manifest_file = state.read_state(MANIFEST)?;
        let manifest: Manifest =
            serde_json::from_slice(&manifest_file.content).map_err(|_| refused())?;
        if manifest.version != 1
            || !manifest.launcher.is_for(LAUNCHER, 0o700)
            || !manifest.drop_in.is_for(DROP_IN, 0o600)
        {
            return Err(refused());
        }
        let launcher = manifest.launcher.reopen(state)?;
        let drop_in = manifest.drop_in.reopen(drop_ins)?;
        lock.check()?;
        Ok(Self {
            state,
            drop_ins,
            lock,
            rendered: None,
            phase: Phase::Complete,
            files: vec![
                (Place::State, launcher),
                (Place::DropIns, drop_in),
                (Place::State, manifest_file),
            ],
            stages: Vec::new(),
            update: None,
            #[cfg(test)]
            stop_after: None,
        })
    }

    fn directory(&self, place: Place) -> &ManagedDirectory {
        match place {
            Place::State => self.state,
            Place::DropIns => self.drop_ins,
        }
    }

    fn publish(
        &mut self,
        place: Place,
        name: &str,
        bytes: &[u8],
        executable: bool,
    ) -> io::Result<()> {
        self.lock.check()?;
        match self.directory(place).publish(name, bytes, executable) {
            Ok(owned) => {
                self.files.push((place, owned));
                Ok(())
            }
            Err(PublishFailure::Unpublished(error)) => Err(error),
            Err(PublishFailure::Published { error, owned }) => {
                self.files.push((place, owned));
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

    pub(super) fn apply(&mut self) -> io::Result<()> {
        if self.phase != Phase::New {
            return Err(refused());
        }
        self.phase = Phase::Partial;
        let rendered = self.rendered.take().ok_or_else(refused)?;
        self.apply_journaled(rendered)?;
        self.phase = Phase::Complete;
        Ok(())
    }

    /// Preflight all current files before deleting any. Failed deletions keep
    /// their ownership lease, except when the pinned inode is already unlinked.
    /// This is explicit process-local rollback, not crash recovery or update.
    pub(super) fn rollback(&mut self) -> io::Result<()> {
        if self.update.is_some() {
            return Err(refused());
        }
        self.lock.check()?;
        for (place, file) in &self.files {
            self.directory(*place).verify(file)?;
        }
        while let Some((place, stage)) = self.stages.last() {
            self.lock.check()?;
            self.directory(*place).discard_stage(stage)?;
            self.stages.pop();
        }
        while let Some((place, file)) = self.files.last() {
            self.lock.check()?;
            let result = self.directory(*place).remove(file);
            match result {
                Ok(()) => {
                    self.files.pop();
                }
                Err(error) => {
                    if file.file.metadata()?.nlink() == 0 {
                        self.files.pop();
                    }
                    return Err(error);
                }
            }
        }
        self.phase = Phase::Removed;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    pub(super) fn fixture() -> (
        tempfile::TempDir,
        ManagedDirectory,
        ManagedDirectory,
        super::super::RenderedActivation,
    ) {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let state_path = root.path().join("state");
        let target_path = root.path().join("drop-ins");
        let state = super::super::directories::open_private(&state_path, true).unwrap();
        let target = super::super::directories::open_private(&target_path, true).unwrap();
        let rendered = super::super::render_activation(
            &root.path().join("native.so"),
            &state_path.join(LAUNCHER),
        )
        .unwrap();
        (root, state, target, rendered)
    }

    #[test]
    fn install_reopen_and_uninstall_preserve_unrelated_settings_exactly() {
        let (root, state, target, rendered) = fixture();
        let expected_launcher = rendered.launcher.clone();
        let expected_drop_in = rendered.drop_in.clone();
        fs::write(
            root.path().join("drop-ins/user.conf"),
            b"[Service]\nEnvironment=EXAMPLE=kept\n",
        )
        .unwrap();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
            expected_launcher
        );
        assert_eq!(
            fs::read_to_string(root.path().join("drop-ins").join(DROP_IN)).unwrap(),
            expected_drop_in
        );
        assert!(root.path().join("state").join(MANIFEST).is_file());
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        drop(transaction);
        let mut reopened = ActivationTransaction::resume(&state, &target).unwrap();
        reopened.rollback().unwrap();
        assert!(!root.path().join("state").join(LAUNCHER).exists());
        assert!(!root.path().join("state").join(MANIFEST).exists());
        assert!(!root.path().join("drop-ins").join(DROP_IN).exists());
        assert_eq!(
            fs::read(root.path().join("drop-ins/user.conf")).unwrap(),
            b"[Service]\nEnvironment=EXAMPLE=kept\n"
        );
        assert!(root.path().join("state/.installation.lock").is_file());
    }

    #[test]
    fn conflict_after_begin_can_be_rolled_back_without_removing_the_user_override() {
        let (root, state, target, rendered) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        fs::write(root.path().join("drop-ins").join(DROP_IN), b"user override").unwrap();
        assert!(transaction.apply().is_err());
        transaction.rollback().unwrap();
        assert!(!root.path().join("state").join(LAUNCHER).exists());
        assert!(!root.path().join("state").join(MANIFEST).exists());
        assert_eq!(
            fs::read(root.path().join("drop-ins").join(DROP_IN)).unwrap(),
            b"user override"
        );
    }

    #[test]
    fn edited_file_refuses_uninstall_before_removing_any_other_artifact() {
        let (root, state, target, rendered) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        fs::write(root.path().join("drop-ins").join(DROP_IN), b"user edit").unwrap();
        assert!(transaction.rollback().is_err());
        assert!(root.path().join("state").join(LAUNCHER).is_file());
        assert!(root.path().join("state").join(MANIFEST).is_file());
        drop(transaction);
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        assert_eq!(
            fs::read(root.path().join("drop-ins").join(DROP_IN)).unwrap(),
            b"user edit"
        );
    }

    #[test]
    fn protected_0755_drop_ins_support_install_update_resume_and_uninstall() {
        let (root, state, target, rendered) = fixture();
        let target_path = root.path().join("drop-ins");
        drop(target);
        fs::set_permissions(&target_path, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(
            target_path.join("user.conf"),
            b"[Service]\nEnvironment=KEEP=1\n",
        )
        .unwrap();
        let target = super::super::directories::open_drop_ins(&target_path, false).unwrap();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        let new = super::super::render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        transaction.update(new).unwrap();
        transaction.finish_update().unwrap();
        drop(transaction);
        let mut reopened = ActivationTransaction::resume(&state, &target).unwrap();
        reopened.rollback().unwrap();
        assert_eq!(
            fs::metadata(&target_path).unwrap().permissions().mode() & 0o7777,
            0o755
        );
        assert_eq!(
            fs::read(target_path.join("user.conf")).unwrap(),
            b"[Service]\nEnvironment=KEEP=1\n"
        );
        assert_eq!(fs::read_dir(target_path).unwrap().count(), 1);
    }
}
