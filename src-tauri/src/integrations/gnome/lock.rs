//! Stable per-directory installation lock; never unlink the lock inode.

use super::managed::ManagedDirectory;
use std::{
    fs::File,
    io,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
};

const LOCK_NAME: &std::ffi::CStr = c".installation.lock";

pub(super) struct InstallationLock<'a> {
    directory: &'a ManagedDirectory,
    file: File,
}

fn check_file(file: &File) -> io::Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
        || metadata.len() != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unsafe installation lock",
        ));
    }
    Ok(())
}

impl<'a> InstallationLock<'a> {
    pub(super) fn acquire(directory: &'a ManagedDirectory) -> io::Result<Self> {
        directory.check_private()?;
        let file = directory.open(&LOCK_NAME.to_owned(), libc::O_CREAT | libc::O_RDWR)?;
        check_file(&file)?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let lock = Self { directory, file };
        lock.check()?;
        Ok(lock)
    }

    pub(super) fn check(&self) -> io::Result<()> {
        self.directory.check_private()?;
        check_file(&self.file)?;
        self.directory.same_entry(&LOCK_NAME.to_owned(), &self.file)
    }
}

impl Drop for InstallationLock<'_> {
    fn drop(&mut self) {
        // flock belongs to the open file description. Explicit unlock avoids a
        // concurrently spawned, pre-exec child extending this process's lease.
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    };

    #[test]
    fn concurrent_installation_refuses_and_release_preserves_the_lock_inode() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let first = super::super::directories::open_private(root.path(), false).unwrap();
        let second = super::super::directories::open_private(root.path(), false).unwrap();
        let lock = InstallationLock::acquire(&first).unwrap();
        let inode = fs::metadata(root.path().join(".installation.lock"))
            .unwrap()
            .ino();
        assert!(InstallationLock::acquire(&second).is_err());
        lock.check().unwrap();
        drop(lock);
        let replacement = InstallationLock::acquire(&second).unwrap();
        assert_eq!(
            fs::metadata(root.path().join(".installation.lock"))
                .unwrap()
                .ino(),
            inode
        );
        replacement.check().unwrap();
    }

    #[test]
    fn replaced_or_foreign_lock_is_never_adopted_or_removed() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let directory = super::super::directories::open_private(root.path(), false).unwrap();
        let lock = InstallationLock::acquire(&directory).unwrap();
        fs::rename(
            root.path().join(".installation.lock"),
            root.path().join("original"),
        )
        .unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        symlink(outside.path(), root.path().join(".installation.lock")).unwrap();
        assert!(lock.check().is_err());
        assert!(InstallationLock::acquire(&directory).is_err());
        drop(lock);
        assert!(
            fs::symlink_metadata(root.path().join(".installation.lock"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(outside.path()).unwrap(), b"");
    }
}
