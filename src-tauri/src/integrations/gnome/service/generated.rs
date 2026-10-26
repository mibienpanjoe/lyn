//! Only an acknowledged manager Reload may replace these generated roots.
use super::{Result, ServiceRefusal};
use crate::integrations::gnome::{directories, managed::ManagedDirectory};
use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

pub(super) struct GeneratedRoots {
    runtime_path: PathBuf,
    runtime: ManagedDirectory,
    systemd: ManagedDirectory,
}

impl GeneratedRoots {
    pub(super) fn existing() -> Option<Self> {
        Self::open(PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?)).ok()
    }

    pub(super) fn open(runtime_path: PathBuf) -> Result<Self> {
        let runtime = directories::open_private(&runtime_path, false)
            .map_err(|_| ServiceRefusal::Unavailable)?;
        let systemd = directories::open_drop_ins(&runtime_path.join("systemd"), false)
            .map_err(|_| ServiceRefusal::Unavailable)?;
        let result = Self {
            runtime_path,
            runtime,
            systemd,
        };
        result.recheck()?;
        Ok(result)
    }

    pub(super) fn recheck(&self) -> Result<()> {
        for (path, original, private) in [
            (self.runtime_path.clone(), &self.runtime, true),
            (self.runtime_path.join("systemd"), &self.systemd, false),
        ] {
            original.check().map_err(|_| ServiceRefusal::Changed)?;
            let current = if private {
                directories::open_private(&path, false)
            } else {
                directories::open_drop_ins(&path, false)
            }
            .map_err(|_| ServiceRefusal::Changed)?;
            let a = original
                .directory
                .metadata()
                .map_err(|_| ServiceRefusal::Changed)?;
            let b = current
                .directory
                .metadata()
                .map_err(|_| ServiceRefusal::Changed)?;
            if (a.dev(), a.ino(), a.uid(), a.mode()) != (b.dev(), b.ino(), b.uid(), b.mode()) {
                return Err(ServiceRefusal::Changed);
            }
        }
        Ok(())
    }

    pub(super) fn contains(&self, path: &Path) -> bool {
        ["generator", "generator.early", "generator.late"]
            .iter()
            .any(|name| path == self.runtime_path.join("systemd").join(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };

    #[test]
    fn generated_roots_pin_both_parents_and_exact_names() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(root.path().join("systemd")).unwrap();
        fs::set_permissions(
            root.path().join("systemd"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let pin = GeneratedRoots::open(root.path().to_owned()).unwrap();
        assert!(pin.contains(&root.path().join("systemd/generator.late")));
        assert!(!pin.contains(&root.path().join("systemd/generator.late/child")));
        assert!(!pin.contains(&root.path().join("systemd/generator.later")));
        fs::rename(root.path().join("systemd"), root.path().join("old")).unwrap();
        fs::create_dir(root.path().join("systemd")).unwrap();
        assert_eq!(pin.recheck(), Err(ServiceRefusal::Changed));
        fs::remove_dir(root.path().join("systemd")).unwrap();
        symlink(root.path().join("old"), root.path().join("systemd")).unwrap();
        assert_eq!(pin.recheck(), Err(ServiceRefusal::Changed));
    }

    #[test]
    fn replacement_runtime_does_not_inherit_the_reload_exception() {
        let parent = tempfile::tempdir().unwrap();
        fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = parent.path().join("runtime");
        fs::create_dir_all(runtime.join("systemd")).unwrap();
        for path in [&runtime, &runtime.join("systemd")] {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let pin = GeneratedRoots::open(runtime.clone()).unwrap();
        fs::rename(&runtime, parent.path().join("old")).unwrap();
        fs::create_dir_all(runtime.join("systemd")).unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(runtime.join("systemd"), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(pin.recheck(), Err(ServiceRefusal::Changed));
    }
}
