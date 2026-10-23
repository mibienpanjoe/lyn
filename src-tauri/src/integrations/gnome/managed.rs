//! Descriptor-relative files for private state or protected activation directories.
//! Not an installer: ancestor validation, persistent ownership and transaction
//! locking belong to the caller before these primitives can be exposed.

use std::{
    ffi::CString,
    fs::{File, Permissions},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, PermissionsExt},
    },
};

pub(super) const MAX_CONTENT: usize = 16 * 1024;

pub(super) struct ManagedDirectory {
    pub(super) directory: File,
    expected_mode: u32,
    #[cfg(test)]
    fault: std::cell::Cell<Fault>,
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    None,
    StageCleanup,
    DirectorySync,
}

#[derive(Debug)]
pub(super) enum PublishFailure {
    Unpublished(io::Error),
    Staged {
        error: io::Error,
        cleanup_error: io::Error,
        stage: StagedFile,
    },
    Published {
        error: io::Error,
        owned: OwnedFile,
    },
}

impl From<io::Error> for PublishFailure {
    fn from(error: io::Error) -> Self {
        Self::Unpublished(error)
    }
}

#[derive(Debug)]
pub(super) struct StagedFile {
    file: File,
    name: CString,
}

/// Process-local ownership only. Retaining the descriptor also prevents inode
/// reuse from making a replacement appear owned. No implicit uninstall on drop.
#[derive(Debug)]
pub(super) struct OwnedFile {
    pub(super) file: File,
    pub(super) name: CString,
    pub(super) content: Vec<u8>,
    pub(super) mode: u32,
}

fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "activation ownership changed",
    )
}

pub(super) fn basename(name: &str) -> io::Result<CString> {
    if name.is_empty() || matches!(name, "." | "..") || name.contains('/') || name.len() > 255 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid activation name",
        ));
    }
    CString::new(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid activation name"))
}

impl ManagedDirectory {
    pub(super) fn require_absent(&self, name: &str) -> io::Result<()> {
        self.check()?;
        let name = basename(name)?;
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                self.directory.as_raw_fd(),
                name.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "activation target occupied",
            ));
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::NotFound {
            Ok(())
        } else {
            Err(error)
        }
    }

    /// Read a private bounded installer state file, retaining its ownership
    /// descriptor. Schema/role validation remains the caller's responsibility.
    pub(super) fn read_state(&self, name: &str) -> io::Result<OwnedFile> {
        self.check_private()?;
        let name = basename(name)?;
        let mut file = self.open(&name, libc::O_RDONLY)?;
        let original = file.metadata()?;
        if !original.is_file()
            || original.uid() != unsafe { libc::geteuid() }
            || original.mode() & 0o7777 != 0o600
            || original.nlink() != 1
            || original.len() > MAX_CONTENT as u64
        {
            return Err(refused());
        }
        let mut content = Vec::new();
        (&mut file)
            .take((MAX_CONTENT + 1) as u64)
            .read_to_end(&mut content)?;
        let final_metadata = file.metadata()?;
        if final_metadata.len() != original.len()
            || content.len() as u64 != original.len()
            || final_metadata.ctime() != original.ctime()
            || final_metadata.ctime_nsec() != original.ctime_nsec()
        {
            return Err(refused());
        }
        self.same_entry(&name, &file)?;
        Ok(OwnedFile {
            file,
            name,
            content,
            mode: 0o600,
        })
    }

    /// Caller must have securely resolved the ancestry of this directory. A
    /// supplied descriptor cannot establish whether its original path was safe.
    pub(super) fn from_directory(directory: File) -> io::Result<Self> {
        Self::from_mode(directory, 0o700)
    }

    /// Existing user-owned systemd drop-in directories may be readable by
    /// others. Never accept group/other write or special permission bits.
    pub(super) fn from_drop_in_directory(directory: File) -> io::Result<Self> {
        let mode = directory.metadata()?.mode() & 0o7777;
        if mode & 0o700 != 0o700 || mode & 0o7022 != 0 {
            return Err(refused());
        }
        Self::from_mode(directory, mode)
    }

    fn from_mode(directory: File, expected_mode: u32) -> io::Result<Self> {
        let directory = Self {
            directory,
            expected_mode,
            #[cfg(test)]
            fault: std::cell::Cell::new(Fault::None),
        };
        directory.check()?;
        Ok(directory)
    }

    pub(super) fn check(&self) -> io::Result<()> {
        let metadata = self.directory.metadata()?;
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o7777 != self.expected_mode
        {
            return Err(refused());
        }
        Ok(())
    }

    pub(super) fn check_private(&self) -> io::Result<()> {
        self.check()?;
        if self.expected_mode != 0o700 {
            return Err(refused());
        }
        Ok(())
    }

    pub(super) fn open(&self, name: &CString, flags: i32) -> io::Result<File> {
        // Every operation is anchored at the retained directory, not a path
        // that can be redirected by renaming an ancestor. Never follow leaves.
        let fd = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    pub(super) fn same_entry(&self, name: &CString, file: &File) -> io::Result<()> {
        let mut current = std::mem::MaybeUninit::<libc::stat>::uninit();
        let result = unsafe {
            libc::fstatat(
                self.directory.as_raw_fd(),
                name.as_ptr(),
                current.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        let current = unsafe { current.assume_init() };
        let original = file.metadata()?;
        if current.st_dev != original.dev() || current.st_ino != original.ino() {
            return Err(refused());
        }
        Ok(())
    }

    fn unlink(&self, name: &CString) -> io::Result<()> {
        if unsafe { libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn sync_directory(&self) -> io::Result<()> {
        #[cfg(test)]
        if self.fault.get() == Fault::DirectorySync {
            return Err(io::Error::other("injected directory sync failure"));
        }
        self.directory.sync_all()
    }

    #[cfg(test)]
    pub(super) fn fail_directory_sync(&self, fail: bool) {
        self.fault.set(if fail {
            Fault::DirectorySync
        } else {
            Fault::None
        });
    }

    pub(super) fn discard_stage(&self, stage: &StagedFile) -> io::Result<()> {
        #[cfg(test)]
        if self.fault.get() == Fault::StageCleanup {
            return Err(io::Error::other("injected staging cleanup failure"));
        }
        self.check()?;
        let metadata = stage.file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.nlink() != 1
        {
            return Err(refused());
        }
        self.same_entry(&stage.name, &stage.file)?;
        self.unlink(&stage.name)
    }

    /// Stage complete, synced bytes before a non-replacing atomic rename. This
    /// primitive only creates new names; update/rollback needs a transaction.
    pub(super) fn publish(
        &self,
        name: &str,
        content: &[u8],
        executable: bool,
    ) -> Result<OwnedFile, PublishFailure> {
        self.check()?;
        let name = basename(name)?;
        if content.len() > MAX_CONTENT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "activation content too large",
            )
            .into());
        }
        let temporary = basename(&format!(".lyn-{}", uuid::Uuid::new_v4()))?;
        let mut file = self.open(&temporary, libc::O_CREAT | libc::O_EXCL | libc::O_RDWR)?;
        let mode = if executable { 0o700 } else { 0o600 };
        let result = (|| {
            file.write_all(content)?;
            file.set_permissions(Permissions::from_mode(mode))?;
            file.sync_all()?;
            self.check()?;
            self.same_entry(&temporary, &file)?;
            // RENAME_NOREPLACE refuses *any* occupied target, including dangling
            // symlinks, without the transient extra hardlink of link/unlink.
            if unsafe {
                libc::renameat2(
                    self.directory.as_raw_fd(),
                    temporary.as_ptr(),
                    self.directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::RENAME_NOREPLACE,
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        })();
        if let Err(error) = result {
            let stage = StagedFile {
                file,
                name: temporary,
            };
            return match self.discard_stage(&stage) {
                Ok(()) => Err(PublishFailure::Unpublished(error)),
                Err(cleanup_error) => Err(PublishFailure::Staged {
                    error,
                    cleanup_error,
                    stage,
                }),
            };
        }
        let owned = OwnedFile {
            file,
            name,
            content: content.to_vec(),
            mode,
        };
        match self.sync_directory() {
            Ok(()) => Ok(owned),
            Err(error) => Err(PublishFailure::Published { error, owned }),
        }
    }

    /// Explicit cleanup for the same locked operation that created this lease.
    /// The higher-level installer must serialize Lyn operations; this is not an
    /// atomic compare-and-unlink against a malicious same-uid concurrent writer.
    pub(super) fn remove(&self, owned: &OwnedFile) -> io::Result<()> {
        self.verify(owned)?;
        self.unlink(&owned.name)?;
        self.sync_directory()
    }

    pub(super) fn verify(&self, owned: &OwnedFile) -> io::Result<()> {
        self.check()?;
        self.same_entry(&owned.name, &owned.file)?;
        let mut current = self.open(&owned.name, libc::O_RDONLY)?;
        let metadata = current.metadata()?;
        let original = owned.file.metadata()?;
        if !metadata.is_file()
            || metadata.dev() != original.dev()
            || metadata.ino() != original.ino()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o7777 != owned.mode
            || metadata.nlink() != 1
            || metadata.len() != owned.content.len() as u64
        {
            return Err(refused());
        }
        self.same_entry(&owned.name, &owned.file)?;
        let mut bytes = Vec::new();
        (&mut current)
            .take((MAX_CONTENT + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes != owned.content {
            return Err(refused());
        }
        self.same_entry(&owned.name, &owned.file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    };

    fn directory() -> (tempfile::TempDir, ManagedDirectory) {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let directory =
            ManagedDirectory::from_directory(fs::File::open(root.path()).unwrap()).unwrap();
        (root, directory)
    }

    #[test]
    fn publication_is_complete_and_never_overwrites_an_existing_name() {
        let (root, directory) = directory();
        let owned = directory
            .publish("launch", b"#!/bin/sh\nexit 0\n", true)
            .unwrap();
        assert_eq!(
            fs::read(root.path().join("launch")).unwrap(),
            b"#!/bin/sh\nexit 0\n"
        );
        let metadata = fs::metadata(root.path().join("launch")).unwrap();
        assert_eq!(metadata.mode() & 0o7777, 0o700);
        assert_eq!(metadata.nlink(), 1);
        let failure = directory
            .publish("launch", b"replacement", true)
            .unwrap_err();
        let PublishFailure::Unpublished(error) = failure else {
            panic!("an occupied name must remain unpublished");
        };
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        directory.remove(&owned).unwrap();
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn failed_stage_cleanup_returns_its_lease_without_touching_the_existing_target() {
        let (root, directory) = directory();
        fs::write(root.path().join("activation"), b"user override").unwrap();
        directory.fault.set(Fault::StageCleanup);
        let failure = directory
            .publish("activation", b"replacement", false)
            .unwrap_err();
        let PublishFailure::Staged {
            error,
            cleanup_error,
            stage,
        } = failure
        else {
            panic!("missing staged lease");
        };
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(cleanup_error.kind(), io::ErrorKind::Other);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
        assert_eq!(
            fs::read(root.path().join("activation")).unwrap(),
            b"user override"
        );
        directory.fault.set(Fault::None);
        directory.discard_stage(&stage).unwrap();
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn postpublication_sync_failure_retains_ownership_for_explicit_rollback() {
        let (root, directory) = directory();
        directory.fault.set(Fault::DirectorySync);
        let failure = directory
            .publish("activation", b"complete", false)
            .unwrap_err();
        let PublishFailure::Published { error, owned } = failure else {
            panic!("missing published lease");
        };
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(
            fs::read(root.path().join("activation")).unwrap(),
            b"complete"
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        directory.fault.set(Fault::None);
        directory.remove(&owned).unwrap();
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn removal_preserves_in_place_edits_and_replaced_files() {
        let (root, directory) = directory();
        let owned = directory.publish("activation", b"original", false).unwrap();
        fs::write(root.path().join("activation"), b"modified").unwrap();
        assert!(directory.remove(&owned).is_err());
        assert_eq!(
            fs::read(root.path().join("activation")).unwrap(),
            b"modified"
        );
        fs::rename(root.path().join("activation"), root.path().join("old")).unwrap();
        fs::write(root.path().join("activation"), b"original").unwrap();
        assert!(directory.remove(&owned).is_err());
        assert_eq!(
            fs::read(root.path().join("activation")).unwrap(),
            b"original"
        );
    }

    #[test]
    fn symlinks_hardlinks_and_path_traversal_are_not_owned_files() {
        let (root, directory) = directory();
        let outside = tempfile::NamedTempFile::new().unwrap();
        symlink(outside.path(), root.path().join("linked")).unwrap();
        assert!(directory.publish("linked", b"never", false).is_err());
        let owned = directory.publish("activation", b"original", false).unwrap();
        fs::hard_link(root.path().join("activation"), root.path().join("alias")).unwrap();
        assert!(directory.remove(&owned).is_err());
        for name in [
            "",
            ".",
            "..",
            "../escape",
            "nested/file",
            "/absolute",
            "nul\0name",
        ] {
            assert!(
                directory.publish(name, b"never", false).is_err(),
                "{name:?}"
            );
        }
        assert_eq!(fs::read(outside.path()).unwrap(), b"");
    }

    #[test]
    fn directory_identity_survives_rename_and_refuses_nonprivate_modes() {
        let (root, directory) = directory();
        let parent = tempfile::tempdir().unwrap();
        let moved = parent.path().join("moved");
        fs::rename(root.path(), &moved).unwrap();
        let owned = directory.publish("activation", b"original", false).unwrap();
        assert!(moved.join("activation").is_file());
        directory.remove(&owned).unwrap();
        fs::set_permissions(&moved, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(ManagedDirectory::from_directory(fs::File::open(&moved).unwrap()).is_err());
        assert!(directory.publish("activation", b"never", false).is_err());
    }
}
