//! Nonmutating inspection, deliberately separate from resume/recovery.

use super::{DROP_IN, LAUNCHER, MANIFEST, Manifest, pending, recovery, removal, update};
use crate::integrations::gnome::managed::{ManagedDirectory, OwnedFile};
use std::{io, os::fd::AsRawFd};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::integrations::gnome) enum ActivationFilesState {
    Absent,
    Owned,
    Changed,
    Pending,
    Unavailable,
}

fn exists(directory: Option<&ManagedDirectory>, name: &str) -> io::Result<bool> {
    directory.map_or(Ok(false), |directory| pending(directory, name))
}

fn has_intent(state: Option<&ManagedDirectory>) -> io::Result<bool> {
    for name in [
        recovery::PENDING_INSTALL,
        removal::PENDING_REMOVAL,
        update::CLEANUP_PENDING,
        update::PRECOMMIT_PENDING,
        update::COMPENSATION_PENDING,
    ]
    .into_iter()
    .chain(update::EXCHANGE_NAMES)
    {
        if exists(state, name)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn require_regular(directory: &ManagedDirectory, name: &str) -> io::Result<()> {
    directory.check()?;
    let name = crate::integrations::gnome::managed::basename(name)?;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            directory.directory.as_raw_fd(),
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if unsafe { metadata.assume_init() }.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(super::refused());
    }
    Ok(())
}

pub(in crate::integrations::gnome) struct PinnedActivation {
    pub(in crate::integrations::gnome) launcher: OwnedFile,
    pub(in crate::integrations::gnome) drop_in: OwnedFile,
    manifest: OwnedFile,
    body: Manifest,
}

impl PinnedActivation {
    pub(in crate::integrations::gnome) fn open(
        state: &ManagedDirectory,
        target: &ManagedDirectory,
    ) -> io::Result<Self> {
        if has_intent(Some(state))? {
            return Err(super::refused());
        }
        for (directory, name) in [(state, LAUNCHER), (state, MANIFEST), (target, DROP_IN)] {
            require_regular(directory, name)?;
        }
        let manifest = state.read_state(MANIFEST)?;
        let body: Manifest =
            serde_json::from_slice(&manifest.content).map_err(|_| super::refused())?;
        if body.version != 1
            || !body.launcher.is_for(LAUNCHER, 0o700)
            || !body.drop_in.is_for(DROP_IN, 0o600)
        {
            return Err(super::refused());
        }
        let result = Self {
            launcher: body.launcher.reopen(state)?,
            drop_in: body.drop_in.reopen(target)?,
            manifest,
            body,
        };
        result.recheck(state, target)?;
        Ok(result)
    }

    pub(in crate::integrations::gnome) fn recheck(
        &self,
        state: &ManagedDirectory,
        target: &ManagedDirectory,
    ) -> io::Result<()> {
        if has_intent(Some(state))? {
            return Err(super::refused());
        }
        state.verify(&self.manifest)?;
        state.verify(&self.launcher)?;
        target.verify(&self.drop_in)?;
        self.body.launcher.reopen(state)?;
        self.body.drop_in.reopen(target)?;
        if has_intent(Some(state))? {
            return Err(super::refused());
        }
        Ok(())
    }
}

fn inspect(
    state: Option<&ManagedDirectory>,
    target: Option<&ManagedDirectory>,
) -> io::Result<ActivationFilesState> {
    if has_intent(state)? {
        return Ok(ActivationFilesState::Pending);
    }
    let launcher = exists(state, LAUNCHER)?;
    let manifest = exists(state, MANIFEST)?;
    let drop_in = exists(target, DROP_IN)?;
    if !launcher && !manifest && !drop_in {
        return Ok(ActivationFilesState::Absent);
    }
    let (Some(state), Some(target)) = (state, target) else {
        return Ok(ActivationFilesState::Changed);
    };
    if !launcher || !manifest || !drop_in {
        return Ok(ActivationFilesState::Changed);
    }
    let pinned = PinnedActivation::open(state, target);
    if has_intent(Some(state))? {
        return Ok(ActivationFilesState::Pending);
    }
    pinned?;
    Ok(ActivationFilesState::Owned)
}

/// Leases were opened without creation by the caller. Owned covers exactly the
/// three activation files; it proves neither module availability nor a running
/// service/connection. Pending is never resumed as a side effect of status.
pub(in crate::integrations::gnome) fn inspect_activation_files(
    state: Option<&ManagedDirectory>,
    target: Option<&ManagedDirectory>,
) -> ActivationFilesState {
    match inspect(state, target) {
        Ok(state) => state,
        Err(error)
            if matches!(
                error.raw_os_error(),
                Some(libc::ELOOP | libc::ENXIO | libc::ENODEV | libc::EISDIR)
            ) || matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
            ) =>
        {
            ActivationFilesState::Changed
        }
        Err(_) => ActivationFilesState::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ActivationTransaction, tests::fixture};
    use super::*;
    use std::fs;

    #[test]
    fn absent_and_owned_activation_files_are_distinct_without_writes() {
        assert_eq!(
            inspect_activation_files(None, None),
            ActivationFilesState::Absent
        );
        let (root, state, target, rendered) = fixture();
        assert_eq!(
            inspect_activation_files(Some(&state), Some(&target)),
            ActivationFilesState::Absent
        );
        assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 0);
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        let before = fs::read(root.path().join("state").join(MANIFEST)).unwrap();
        assert_eq!(
            inspect_activation_files(Some(&state), Some(&target)),
            ActivationFilesState::Owned
        );
        assert_eq!(
            fs::read(root.path().join("state").join(MANIFEST)).unwrap(),
            before
        );
        assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 3);
        assert_eq!(
            fs::read_dir(root.path().join("drop-ins")).unwrap().count(),
            1
        );
    }

    #[test]
    fn pending_update_is_observed_without_committing_or_restoring_it() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let rendered = crate::integrations::gnome::render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        let new_launcher = rendered.launcher.clone();
        transaction.update(rendered).unwrap();
        let count = fs::read_dir(root.path().join("state")).unwrap().count();
        assert_eq!(
            inspect_activation_files(Some(&state), Some(&target)),
            ActivationFilesState::Pending
        );
        assert_eq!(
            fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
            new_launcher
        );
        assert_eq!(
            fs::read_dir(root.path().join("state")).unwrap().count(),
            count
        );
        assert!(
            root.path()
                .join("state")
                .join(update::PRECOMMIT_PENDING)
                .exists()
        );
    }

    #[test]
    fn pending_uninstall_is_not_advanced_by_inspection() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        transaction.stop_after = Some((12, false));
        assert!(transaction.rollback().is_err());
        assert_eq!(
            inspect_activation_files(Some(&state), Some(&target)),
            ActivationFilesState::Pending
        );
        assert!(root.path().join("state").join(LAUNCHER).exists());
        assert!(root.path().join("state").join(MANIFEST).exists());
        assert!(root.path().join("drop-ins").join(DROP_IN).exists());
        assert!(
            root.path()
                .join("state")
                .join(removal::PENDING_REMOVAL)
                .exists()
        );
    }

    #[test]
    fn partial_or_edited_files_are_not_owned_and_remain_unchanged() {
        let (root, state, target, initial) = fixture();
        let file = root.path().join("drop-ins").join(DROP_IN);
        fs::write(&file, b"foreign").unwrap();
        assert_eq!(
            inspect_activation_files(None, Some(&target)),
            ActivationFilesState::Changed
        );
        assert_eq!(fs::read(&file).unwrap(), b"foreign");
        fs::remove_file(&file).unwrap();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        fs::write(&file, b"edited").unwrap();
        assert_eq!(
            inspect_activation_files(Some(&state), Some(&target)),
            ActivationFilesState::Changed
        );
        assert_eq!(fs::read(&file).unwrap(), b"edited");
        assert!(root.path().join("state").join(MANIFEST).exists());
    }

    #[test]
    fn symlink_replacements_are_changed_files_and_are_never_followed() {
        for (directory, name) in [
            ("state", LAUNCHER),
            ("state", MANIFEST),
            ("drop-ins", DROP_IN),
        ] {
            let (root, state, target, initial) = fixture();
            let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
            transaction.apply().unwrap();
            let file = root.path().join(directory).join(name);
            let retained = root.path().join("retained");
            fs::rename(&file, &retained).unwrap();
            std::os::unix::fs::symlink(&retained, &file).unwrap();
            assert_eq!(
                inspect_activation_files(Some(&state), Some(&target)),
                ActivationFilesState::Changed
            );
            assert_eq!(fs::read_link(&file).unwrap(), retained);
            assert!(
                root.path()
                    .join("state")
                    .join(".installation.lock")
                    .exists()
            );
        }
    }

    #[test]
    fn sockets_pipes_and_directories_are_changed_without_opening_them() {
        use std::{
            ffi::CString,
            os::unix::{ffi::OsStrExt, net::UnixListener},
        };
        for kind in ["socket", "pipe", "directory"] {
            for (directory, name) in [
                ("state", LAUNCHER),
                ("state", MANIFEST),
                ("drop-ins", DROP_IN),
            ] {
                let (root, state, target, initial) = fixture();
                let mut transaction =
                    ActivationTransaction::begin(&state, &target, initial).unwrap();
                transaction.apply().unwrap();
                let file = root.path().join(directory).join(name);
                fs::remove_file(&file).unwrap();
                let _socket = match kind {
                    "socket" => Some(UnixListener::bind(&file).unwrap()),
                    "pipe" => {
                        let path = CString::new(file.as_os_str().as_bytes()).unwrap();
                        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
                        None
                    }
                    _ => {
                        fs::create_dir(&file).unwrap();
                        None
                    }
                };
                assert_eq!(
                    inspect_activation_files(Some(&state), Some(&target)),
                    ActivationFilesState::Changed
                );
                assert!(fs::symlink_metadata(&file).is_ok());
            }
        }
    }
}
