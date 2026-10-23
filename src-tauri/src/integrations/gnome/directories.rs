//! Resolve activation directories without following an ancestor symlink.

use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Component, Path},
};

use super::{activation_path, managed::ManagedDirectory};

fn check_ancestor(directory: &File) -> io::Result<()> {
    let metadata = directory.metadata()?;
    let owner = metadata.uid();
    let mode = metadata.mode();
    // A root-owned sticky directory (e.g. /tmp) protects our direct child's
    // name. Every child is still opened and checked separately, without links.
    let root_sticky = owner == 0 && mode & libc::S_ISVTX != 0;
    if !metadata.is_dir()
        || ![0, unsafe { libc::geteuid() }].contains(&owner)
        || (mode & 0o022 != 0 && !root_sticky)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unsafe activation ancestor",
        ));
    }
    Ok(())
}

/// Existing ancestors may be root/user owned and non-writable to others;
/// the final directory must already be private or be newly created mode0700.
/// Never chmod or remove an existing directory to make it pass validation.
pub(super) fn open_private(path: &Path, create: bool) -> io::Result<ManagedDirectory> {
    ManagedDirectory::from_directory(open_directory(path, create)?)
}

/// Same ancestry checks, but a protected user-owned final directory may keep
/// its existing read/search permissions. Newly created directories stay0700.
pub(super) fn open_drop_ins(path: &Path, create: bool) -> io::Result<ManagedDirectory> {
    ManagedDirectory::from_drop_in_directory(open_directory(path, create)?)
}

fn open_directory(path: &Path, create: bool) -> io::Result<File> {
    activation_path(path)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid activation directory"))?;
    let components: Vec<_> = path.components().collect();
    if components.len() > 64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "activation directory too deep",
        ));
    }
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")?;
    for component in components {
        let Component::Normal(name) = component else {
            if component == Component::RootDir {
                continue;
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid activation component",
            ));
        };
        check_ancestor(&directory)?;
        let name = CString::new(name.as_bytes()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid activation component")
        })?;
        let open = || {
            let fd = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY
                        | libc::O_DIRECTORY
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC
                        | libc::O_NONBLOCK,
                )
            };
            if fd < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(unsafe { File::from_raw_fd(fd) })
            }
        };
        let next = match open() {
            Ok(next) => next,
            Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                if unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(error);
                    }
                }
                let next = open()?;
                directory.sync_all()?;
                next
            }
            Err(error) => return Err(error),
        };
        directory = next;
    }
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };

    #[test]
    fn private_tree_can_be_created_and_reopened_without_changing_existing_modes() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
        let path = root.path().join("data/lyn/gnome");
        assert!(open_private(&path, false).is_err());
        let directory = open_private(&path, true).unwrap();
        let file = directory.publish("witness", b"exact", false).unwrap();
        assert_eq!(fs::read(path.join("witness")).unwrap(), b"exact");
        open_private(&path, false).unwrap();
        assert_eq!(
            fs::metadata(root.path()).unwrap().permissions().mode() & 0o7777,
            0o755
        );
        directory.remove(&file).unwrap();
    }

    #[test]
    fn ancestor_and_leaf_symlinks_refuse_without_writing_through_them() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join("redirect")).unwrap();
        assert!(open_private(&root.path().join("redirect/child"), true).is_err());
        assert!(open_private(&root.path().join("redirect"), false).is_err());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[test]
    fn unsafe_permissions_and_ambiguous_paths_are_not_repaired_or_traversed() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o777)).unwrap();
        assert!(open_private(&root.path().join("child"), true).is_err());
        assert!(!root.path().join("child").exists());
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(open_private(root.path(), false).is_err());
        for path in ["relative", "/", "/tmp/../escape", "/tmp/a/./b"] {
            assert!(
                open_private(std::path::Path::new(path), true).is_err(),
                "{path}"
            );
        }
    }

    #[test]
    fn protected_drop_in_directory_preserves_0755_but_cannot_be_private_state() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(root.path().join("user.conf"), b"user settings").unwrap();
        let directory = open_drop_ins(root.path(), false).unwrap();
        assert!(open_private(root.path(), false).is_err());
        let owned = directory.publish("probe", b"owned", false).unwrap();
        assert!(directory.read_state("probe").is_err());
        let receipt = super::super::ownership::Receipt::record(&directory, &owned).unwrap();
        assert!(receipt.store(&directory, "receipt.json").is_err());
        assert!(!root.path().join("receipt.json").exists());
        assert!(super::super::lock::InstallationLock::acquire(&directory).is_err());
        assert!(!root.path().join(".installation.lock").exists());
        directory.remove(&owned).unwrap();
        assert_eq!(
            fs::metadata(root.path()).unwrap().permissions().mode() & 0o7777,
            0o755
        );
        assert_eq!(
            fs::read(root.path().join("user.conf")).unwrap(),
            b"user settings"
        );
    }

    #[test]
    fn drop_in_directory_refuses_unsafe_modes_links_and_later_permission_changes() {
        let root = tempfile::tempdir().unwrap();
        for mode in [0o777, 0o775, 0o770, 0o1755, 0o2755, 0o4755, 0o600] {
            fs::set_permissions(root.path(), fs::Permissions::from_mode(mode)).unwrap();
            assert!(open_drop_ins(root.path(), false).is_err(), "{mode:o}");
            assert_eq!(
                fs::metadata(root.path()).unwrap().permissions().mode() & 0o7777,
                mode
            );
        }
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
        let directory = open_drop_ins(root.path(), false).unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o750)).unwrap();
        assert!(directory.publish("probe", b"never", false).is_err());
        let parent = tempfile::tempdir().unwrap();
        fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
        symlink(root.path(), parent.path().join("redirect")).unwrap();
        assert!(open_drop_ins(&parent.path().join("redirect"), false).is_err());
        assert!(!root.path().join("probe").exists());
    }
}
