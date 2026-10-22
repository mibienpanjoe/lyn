//! Atomic swaps between two verified owned activation files. The old bytes
//! remain named and pinned until the surrounding transaction commits.

use super::managed::{ManagedDirectory, OwnedFile};
use std::{io, os::fd::AsRawFd};

/// Physical exchange succeeded. A durability error must not be confused with
/// no filesystem effect; both supplied leases already track their new names.
pub(super) struct Exchanged {
    pub(super) durability: io::Result<()>,
}

impl ManagedDirectory {
    /// Both files must be in this directory, of the same executable/data role,
    /// and currently unchanged. The installer holds its lock across this call.
    pub(super) fn exchange(
        &self,
        left: &mut OwnedFile,
        right: &mut OwnedFile,
    ) -> io::Result<Exchanged> {
        if left.name == right.name || left.mode != right.mode {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid activation exchange",
            ));
        }
        self.verify(left)?;
        self.verify(right)?;
        if unsafe {
            libc::renameat2(
                self.directory.as_raw_fd(),
                left.name.as_ptr(),
                self.directory.as_raw_fd(),
                right.name.as_ptr(),
                libc::RENAME_EXCHANGE,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        std::mem::swap(&mut left.name, &mut right.name);
        Ok(Exchanged {
            durability: self.sync_directory(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt},
    };

    #[test]
    fn exchange_and_rollback_preserve_both_original_inodes_and_exact_bytes() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let directory = super::super::directories::open_private(root.path(), false).unwrap();
        let mut original = directory.publish("launcher", b"old", true).unwrap();
        let mut candidate = directory.publish("candidate", b"new", true).unwrap();
        let original_inode = fs::metadata(root.path().join("launcher")).unwrap().ino();
        let candidate_inode = fs::metadata(root.path().join("candidate")).unwrap().ino();
        directory
            .exchange(&mut original, &mut candidate)
            .unwrap()
            .durability
            .unwrap();
        assert_eq!(fs::read(root.path().join("launcher")).unwrap(), b"new");
        assert_eq!(fs::read(root.path().join("candidate")).unwrap(), b"old");
        assert_eq!(original.name.to_bytes(), b"candidate");
        assert_eq!(candidate.name.to_bytes(), b"launcher");
        directory
            .exchange(&mut original, &mut candidate)
            .unwrap()
            .durability
            .unwrap();
        assert_eq!(
            fs::metadata(root.path().join("launcher")).unwrap().ino(),
            original_inode
        );
        assert_eq!(
            fs::metadata(root.path().join("candidate")).unwrap().ino(),
            candidate_inode
        );
        assert_eq!(fs::read(root.path().join("launcher")).unwrap(), b"old");
        directory.remove(&candidate).unwrap();
        directory.remove(&original).unwrap();
    }

    #[test]
    fn unknown_replacement_refuses_before_either_name_is_exchanged() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let directory = super::super::directories::open_private(root.path(), false).unwrap();
        let mut original = directory.publish("launcher", b"old", true).unwrap();
        let mut candidate = directory.publish("candidate", b"new", true).unwrap();
        fs::rename(root.path().join("launcher"), root.path().join("retired")).unwrap();
        fs::write(root.path().join("launcher"), b"user override").unwrap();
        assert!(directory.exchange(&mut original, &mut candidate).is_err());
        assert_eq!(
            fs::read(root.path().join("launcher")).unwrap(),
            b"user override"
        );
        assert_eq!(fs::read(root.path().join("candidate")).unwrap(), b"new");
    }

    #[test]
    fn postexchange_sync_failure_keeps_swapped_names_and_leases_for_recovery() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let directory = super::super::directories::open_private(root.path(), false).unwrap();
        let mut original = directory.publish("launcher", b"old", false).unwrap();
        let mut candidate = directory.publish("candidate", b"new", false).unwrap();
        directory.fail_directory_sync(true);
        let outcome = directory.exchange(&mut original, &mut candidate).unwrap();
        assert!(outcome.durability.is_err());
        assert_eq!(original.name.to_bytes(), b"candidate");
        assert_eq!(candidate.name.to_bytes(), b"launcher");
        directory.fail_directory_sync(false);
        directory
            .exchange(&mut original, &mut candidate)
            .unwrap()
            .durability
            .unwrap();
        assert_eq!(fs::read(root.path().join("launcher")).unwrap(), b"old");
        directory.remove(&candidate).unwrap();
        directory.remove(&original).unwrap();
    }
}
