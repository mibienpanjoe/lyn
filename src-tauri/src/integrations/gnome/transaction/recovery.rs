//! Durable intent for first-install publication, never a generic file journal.

use super::super::{
    RenderedActivation,
    lock::InstallationLock,
    managed::{ManagedDirectory, OwnedFile, PublishFailure},
    ownership::Receipt,
};
use super::{ActivationTransaction, DROP_IN, LAUNCHER, MANIFEST, Manifest, Place, refused};
use serde::{Deserialize, Serialize};
use std::{io, os::unix::fs::MetadataExt};

pub(super) const PENDING_INSTALL: &str = "installation-pending-v1.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallIntent {
    version: u8,
    nonce: uuid::Uuid,
    launcher: Receipt,
    drop_in: Receipt,
}

impl InstallIntent {
    fn launcher_name(&self) -> String {
        format!(".lyn-install-{}-launcher", self.nonce)
    }
    fn drop_in_name(&self) -> String {
        format!(".lyn-install-{}-drop-in", self.nonce)
    }
    fn validate(&self) -> io::Result<()> {
        if self.version != 1
            || !self.launcher.is_for(&self.launcher_name(), 0o700)
            || !self.drop_in.is_for(&self.drop_in_name(), 0o600)
        {
            return Err(refused());
        }
        Ok(())
    }
}

fn manifest_bytes(
    state: &ManagedDirectory,
    drop_ins: &ManagedDirectory,
    launcher: &OwnedFile,
    drop_in: &OwnedFile,
) -> io::Result<Vec<u8>> {
    let manifest = Manifest {
        version: 1,
        launcher: Receipt::record(state, launcher)?,
        drop_in: Receipt::record(drop_ins, drop_in)?,
    };
    serde_json::to_vec(&manifest).map_err(io::Error::other)
}

impl ActivationTransaction<'_> {
    pub(super) fn apply_journaled(&mut self, rendered: RenderedActivation) -> io::Result<()> {
        let nonce = uuid::Uuid::new_v4();
        let launcher_name = format!(".lyn-install-{nonce}-launcher");
        let drop_in_name = format!(".lyn-install-{nonce}-drop-in");
        // Preparation has no active-path effect. Persist identities and exact
        // digests before either final name is published.
        self.publish(
            Place::State,
            &launcher_name,
            rendered.launcher.as_bytes(),
            true,
        )?;
        self.publish(
            Place::DropIns,
            &drop_in_name,
            rendered.drop_in.as_bytes(),
            false,
        )?;
        let intent = InstallIntent {
            version: 1,
            nonce,
            launcher: Receipt::record(self.state, &self.files[0].1)?,
            drop_in: Receipt::record(self.drop_ins, &self.files[1].1)?,
        };
        let bytes = serde_json::to_vec(&intent).map_err(io::Error::other)?;
        self.publish(Place::State, PENDING_INSTALL, &bytes, false)?;
        #[cfg(test)]
        self.checkpoint(1)?;
        self.lock.check()?;
        self.state.require_absent(LAUNCHER)?;
        self.drop_ins.require_absent(DROP_IN)?;
        self.state
            .move_to_unoccupied(&mut self.files[0].1, LAUNCHER)?
            .durability?;
        #[cfg(test)]
        self.checkpoint(2)?;
        self.lock.check()?;
        self.drop_ins
            .move_to_unoccupied(&mut self.files[1].1, DROP_IN)?
            .durability?;
        #[cfg(test)]
        self.checkpoint(3)?;
        let bytes = manifest_bytes(
            self.state,
            self.drop_ins,
            &self.files[0].1,
            &self.files[1].1,
        )?;
        self.publish(Place::State, MANIFEST, &bytes, false)?;
        #[cfg(test)]
        self.checkpoint(4)?;
        self.state.sync_directory()?;
        self.drop_ins.sync_directory()?;
        self.lock.check()?;
        let result = self.state.remove(&self.files[2].1);
        if result.is_ok() || self.files[2].1.file.metadata()?.nlink() == 0 {
            self.files.remove(2);
        }
        result
    }

    #[cfg(test)]
    fn checkpoint(&self, checkpoint: u8) -> io::Result<()> {
        if let Some((point, abrupt)) = self.stop_after {
            if point == checkpoint {
                if abrupt {
                    std::process::exit(77);
                }
                return Err(io::Error::other("private first-install checkpoint"));
            }
        }
        Ok(())
    }
}

fn locate(
    directory: &ManagedDirectory,
    receipt: &Receipt,
    staging: &str,
    destination: &str,
) -> io::Result<OwnedFile> {
    match receipt.reopen_after_rename(directory, staging) {
        Ok(staged) => {
            directory.require_absent(destination)?;
            Ok(staged)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            receipt.reopen_after_rename(directory, destination)
        }
        Err(error) => Err(error),
    }
}

pub(in crate::integrations::gnome) struct InstallSnapshot {
    pending: OwnedFile,
    pub(in crate::integrations::gnome) launcher: OwnedFile,
    pub(in crate::integrations::gnome) drop_in: OwnedFile,
    manifest: Option<OwnedFile>,
}

impl InstallSnapshot {
    pub(in crate::integrations::gnome) fn open(
        state: &ManagedDirectory,
        drop_ins: &ManagedDirectory,
    ) -> io::Result<Self> {
        for name in [
            super::removal::PENDING_REMOVAL,
            super::update::CLEANUP_PENDING,
            super::update::PRECOMMIT_PENDING,
            super::update::COMPENSATION_PENDING,
        ]
        .into_iter()
        .chain(super::update::EXCHANGE_NAMES)
        {
            state.require_absent(name)?;
        }
        let pending = state.read_state(PENDING_INSTALL)?;
        let intent: InstallIntent =
            serde_json::from_slice(&pending.content).map_err(|_| refused())?;
        intent.validate()?;
        // Preflight both slots before moving either. Unknown names, modified bytes
        // and a missing journaled inode refuse, without adopting replacement files.
        let launcher = locate(state, &intent.launcher, &intent.launcher_name(), LAUNCHER)?;
        let drop_in = locate(drop_ins, &intent.drop_in, &intent.drop_in_name(), DROP_IN)?;
        let existing_manifest = match state.read_state(MANIFEST) {
            Ok(existing) => {
                if launcher.name.to_bytes() != LAUNCHER.as_bytes()
                    || drop_in.name.to_bytes() != DROP_IN.as_bytes()
                    || existing.content != manifest_bytes(state, drop_ins, &launcher, &drop_in)?
                {
                    return Err(refused());
                }
                Some(existing)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok(Self {
            pending,
            launcher,
            drop_in,
            manifest: existing_manifest,
        })
    }

    pub(in crate::integrations::gnome) fn recheck(
        &self,
        state: &ManagedDirectory,
        target: &ManagedDirectory,
    ) -> io::Result<()> {
        state.verify(&self.pending)?;
        let current = Self::open(state, target)?;
        state.verify(&self.launcher)?;
        target.verify(&self.drop_in)?;
        match (&self.manifest, &current.manifest) {
            (Some(file), Some(_)) => state.verify(file)?,
            (None, None) => {}
            _ => return Err(refused()),
        }
        Ok(())
    }
}

pub(super) fn recover_pending(
    state: &ManagedDirectory,
    drop_ins: &ManagedDirectory,
    lock: &InstallationLock<'_>,
) -> io::Result<()> {
    lock.check()?;
    if !super::pending(state, PENDING_INSTALL)? {
        return Ok(());
    }
    let InstallSnapshot {
        pending,
        mut launcher,
        mut drop_in,
        manifest: existing_manifest,
    } = InstallSnapshot::open(state, drop_ins)?;
    lock.check()?;
    if launcher.name.to_bytes() != LAUNCHER.as_bytes() {
        state
            .move_to_unoccupied(&mut launcher, LAUNCHER)?
            .durability?;
    }
    lock.check()?;
    if drop_in.name.to_bytes() != DROP_IN.as_bytes() {
        drop_ins
            .move_to_unoccupied(&mut drop_in, DROP_IN)?
            .durability?;
    }
    if existing_manifest.is_none() {
        let bytes = manifest_bytes(state, drop_ins, &launcher, &drop_in)?;
        match state.publish(MANIFEST, &bytes, false) {
            Ok(_) => {}
            // The durable intent remains. A subsequent explicit resume can
            // verify a published manifest despite losing this process's lease.
            Err(PublishFailure::Published { error, .. } | PublishFailure::Unpublished(error)) => {
                return Err(error);
            }
            Err(PublishFailure::Staged {
                error,
                cleanup_error,
                ..
            }) => {
                return Err(io::Error::other(format!(
                    "{error}; staging cleanup failed: {cleanup_error}"
                )));
            }
        }
    }
    state.sync_directory()?;
    drop_ins.sync_directory()?;
    lock.check()?;
    state.remove(&pending)
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::{ActivationTransaction, DROP_IN, LAUNCHER, tests::fixture};
    use super::*;
    use std::{
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt},
        path::Path,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    #[test]
    fn abrupt_exit_child() {
        let Some(path) = std::env::var_os("LYN_PRIVATE_INSTALL_RECOVERY_ROOT") else {
            return;
        };
        let path = Path::new(&path);
        let metadata = fs::symlink_metadata(path).unwrap();
        assert!(
            metadata.is_dir()
                && metadata.uid() == unsafe { libc::geteuid() }
                && metadata.mode() & 0o7777 == 0o700
        );
        assert_eq!(path.parent().unwrap(), std::env::temp_dir());
        assert!(
            path.file_name()
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
        let state_path = path.join("state");
        let state =
            crate::integrations::gnome::directories::open_private(&state_path, false).unwrap();
        let target =
            crate::integrations::gnome::directories::open_private(&path.join("drop-ins"), false)
                .unwrap();
        let rendered = crate::integrations::gnome::render_activation(
            &path.join("native.so"),
            &state_path.join(LAUNCHER),
        )
        .unwrap();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.stop_after = Some((checkpoint, true));
        transaction.apply().unwrap();
        panic!("checkpoint did not exit");
    }

    struct OwnedChild(std::process::Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }

    pub(in crate::integrations::gnome::transaction) fn abrupt_exit(
        root: &Path,
        checkpoint: u8,
        filter: &str,
    ) {
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", filter, "--nocapture"])
                .env("LYN_PRIVATE_INSTALL_RECOVERY_ROOT", root)
                .env("LYN_PRIVATE_INSTALL_RECOVERY_POINT", checkpoint.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert_eq!(status.code(), Some(77));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "private recovery child exceeded eight seconds"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn four_abrupt_process_exits_recover_one_exact_installation_each() {
        for checkpoint in 1..=4 {
            let root = tempfile::Builder::new()
                .prefix("lyn-install-recovery-")
                .tempdir()
                .unwrap();
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
            let state_path = root.path().join("state");
            let state =
                crate::integrations::gnome::directories::open_private(&state_path, true).unwrap();
            let target = crate::integrations::gnome::directories::open_private(
                &root.path().join("drop-ins"),
                true,
            )
            .unwrap();
            let expected = crate::integrations::gnome::render_activation(
                &root.path().join("native.so"),
                &state_path.join(LAUNCHER),
            )
            .unwrap();
            abrupt_exit(
                root.path(),
                checkpoint,
                "integrations::gnome::transaction::recovery::tests::abrupt_exit_child",
            );
            let mut resumed = ActivationTransaction::resume(&state, &target).unwrap();
            assert_eq!(
                fs::read_to_string(state_path.join(LAUNCHER)).unwrap(),
                expected.launcher
            );
            assert_eq!(
                fs::read_to_string(root.path().join("drop-ins").join(DROP_IN)).unwrap(),
                expected.drop_in
            );
            assert!(!state_path.join(PENDING_INSTALL).exists());
            resumed.rollback().unwrap();
            assert_eq!(fs::read_dir(&state_path).unwrap().count(), 1); // stable lock only
            assert_eq!(
                fs::read_dir(root.path().join("drop-ins")).unwrap().count(),
                0
            );
        }
    }

    #[test]
    fn recovery_preserves_a_foreign_target_without_even_moving_the_launcher() {
        let (root, state, target, rendered) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.stop_after = Some((1, false));
        assert!(transaction.apply().is_err());
        drop(transaction);
        fs::write(root.path().join("drop-ins").join(DROP_IN), b"user override").unwrap();
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        assert!(!root.path().join("state").join(LAUNCHER).exists());
        assert_eq!(
            fs::read(root.path().join("drop-ins").join(DROP_IN)).unwrap(),
            b"user override"
        );
        assert!(root.path().join("state").join(PENDING_INSTALL).is_file());
    }

    #[test]
    fn modified_staging_bytes_refuse_recovery_without_publishing_either_target() {
        let (root, state, target, rendered) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.stop_after = Some((1, false));
        assert!(transaction.apply().is_err());
        drop(transaction);
        let pending = state.read_state(PENDING_INSTALL).unwrap();
        let intent: InstallIntent = serde_json::from_slice(&pending.content).unwrap();
        let staging = root.path().join("state").join(intent.launcher_name());
        fs::write(&staging, b"modified").unwrap();
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        assert_eq!(fs::read(staging).unwrap(), b"modified");
        assert!(!root.path().join("state").join(LAUNCHER).exists());
        assert!(!root.path().join("drop-ins").join(DROP_IN).exists());
    }
}
