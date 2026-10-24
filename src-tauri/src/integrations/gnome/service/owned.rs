//! Read-only authority for one factory-rendered, receipt-owned activation.

use super::super::{
    PREVIOUS_MODULE_PREFIX,
    artifact::format,
    directories,
    managed::ManagedDirectory,
    render_activation_with_previous,
    transaction::{
        CommittedUpdateSnapshot, DROP_IN, InstallSnapshot, LAUNCHER, PinnedActivation,
        RemovalSnapshot, RetirementSnapshot, UncommittedUpdateSnapshot,
    },
};
use std::{
    fs, io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

pub(in crate::integrations::gnome) struct OwnedActivation {
    state_path: PathBuf,
    target_path: PathBuf,
    state: ManagedDirectory,
    target: ManagedDirectory,
    files: ActivationEvidence,
    launcher_path: String,
    drop_in_path: String,
    module_path: PathBuf,
    previous_module: Option<String>,
}

enum ActivationEvidence {
    Complete(PinnedActivation),
    Installing(InstallSnapshot),
    Retired(RetirementSnapshot),
    Removing(RemovalSnapshot),
    CommittedUpdate(CommittedUpdateSnapshot),
    UncommittedUpdate(UncommittedUpdateSnapshot),
}

impl ActivationEvidence {
    fn contents(&self) -> (&[u8], &[u8]) {
        match self {
            Self::Complete(files) => (&files.launcher.content, &files.drop_in.content),
            Self::Installing(files) => (&files.launcher.content, &files.drop_in.content),
            Self::Retired(files) => files.contents(),
            Self::Removing(files) => files.contents(),
            Self::CommittedUpdate(files) => (&files.launcher.content, &files.drop_in.content),
            Self::UncommittedUpdate(files) => files.contents(),
        }
    }

    fn recheck(&self, state: &ManagedDirectory, target: &ManagedDirectory) -> io::Result<()> {
        match self {
            Self::Complete(files) => files.recheck(state, target),
            Self::Installing(files) => files.recheck(state, target),
            Self::Retired(files) => files.recheck(state, target),
            Self::Removing(files) => files.recheck(state, target),
            Self::CommittedUpdate(files) => files.recheck(state, target),
            Self::UncommittedUpdate(files) => files.recheck(state, target),
        }
    }
}

fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "unrecognized managed GNOME activation",
    )
}

fn decode_factory(
    state_path: &Path,
    launcher_path: &str,
    launcher_bytes: &[u8],
    drop_in_bytes: &[u8],
) -> io::Result<(PathBuf, Option<String>)> {
    // Decode only the factory's single-quoted literal, then require the
    // entire launcher and drop-in to round-trip byte for byte. This is not
    // a generic shell parser and never executes the supplied text.
    let text = std::str::from_utf8(launcher_bytes).map_err(|_| refused())?;
    let literal = text
        .lines()
        .nth(2)
        .and_then(|line| line.strip_prefix("lyn_module='"))
        .and_then(|line| line.strip_suffix('\''))
        .ok_or_else(refused)?;
    let module_path = PathBuf::from(literal.replace("'\\''", "'"));
    let version = module_path.parent().ok_or_else(refused)?;
    let digest = version
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(refused)?;
    if module_path.file_name() != Some(std::ffi::OsStr::new(format::MODULE_NAME))
        || version.parent() != Some(state_path.join("modules").as_path())
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(refused());
    }
    let previous_module = text
        .lines()
        .last()
        .and_then(|line| line.strip_prefix(PREVIOUS_MODULE_PREFIX));
    let rendered =
        render_activation_with_previous(&module_path, Path::new(launcher_path), previous_module)
            .map_err(|_| refused())?;
    if rendered.launcher.as_bytes() != launcher_bytes
        || rendered.drop_in.as_bytes() != drop_in_bytes
    {
        return Err(refused());
    }
    let previous_module = previous_module.map(str::to_owned);
    Ok((module_path, previous_module))
}

impl OwnedActivation {
    /// Internal paths only; this constructor is never an IPC path parameter.
    pub(in crate::integrations::gnome) fn open(
        state_path: &Path,
        target_path: &Path,
    ) -> io::Result<Self> {
        let state = directories::open_private(state_path, false)?;
        let target = directories::open_drop_ins(target_path, false)?;
        let files = ActivationEvidence::Complete(PinnedActivation::open(&state, &target)?);
        Self::from_files(state_path, target_path, state, target, files)
    }

    pub(in crate::integrations::gnome) fn open_pending_install(
        state_path: &Path,
        target_path: &Path,
    ) -> io::Result<Self> {
        let state = directories::open_private(state_path, false)?;
        let target = directories::open_drop_ins(target_path, false)?;
        let files = ActivationEvidence::Installing(InstallSnapshot::open(&state, &target)?);
        Self::from_files(state_path, target_path, state, target, files)
    }

    pub(in crate::integrations::gnome) fn open_committed_update(
        state_path: &Path,
        target_path: &Path,
    ) -> io::Result<Self> {
        let state = directories::open_private(state_path, false)?;
        let target = directories::open_drop_ins(target_path, false)?;
        let files =
            ActivationEvidence::CommittedUpdate(CommittedUpdateSnapshot::open(&state, &target)?);
        Self::from_files(state_path, target_path, state, target, files)
    }

    pub(in crate::integrations::gnome) fn open_retired(
        state_path: &Path,
        target_path: &Path,
    ) -> io::Result<Self> {
        let state = directories::open_private(state_path, false)?;
        let target = directories::open_drop_ins(target_path, false)?;
        let files =
            ActivationEvidence::Retired(RetirementSnapshot::open(&state, &target, state_path)?);
        Self::from_files(state_path, target_path, state, target, files)
    }

    pub(in crate::integrations::gnome) fn is_retired(&self) -> bool {
        matches!(&self.files, ActivationEvidence::Retired(_))
    }

    pub(in crate::integrations::gnome) fn open_uncommitted_update(
        state_path: &Path,
        target_path: &Path,
    ) -> io::Result<Self> {
        let state = directories::open_private(state_path, false)?;
        let target = directories::open_drop_ins(target_path, false)?;
        let files = ActivationEvidence::UncommittedUpdate(UncommittedUpdateSnapshot::open(
            &state, &target,
        )?);
        Self::from_files(state_path, target_path, state, target, files)
    }

    pub(in crate::integrations::gnome) fn open_pending_removal(
        state_path: &Path,
        target_path: &Path,
    ) -> io::Result<Self> {
        let state = directories::open_private(state_path, false)?;
        let target = directories::open_drop_ins(target_path, false)?;
        let files =
            ActivationEvidence::Removing(RemovalSnapshot::open(&state, &target, state_path)?);
        Self::from_files(state_path, target_path, state, target, files)
    }

    fn from_files(
        state_path: &Path,
        target_path: &Path,
        state: ManagedDirectory,
        target: ManagedDirectory,
        files: ActivationEvidence,
    ) -> io::Result<Self> {
        let launcher_path = state_path
            .join(LAUNCHER)
            .to_str()
            .ok_or_else(refused)?
            .to_owned();
        let drop_in_path = target_path
            .join(DROP_IN)
            .to_str()
            .ok_or_else(refused)?
            .to_owned();
        let (launcher_bytes, drop_in_bytes) = files.contents();
        let (module_path, previous_module) =
            decode_factory(state_path, &launcher_path, launcher_bytes, drop_in_bytes)?;
        if let ActivationEvidence::UncommittedUpdate(snapshot) = &files {
            if let Some((launcher, drop_in)) = snapshot.replacement_contents() {
                decode_factory(state_path, &launcher_path, launcher, drop_in)?;
            }
        }
        let result = Self {
            state_path: state_path.into(),
            target_path: target_path.into(),
            state,
            target,
            files,
            launcher_path,
            drop_in_path,
            module_path,
            previous_module,
        };
        result.recheck()?;
        Ok(result)
    }

    pub(super) fn paths(&self) -> (&str, &str) {
        (&self.launcher_path, &self.drop_in_path)
    }

    pub(in crate::integrations::gnome) fn module_path(&self) -> &Path {
        &self.module_path
    }

    pub(in crate::integrations::gnome) fn previous_module(&self) -> Option<&str> {
        self.previous_module.as_deref()
    }

    pub(in crate::integrations::gnome) fn module_digest(&self) -> &str {
        self.module_path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .expect("owned module reference was validated")
    }

    pub(in crate::integrations::gnome) fn revision(&self) -> io::Result<String> {
        self.recheck()?;
        let ActivationEvidence::Complete(files) = &self.files else {
            return Err(refused());
        };
        let revision = files.revision()?;
        self.recheck()?;
        Ok(revision)
    }

    pub(super) fn drop_in_metadata(&self) -> io::Result<Option<fs::Metadata>> {
        match &self.files {
            ActivationEvidence::Complete(files) => files.drop_in.file.metadata().map(Some),
            ActivationEvidence::CommittedUpdate(files) => files.drop_in.file.metadata().map(Some),
            ActivationEvidence::UncommittedUpdate(files) => files.drop_in_metadata(),
            ActivationEvidence::Installing(files)
                if files.drop_in.name.to_bytes() == DROP_IN.as_bytes() =>
            {
                files.drop_in.file.metadata().map(Some)
            }
            ActivationEvidence::Installing(_) => Ok(None),
            ActivationEvidence::Retired(_) => Ok(None),
            ActivationEvidence::Removing(files) => files.drop_in_metadata(),
        }
    }

    pub(in crate::integrations::gnome) fn recheck(&self) -> io::Result<()> {
        self.recheck_namespace()?;
        self.files.recheck(&self.state, &self.target)
    }

    pub(in crate::integrations::gnome) fn acknowledge_retirement(&self) -> io::Result<()> {
        self.recheck_namespace()?;
        match &self.files {
            ActivationEvidence::Retired(files) => files.acknowledge(&self.state, &self.target),
            _ => Err(refused()),
        }
    }

    fn recheck_namespace(&self) -> io::Result<()> {
        let state = directories::open_private(&self.state_path, false)?;
        let target = directories::open_drop_ins(&self.target_path, false)?;
        for (current, pinned) in [(&state, &self.state), (&target, &self.target)] {
            let current = current.directory.metadata()?;
            let pinned = pinned.directory.metadata()?;
            if (current.dev(), current.ino()) != (pinned.dev(), pinned.ino()) {
                return Err(refused());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::transaction::ActivationTransaction;
    use super::*;
    use crate::integrations::gnome::render_activation;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn receipt_owned_factory_pair_is_pinned_without_recovery_or_lock_creation() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let state_path = root.path().join("state with quote'");
        let target_path = root.path().join("target");
        let state = directories::open_private(&state_path, true).unwrap();
        let target = directories::open_drop_ins(&target_path, true).unwrap();
        let module = state_path
            .join("modules")
            .join("a".repeat(64))
            .join(format::MODULE_NAME);
        let rendered = render_activation(&module, &state_path.join(LAUNCHER)).unwrap();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        drop(transaction);
        let _writer_lock =
            crate::integrations::gnome::lock::InstallationLock::acquire(&state).unwrap();
        let before = fs::read_dir(&state_path).unwrap().count();
        let authority = OwnedActivation::open(&state_path, &target_path).unwrap();
        authority.recheck().unwrap();
        assert_eq!(authority.module_path(), module);
        assert_eq!(fs::read_dir(&state_path).unwrap().count(), before);
        fs::set_permissions(state_path.join(LAUNCHER), fs::Permissions::from_mode(0o600)).unwrap();
        assert!(authority.recheck().is_err());
        assert!(OwnedActivation::open(&state_path, &target_path).is_err());
    }

    fn factory_pair(root: &Path, kind: &str) -> (PathBuf, PathBuf) {
        let state_path = root.join("state");
        let target_path = root.join("target");
        let state = directories::open_private(&state_path, true).unwrap();
        let target = directories::open_drop_ins(&target_path, true).unwrap();
        let module = match kind {
            "outside-module" => root.join("other.so"),
            "invalid-digest" => state_path
                .join("modules")
                .join("G".repeat(64))
                .join(format::MODULE_NAME),
            "wrong-module-name" => state_path
                .join("modules")
                .join("a".repeat(64))
                .join("other.so"),
            _ => state_path
                .join("modules")
                .join("a".repeat(64))
                .join(format::MODULE_NAME),
        };
        let mut rendered = render_activation(&module, &state_path.join(LAUNCHER)).unwrap();
        if kind == "extra-shell" {
            rendered.launcher.push_str("exit 0\n");
        }
        if kind == "extra-drop-in" {
            rendered.drop_in.push_str("# unexpected\n");
        }
        if kind == "bad-history" {
            rendered
                .launcher
                .push_str(&format!("{PREVIOUS_MODULE_PREFIX}../escape\n"));
        }
        if kind == "self-history" {
            rendered
                .launcher
                .push_str(&format!("{PREVIOUS_MODULE_PREFIX}{}\n", "a".repeat(64)));
        }
        if kind == "duplicate-history" {
            for _ in 0..2 {
                rendered
                    .launcher
                    .push_str(&format!("{PREVIOUS_MODULE_PREFIX}{}\n", "b".repeat(64)));
            }
        }
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        (state_path, target_path)
    }

    #[test]
    fn valid_receipts_do_not_admit_nonfactory_code_or_external_module_references() {
        for kind in [
            "outside-module",
            "invalid-digest",
            "wrong-module-name",
            "extra-shell",
            "extra-drop-in",
            "bad-history",
            "self-history",
            "duplicate-history",
        ] {
            let root = tempfile::tempdir().unwrap();
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
            let (state, target) = factory_pair(root.path(), kind);
            let before = fs::read(state.join(LAUNCHER)).unwrap();
            assert!(OwnedActivation::open(&state, &target).is_err(), "{kind}");
            assert_eq!(fs::read(state.join(LAUNCHER)).unwrap(), before);
        }
    }

    #[test]
    fn path_replacement_identical_rewrite_and_pending_authority_are_refused() {
        for kind in [
            "state-directory",
            "target-directory",
            "identical-rewrite",
            "drop-in-replacement",
            "pending",
        ] {
            let root = tempfile::tempdir().unwrap();
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
            let (state, target) = factory_pair(root.path(), "valid");
            let authority = OwnedActivation::open(&state, &target).unwrap();
            match kind {
                "state-directory" | "target-directory" => {
                    let path = if kind == "state-directory" {
                        &state
                    } else {
                        &target
                    };
                    fs::rename(path, root.path().join("original")).unwrap();
                    fs::create_dir(path).unwrap();
                    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
                }
                "identical-rewrite" => {
                    let path = state.join(LAUNCHER);
                    fs::write(&path, fs::read(&path).unwrap()).unwrap();
                }
                "drop-in-replacement" => {
                    let path = target.join(DROP_IN);
                    let bytes = fs::read(&path).unwrap();
                    fs::rename(&path, root.path().join("original")).unwrap();
                    fs::write(&path, bytes).unwrap();
                    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
                }
                _ => fs::write(
                    state.join("installation-pending-v1.json"),
                    b"pending authority",
                )
                .unwrap(),
            }
            let before = fs::read_dir(root.path()).unwrap().count();
            assert!(authority.recheck().is_err(), "{kind}");
            assert!(OwnedActivation::open(&state, &target).is_err(), "{kind}");
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), before);
            if kind == "pending" {
                assert_eq!(
                    fs::read(state.join("installation-pending-v1.json")).unwrap(),
                    b"pending authority"
                );
            }
        }
    }

    #[test]
    fn absent_activation_does_not_create_state_or_lock() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            OwnedActivation::open(&root.path().join("state"), &root.path().join("target")).is_err()
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
