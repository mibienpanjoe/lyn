//! Bounded on-disk ownership receipts, consumed only from Lyn's private state.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read},
    os::unix::fs::MetadataExt,
};

use super::managed::{MAX_CONTENT, ManagedDirectory, OwnedFile, PublishFailure, basename};

/// An integrity/ownership record, not a credential or kernel lifetime proof.
/// Only read from a bounded, no-symlink, same-user private installer manifest.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    version: u8,
    name: String,
    directory_device: u64,
    directory_inode: u64,
    device: u64,
    inode: u64,
    ctime: i64,
    ctime_nsec: i64,
    mode: u32,
    length: u64,
    sha256: [u8; 32],
}

fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "activation receipt mismatch",
    )
}

impl Receipt {
    pub(super) fn filename(&self) -> &str {
        &self.name
    }

    pub(super) fn same_inode(&self, other: &Self) -> bool {
        self.directory_device == other.directory_device
            && self.directory_inode == other.directory_inode
            && self.device == other.device
            && self.inode == other.inode
    }

    pub(super) fn same_payload_identity(&self, other: &Self) -> bool {
        self.version == other.version
            && self.same_inode(other)
            && self.name == other.name
            && self.mode == other.mode
            && self.length == other.length
            && self.sha256 == other.sha256
    }

    pub(super) fn store(
        &self,
        state: &ManagedDirectory,
        name: &str,
    ) -> Result<OwnedFile, PublishFailure> {
        state.check_private()?;
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(io::Error::other)?;
        state.publish(name, &bytes, false)
    }

    pub(super) fn load(state: &ManagedDirectory, name: &str) -> io::Result<Self> {
        Self::decode(&state.read_state(name)?.content)
    }

    pub(super) fn is_for(&self, name: &str, mode: u32) -> bool {
        self.name == name && self.mode == mode
    }

    pub(super) fn validate(&self) -> io::Result<()> {
        basename(&self.name)?;
        if self.version != 1
            || ![0o600, 0o700].contains(&self.mode)
            || self.length > MAX_CONTENT as u64
            || !(0..1_000_000_000).contains(&self.ctime_nsec)
        {
            return Err(refused());
        }
        Ok(())
    }

    pub(super) fn decode(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > MAX_CONTENT {
            return Err(refused());
        }
        let receipt: Self = serde_json::from_slice(bytes).map_err(|_| refused())?;
        receipt.validate()?;
        Ok(receipt)
    }

    pub(super) fn record(directory: &ManagedDirectory, owned: &OwnedFile) -> io::Result<Self> {
        directory.verify(owned)?;
        let parent = directory.directory.metadata()?;
        let file = owned.file.metadata()?;
        Ok(Self {
            version: 1,
            name: owned.name.to_str().map_err(|_| refused())?.to_owned(),
            directory_device: parent.dev(),
            directory_inode: parent.ino(),
            device: file.dev(),
            inode: file.ino(),
            ctime: file.ctime(),
            ctime_nsec: file.ctime_nsec(),
            mode: owned.mode,
            length: owned.content.len() as u64,
            sha256: Sha256::digest(&owned.content).into(),
        })
    }

    pub(super) fn reopen(&self, directory: &ManagedDirectory) -> io::Result<OwnedFile> {
        self.reopen_at(directory, &self.name, true)
    }

    /// Only a validated private install journal may use this relocation path.
    /// Rename changes ctime; inode, parent, owner, mode, size and digest remain
    /// mandatory. This is not a kernel lifetime proof or generic authorization.
    pub(super) fn reopen_after_rename(
        &self,
        directory: &ManagedDirectory,
        destination: &str,
    ) -> io::Result<OwnedFile> {
        self.reopen_at(directory, destination, destination == self.name)
    }

    /// Only for a validated exchange intent's original/candidate slots. Both
    /// rename directions change ctime; exact directory/inode/owner/mode/content
    /// remain required. Ordinary reopening is deliberately unchanged.
    pub(super) fn reopen_exchange_slot(
        &self,
        directory: &ManagedDirectory,
        destination: &str,
    ) -> io::Result<OwnedFile> {
        self.reopen_at(directory, destination, false)
    }

    fn reopen_at(
        &self,
        directory: &ManagedDirectory,
        destination: &str,
        check_ctime: bool,
    ) -> io::Result<OwnedFile> {
        self.validate()?;
        directory.check()?;
        let parent = directory.directory.metadata()?;
        if (parent.dev(), parent.ino()) != (self.directory_device, self.directory_inode) {
            return Err(refused());
        }
        let name = basename(destination)?;
        let mut file = directory.open(&name, libc::O_RDONLY)?;
        let check_metadata = |file: &std::fs::File| -> io::Result<()> {
            let metadata = file.metadata()?;
            if !metadata.is_file()
                || metadata.uid() != unsafe { libc::geteuid() }
                || metadata.nlink() != 1
                || metadata.mode() & 0o7777 != self.mode
                || metadata.len() != self.length
                || metadata.dev() != self.device
                || metadata.ino() != self.inode
                || (check_ctime
                    && (metadata.ctime() != self.ctime || metadata.ctime_nsec() != self.ctime_nsec))
            {
                return Err(refused());
            }
            Ok(())
        };
        check_metadata(&file)?;
        let mut content = Vec::new();
        (&mut file)
            .take((MAX_CONTENT + 1) as u64)
            .read_to_end(&mut content)?;
        if content.len() as u64 != self.length
            || <[u8; 32]>::from(Sha256::digest(&content)) != self.sha256
        {
            return Err(refused());
        }
        check_metadata(&file)?;
        directory.same_entry(&name, &file)?;
        Ok(OwnedFile {
            file,
            name,
            content,
            mode: self.mode,
        })
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
    fn serialized_receipt_reopens_only_the_original_unchanged_file_after_restart() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let directory = super::super::directories::open_private(root.path(), false).unwrap();
        let owned = directory.publish("launch", b"original", true).unwrap();
        let receipt = Receipt::record(&directory, &owned).unwrap();
        let state =
            super::super::directories::open_private(&root.path().join("state"), true).unwrap();
        let record = receipt.store(&state, "launch-receipt.json").unwrap();
        drop(record);
        drop(state);
        drop(owned);
        drop(directory);
        let directory = super::super::directories::open_private(root.path(), false).unwrap();
        let state =
            super::super::directories::open_private(&root.path().join("state"), false).unwrap();
        let receipt = Receipt::load(&state, "launch-receipt.json").unwrap();
        let reopened = receipt.reopen(&directory).unwrap();
        directory.remove(&reopened).unwrap();
        assert!(!root.path().join("launch").exists());
    }

    #[test]
    fn receipts_do_not_adopt_edits_or_same_content_replacements() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let directory = super::super::directories::open_private(root.path(), false).unwrap();
        let owned = directory.publish("launch", b"original", false).unwrap();
        let receipt = Receipt::record(&directory, &owned).unwrap();
        fs::write(root.path().join("launch"), b"changed!").unwrap();
        assert!(receipt.reopen(&directory).is_err());
        assert!(Receipt::record(&directory, &owned).is_err());
        fs::rename(root.path().join("launch"), root.path().join("old")).unwrap();
        fs::write(root.path().join("launch"), b"original").unwrap();
        fs::set_permissions(
            root.path().join("launch"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        assert!(receipt.reopen(&directory).is_err());
        assert_eq!(fs::read(root.path().join("launch")).unwrap(), b"original");
    }

    #[test]
    fn receipts_are_bound_to_the_directory_and_refuse_links_or_permission_changes() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let directory = super::super::directories::open_private(root.path(), false).unwrap();
        let owned = directory.publish("launch", b"original", false).unwrap();
        let receipt = Receipt::record(&directory, &owned).unwrap();
        let other = tempfile::tempdir().unwrap();
        fs::set_permissions(other.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let other_directory = super::super::directories::open_private(other.path(), false).unwrap();
        assert!(receipt.reopen(&other_directory).is_err());
        fs::set_permissions(
            root.path().join("launch"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(receipt.reopen(&directory).is_err());
        fs::rename(root.path().join("launch"), root.path().join("old")).unwrap();
        symlink(root.path().join("old"), root.path().join("launch")).unwrap();
        assert!(receipt.reopen(&directory).is_err());
        assert_eq!(fs::read(root.path().join("old")).unwrap(), b"original");
    }

    #[test]
    fn malformed_unknown_version_and_oversized_receipts_refuse() {
        assert!(Receipt::decode(&vec![b' '; 16 * 1024 + 1]).is_err());
        assert!(Receipt::decode(b"{}").is_err());
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let directory = super::super::directories::open_private(root.path(), false).unwrap();
        let owned = directory.publish("launch", b"original", false).unwrap();
        let receipt = Receipt::record(&directory, &owned).unwrap();
        let mut value = serde_json::to_value(&receipt).unwrap();
        value["version"] = 2.into();
        assert!(Receipt::decode(&serde_json::to_vec(&value).unwrap()).is_err());
        value["version"] = 1.into();
        value["extra"] = true.into();
        assert!(Receipt::decode(&serde_json::to_vec(&value).unwrap()).is_err());
        value.as_object_mut().unwrap().remove("extra");
        value["name"] = "../escape".into();
        assert!(Receipt::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}
