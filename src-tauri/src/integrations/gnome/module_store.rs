//! Atomic, immutable module versions. No service writes or automatic deletion.

#![allow(dead_code)] // Some application consumers remain gated pending acceptance.

use super::{
    artifact::{Artifact, format},
    lock::InstallationLock,
    managed::{ManagedDirectory, OwnedFile, PublishFailure, basename},
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CStr, CString},
    fs::{File, Permissions},
    io::{self, Read, Seek, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, PermissionsExt},
    },
};

const RECORD: &str = "ownership-v1.json";
const MAX_ENTRIES: usize = 64;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u8,
    directory_device: u64,
    directory_inode: u64,
    device: u64,
    inode: u64,
    ctime: i64,
    ctime_nsec: i64,
    manifest: format::Manifest,
}

fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "native module ownership changed",
    )
}

/// Open a separate directory description so enumeration never leaves the
/// retained descriptor's offset at EOF. No paths, symlinks or recursive scan.
fn count_entries(directory: &ManagedDirectory, limit: usize) -> io::Result<usize> {
    directory.check_private()?;
    let file = directory.open(&c".".to_owned(), libc::O_RDONLY | libc::O_DIRECTORY)?;
    use std::os::fd::IntoRawFd;
    let fd = file.into_raw_fd();
    let pointer = unsafe { libc::fdopendir(fd) };
    if pointer.is_null() {
        let error = io::Error::last_os_error();
        unsafe {
            libc::close(fd);
        }
        return Err(error);
    }
    struct Directory(*mut libc::DIR);
    impl Drop for Directory {
        fn drop(&mut self) {
            unsafe {
                libc::closedir(self.0);
            }
        }
    }
    let handle = Directory(pointer);
    let mut count = 0;
    loop {
        unsafe {
            *libc::__errno_location() = 0;
        }
        let entry = unsafe { libc::readdir(handle.0) };
        if entry.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(0) {
                return Err(error);
            }
            directory.check_private()?;
            return Ok(count);
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        count += 1;
        if count > limit {
            return Err(io::Error::other("native module store capacity exceeded"));
        }
    }
}

pub(super) struct ModuleStore<'a> {
    root: &'a ManagedDirectory,
    lock: InstallationLock<'a>,
}

pub(super) struct StoredModule {
    name: CString,
    directory: ManagedDirectory,
    module: File,
    record: OwnedFile,
}

pub(super) fn read_existing(root: &ManagedDirectory, digest: &str) -> io::Result<StoredModule> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(refused());
    }
    root.check_private()?;
    let name = basename(digest)?;
    let file = root.open(&name, libc::O_RDONLY | libc::O_DIRECTORY)?;
    let stored = open_files(name, file)?;
    verify_existing(root, &stored)?;
    Ok(stored)
}

pub(super) fn verify_existing(root: &ManagedDirectory, stored: &StoredModule) -> io::Result<()> {
    verify_stored(root, stored, None)
}

fn open_files(name: CString, file: File) -> io::Result<StoredModule> {
    let directory = ManagedDirectory::from_directory(file)?;
    let record = directory.read_state(RECORD)?;
    let module = directory.open(&basename(format::MODULE_NAME)?, libc::O_RDONLY)?;
    Ok(StoredModule {
        name,
        directory,
        module,
        record,
    })
}

impl<'a> ModuleStore<'a> {
    pub(super) fn open(root: &'a ManagedDirectory) -> io::Result<Self> {
        let count = count_entries(root, MAX_ENTRIES)?;
        match root.require_absent(".installation.lock") {
            Ok(()) if count >= MAX_ENTRIES => {
                return Err(io::Error::other("native module store capacity exceeded"));
            }
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        Ok(Self {
            root,
            lock: InstallationLock::acquire(root)?,
        })
    }

    /// Error may leave an inert private stage or a fully published version.
    /// Neither is removed on Drop; a published version is retryable by receipt.
    pub(super) fn retain(&self, artifact: &Artifact) -> io::Result<StoredModule> {
        self.lock.check()?;
        let manifest = serde_json::to_vec(&artifact.manifest).map_err(io::Error::other)?;
        format::validate(&manifest, artifact.bytes).map_err(io::Error::other)?;
        let destination = basename(&artifact.manifest.sha256)?;
        match self
            .root
            .open(&destination, libc::O_RDONLY | libc::O_DIRECTORY)
        {
            Ok(file) => {
                let stored = self.load(destination, file, artifact)?;
                stored.directory.sync_directory()?;
                self.root.sync_directory()?;
                self.verify(&stored, artifact)?;
                return Ok(stored);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if count_entries(self.root, MAX_ENTRIES)? >= MAX_ENTRIES {
            return Err(io::Error::other("native module store capacity exceeded"));
        }
        self.lock.check()?;
        let stage = basename(&format!(".lyn-module-{}", uuid::Uuid::new_v4()))?;
        if unsafe { libc::mkdirat(self.root.directory.as_raw_fd(), stage.as_ptr(), 0o700) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let directory = ManagedDirectory::from_directory(
            self.root.open(&stage, libc::O_RDONLY | libc::O_DIRECTORY)?,
        )?;
        self.root.same_entry(&stage, &directory.directory)?;
        let mut module = directory.open(
            &basename(format::MODULE_NAME)?,
            libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
        )?;
        module.write_all(artifact.bytes)?;
        module.set_permissions(Permissions::from_mode(0o400))?;
        module.sync_all()?;
        let metadata = module.metadata()?;
        let parent = directory.directory.metadata()?;
        let record = Record {
            version: 1,
            directory_device: parent.dev(),
            directory_inode: parent.ino(),
            device: metadata.dev(),
            inode: metadata.ino(),
            ctime: metadata.ctime(),
            ctime_nsec: metadata.ctime_nsec(),
            manifest: serde_json::from_slice(&manifest).map_err(io::Error::other)?,
        };
        directory
            .publish(
                RECORD,
                &serde_json::to_vec(&record).map_err(io::Error::other)?,
                false,
            )
            .map_err(|failure| match failure {
                PublishFailure::Unpublished(error)
                | PublishFailure::Published { error, .. }
                | PublishFailure::Staged { error, .. } => error,
            })?;
        drop(module); // Retain only a read descriptor in the published lease.
        let file = directory.directory.try_clone()?;
        let mut stored = self.load(stage, file, artifact)?;
        directory.sync_directory()?;
        self.lock.check()?;
        self.verify(&stored, artifact)?;
        if unsafe {
            libc::renameat2(
                self.root.directory.as_raw_fd(),
                stored.name.as_ptr(),
                self.root.directory.as_raw_fd(),
                destination.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        stored.name = destination;
        self.root.sync_directory()?;
        self.verify(&stored, artifact)?;
        Ok(stored)
    }

    fn load(&self, name: CString, file: File, artifact: &Artifact) -> io::Result<StoredModule> {
        let stored = open_files(name, file)?;
        self.verify(&stored, artifact)?;
        Ok(stored)
    }

    pub(super) fn verify(&self, stored: &StoredModule, artifact: &Artifact) -> io::Result<()> {
        self.lock.check()?;
        verify_stored(self.root, stored, Some(artifact))?;
        self.lock.check()
    }
}

// The writer compares against the embedded candidate, while the read-only
// existing-version path requires the published digest name and its own receipt.
fn verify_stored(
    root: &ManagedDirectory,
    stored: &StoredModule,
    expected: Option<&Artifact>,
) -> io::Result<()> {
    root.check_private()?;
    root.same_entry(&stored.name, &stored.directory.directory)?;
    stored.directory.check_private()?;
    if count_entries(&stored.directory, 2)? != 2 {
        return Err(refused());
    }
    stored.directory.verify(&stored.record)?;
    let record: Record = serde_json::from_slice(&stored.record.content).map_err(|_| refused())?;
    let parent = stored.directory.directory.metadata()?;
    if record.version != 1
        || expected.is_some_and(|artifact| record.manifest != artifact.manifest)
        || (expected.is_none() && stored.name.to_bytes() != record.manifest.sha256.as_bytes())
        || (parent.dev(), parent.ino()) != (record.directory_device, record.directory_inode)
        || !(0..1_000_000_000).contains(&record.ctime_nsec)
    {
        return Err(refused());
    }
    let check_file = || -> io::Result<()> {
        let metadata = stored.module.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.nlink() != 1
            || metadata.mode() & 0o7777 != 0o400
            || metadata.len() != record.manifest.length as u64
            || (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            ) != (record.device, record.inode, record.ctime, record.ctime_nsec)
        {
            return Err(refused());
        }
        stored
            .directory
            .same_entry(&basename(format::MODULE_NAME)?, &stored.module)
    };
    check_file()?;
    let mut file = stored.module.try_clone()?;
    file.rewind()?;
    let mut bytes = Vec::new();
    file.take((format::MAX_MODULE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    let manifest = serde_json::to_vec(&record.manifest).map_err(io::Error::other)?;
    format::validate(&manifest, &bytes).map_err(|_| refused())?;
    if expected.is_some_and(|artifact| bytes != artifact.bytes) {
        return Err(refused());
    }
    check_file()?;
    stored.directory.verify(&stored.record)?;
    root.same_entry(&stored.name, &stored.directory.directory)?;
    root.check_private()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    };

    fn artifact(marker: u8) -> Artifact {
        use sha2::{Digest, Sha256};
        let mut bytes = vec![marker; 32768];
        bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        bytes[16..18].copy_from_slice(&3_u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&62_u16.to_le_bytes());
        bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
        bytes[52..54].copy_from_slice(&64_u16.to_le_bytes());
        let manifest = format::Manifest {
            version: 1,
            protocol: 1,
            target: format::TARGET.into(),
            gtk: [3, 24, 33],
            vte: [0, 68, 0],
            length: bytes.len(),
            sha256: Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        };
        Artifact {
            manifest,
            bytes: bytes.leak(),
        }
    }

    #[test]
    fn lifecycle_reader_reopens_old_version_without_new_payload_or_writer_lock() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let directory = super::super::directories::open_private(root.path(), false).unwrap();
        let old = artifact(0);
        let new = artifact(1);
        let store = ModuleStore::open(&directory).unwrap();
        store.retain(&old).unwrap();
        store.retain(&new).unwrap();
        let before = fs::read_dir(root.path()).unwrap().count();
        // Writer lock intentionally remains held during both read operations.
        let existing = read_existing(&directory, &old.manifest.sha256).unwrap();
        verify_existing(&directory, &existing).unwrap();
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), before);
        assert!(read_existing(&directory, "../escape").is_err());
    }

    #[test]
    fn real_sized_module_survives_reopen_and_old_version_is_retained() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let directory = super::super::directories::open_private(root.path(), false).unwrap();
        let first = artifact(0);
        let second = artifact(1);
        {
            let store = ModuleStore::open(&directory).unwrap();
            let _first = store.retain(&first).unwrap();
            let _second = store.retain(&second).unwrap();
        }
        let reopened = super::super::directories::open_private(root.path(), false).unwrap();
        let store = ModuleStore::open(&reopened).unwrap();
        store.retain(&first).unwrap();
        store.retain(&second).unwrap();
        for item in [&first, &second] {
            let path = root
                .path()
                .join(&item.manifest.sha256)
                .join(format::MODULE_NAME);
            assert_eq!(fs::read(&path).unwrap(), item.bytes);
            assert_eq!(fs::metadata(path).unwrap().mode() & 0o7777, 0o400);
        }
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3);
        // The configuration/receipt publication boundary is still 16 KiB.
        assert!(
            directory
                .publish("oversize-config", first.bytes, false)
                .is_err()
        );
    }

    #[test]
    fn unknown_occupied_version_and_links_are_never_adopted_or_replaced() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let directory = super::super::directories::open_private(root.path(), false).unwrap();
        let artifact = artifact(0);
        let path = root.path().join(&artifact.manifest.sha256);
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(path.join(format::MODULE_NAME), artifact.bytes).unwrap();
        fs::set_permissions(
            path.join(format::MODULE_NAME),
            fs::Permissions::from_mode(0o400),
        )
        .unwrap();
        let store = ModuleStore::open(&directory).unwrap();
        assert!(store.retain(&artifact).is_err());
        assert_eq!(fs::read_dir(&path).unwrap().count(), 1);
        fs::remove_file(path.join(format::MODULE_NAME)).unwrap();
        fs::remove_dir(&path).unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), &path).unwrap();
        assert!(store.retain(&artifact).is_err());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[test]
    fn changes_to_module_bytes_identity_links_or_permissions_refuse() {
        for change in [
            "bytes",
            "identical-rewrite",
            "replacement",
            "hardlink",
            "mode",
        ] {
            let root = tempfile::tempdir().unwrap();
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
            let directory = super::super::directories::open_private(root.path(), false).unwrap();
            let artifact = artifact(0);
            let store = ModuleStore::open(&directory).unwrap();
            let stored = store.retain(&artifact).unwrap();
            let path = root
                .path()
                .join(&artifact.manifest.sha256)
                .join(format::MODULE_NAME);
            match change {
                "replacement" => {
                    fs::rename(&path, root.path().join("original")).unwrap();
                    fs::write(&path, artifact.bytes).unwrap();
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
                }
                "hardlink" => fs::hard_link(&path, root.path().join("alias")).unwrap(),
                _ => {
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
                    if change != "mode" {
                        let mut bytes = artifact.bytes.to_vec();
                        if change == "bytes" {
                            bytes[63] ^= 1;
                        }
                        fs::write(&path, bytes).unwrap();
                        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
                    }
                }
            }
            let before = fs::read(&path).unwrap();
            assert!(store.verify(&stored, &artifact).is_err(), "{change}");
            assert!(store.retain(&artifact).is_err(), "{change}");
            assert_eq!(fs::read(path).unwrap(), before, "{change}");
        }
    }

    #[test]
    fn copied_directory_or_extra_files_do_not_qualify_as_the_owned_version() {
        for change in [
            "copied-directory",
            "extra-file",
            "module-symlink",
            "record-symlink",
            "record-fields",
        ] {
            let root = tempfile::tempdir().unwrap();
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
            let directory = super::super::directories::open_private(root.path(), false).unwrap();
            let artifact = artifact(0);
            let store = ModuleStore::open(&directory).unwrap();
            let stored = store.retain(&artifact).unwrap();
            let path = root.path().join(&artifact.manifest.sha256);
            match change {
                "copied-directory" => {
                    let original = root.path().join("original");
                    fs::rename(&path, &original).unwrap();
                    fs::create_dir(&path).unwrap();
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
                    for name in [format::MODULE_NAME, RECORD] {
                        fs::copy(original.join(name), path.join(name)).unwrap();
                    }
                }
                "extra-file" => fs::write(path.join("settings"), b"user data").unwrap(),
                "record-fields" => {
                    let record = path.join(RECORD);
                    let mut json: serde_json::Value =
                        serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
                    json["unknown"] = true.into();
                    fs::write(record, serde_json::to_vec(&json).unwrap()).unwrap();
                }
                _ => {
                    let name = if change == "module-symlink" {
                        format::MODULE_NAME
                    } else {
                        RECORD
                    };
                    let outside = root.path().join("original");
                    fs::rename(path.join(name), &outside).unwrap();
                    symlink(outside, path.join(name)).unwrap();
                }
            }
            assert!(store.verify(&stored, &artifact).is_err(), "{change}");
            assert!(store.retain(&artifact).is_err(), "{change}");
            assert!(path.is_dir());
        }
    }

    #[test]
    fn complete_publication_is_retryable_after_parent_sync_failure() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let directory = super::super::directories::open_private(root.path(), false).unwrap();
        let artifact = artifact(0);
        let store = ModuleStore::open(&directory).unwrap();
        directory.fail_directory_sync(true);
        assert!(store.retain(&artifact).is_err());
        let path = root
            .path()
            .join(&artifact.manifest.sha256)
            .join(format::MODULE_NAME);
        let inode = fs::metadata(&path).unwrap().ino();
        assert_eq!(fs::read(&path).unwrap(), artifact.bytes);
        directory.fail_directory_sync(false);
        store.retain(&artifact).unwrap();
        assert_eq!(fs::metadata(path).unwrap().ino(), inode);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[test]
    fn inert_stages_are_preserved_and_capacity_counts_unknown_entries() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let directory = super::super::directories::open_private(root.path(), false).unwrap();
        let store = ModuleStore::open(&directory).unwrap();
        let stage = root.path().join(".lyn-module-abandoned");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join(format::MODULE_NAME), b"partial").unwrap();
        let first = artifact(0);
        store.retain(&first).unwrap();
        assert_eq!(
            fs::read(stage.join(format::MODULE_NAME)).unwrap(),
            b"partial"
        );
        for index in 0..MAX_ENTRIES - 3 {
            fs::write(root.path().join(format!("unknown-{index}")), b"keep").unwrap();
        }
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), MAX_ENTRIES);
        assert!(store.retain(&artifact(1)).is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), MAX_ENTRIES);
        store.retain(&first).unwrap();
        drop(store);
        ModuleStore::open(&directory)
            .unwrap()
            .retain(&first)
            .unwrap();
    }

    #[test]
    fn full_unknown_directory_refuses_even_lock_creation() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let directory = super::super::directories::open_private(root.path(), false).unwrap();
        for index in 0..MAX_ENTRIES {
            fs::write(root.path().join(format!("unknown-{index}")), b"keep").unwrap();
        }
        assert!(ModuleStore::open(&directory).is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), MAX_ENTRIES);
        assert!(!root.path().join(".installation.lock").exists());
    }
}
