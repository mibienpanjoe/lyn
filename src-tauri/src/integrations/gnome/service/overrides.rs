//! Inspect disk configuration which may not yet be loaded by systemd.

use super::{Result, ServiceRefusal};
use std::{
    ffi::OsString,
    fs, io,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
};

const DROP_INS: [&str; 4] = [
    "gnome-terminal-server.service.d",
    "gnome-terminal-.service.d",
    "gnome-.service.d",
    "service.d",
];
const MAX_PATHS: usize = 64;
const MAX_ENTRIES: usize = 4096;

#[derive(Debug, PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    mode: u32,
    uid: u32,
    links: u64,
    length: u64,
    changed: (i64, i64),
    modified: (i64, i64),
    resolved: Option<Box<Stamp>>,
}
impl From<&fs::Metadata> for Stamp {
    fn from(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
            uid: metadata.uid(),
            links: metadata.nlink(),
            length: metadata.len(),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            resolved: None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Snapshot(Vec<(PathBuf, Option<Stamp>, Vec<OsString>)>);

pub(super) struct OwnedDropIn {
    path: PathBuf,
    stamp: Stamp,
}
impl OwnedDropIn {
    pub(super) fn pin(path: &Path, metadata: &fs::Metadata) -> Result<Self> {
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o7777 != 0o600
            || metadata.nlink() != 1
        {
            return Err(ServiceRefusal::Changed);
        }
        Ok(Self {
            path: path.into(),
            stamp: Stamp::from(metadata),
        })
    }
}

impl Snapshot {
    pub(super) fn recheck(&self, paths: &[String], vendor: (u64, u64)) -> Result<()> {
        if self != &inspect(paths, vendor)? {
            return Err(ServiceRefusal::Changed);
        }
        Ok(())
    }

    pub(super) fn recheck_for(
        &self,
        paths: &[String],
        vendor: (u64, u64),
        owned: Option<&OwnedDropIn>,
    ) -> Result<()> {
        let current = inspect_for(paths, vendor, owned)?;
        if self != &current {
            #[cfg(test)]
            for (before, after) in self
                .0
                .iter()
                .zip(&current.0)
                .filter(|(before, after)| before != after)
            {
                eprintln!(
                    "GNOME service disk inventory changed: {} (names changed: {})",
                    before.0.display(),
                    before.2 != after.2
                );
            }
            return Err(ServiceRefusal::Changed);
        }
        Ok(())
    }
}

fn metadata(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(ServiceRefusal::Unavailable),
    }
}

fn trusted_directory_target(path: &Path) -> Result<fs::Metadata> {
    let target = fs::read_link(path).map_err(|_| ServiceRefusal::Unsupported)?;
    let target = if target.is_absolute() {
        target
    } else {
        path.parent()
            .ok_or(ServiceRefusal::Unsupported)?
            .join(target)
    };
    let canonical = fs::canonicalize(path).map_err(|_| ServiceRefusal::Unsupported)?;
    // Admit one protected system link only. Do not hide user-controlled
    // intermediate links behind canonicalize(), even when their current final
    // destination happens to be root-owned.
    for directory in path
        .ancestors()
        .skip(1)
        .chain(target.ancestors())
        .chain(canonical.ancestors())
    {
        let metadata = fs::symlink_metadata(directory).map_err(|_| ServiceRefusal::Unsupported)?;
        if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err(ServiceRefusal::Unsupported);
        }
    }
    fs::metadata(canonical).map_err(|_| ServiceRefusal::Unsupported)
}

fn directory(
    path: &Path,
    visited: &mut usize,
    trusted_root_link: bool,
) -> Result<(Option<Stamp>, Vec<OsString>)> {
    let Some(metadata) = metadata(path)? else {
        return Ok((None, Vec::new()));
    };
    let mut stamp = Stamp::from(&metadata);
    if metadata.is_symlink() && trusted_root_link && metadata.uid() == 0 {
        // e.g. /etc/xdg/systemd/user -> /etc/systemd/user. Only protected
        // system-directory indirection, never a user-controlled root alias.
        stamp.resolved = Some(Box::new(Stamp::from(&trusted_directory_target(path)?)));
    } else if !metadata.is_dir() {
        return Err(ServiceRefusal::Unsupported);
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(path).map_err(|_| ServiceRefusal::Unavailable)? {
        if *visited >= MAX_ENTRIES {
            return Err(ServiceRefusal::Unsupported);
        }
        *visited += 1;
        names.push(entry.map_err(|_| ServiceRefusal::Unavailable)?.file_name());
    }
    names.sort();
    Ok((Some(stamp), names))
}

pub(super) fn inspect(paths: &[String], vendor: (u64, u64)) -> Result<Snapshot> {
    inspect_for(paths, vendor, None)
}

pub(super) fn inspect_for(
    paths: &[String],
    vendor: (u64, u64),
    owned: Option<&OwnedDropIn>,
) -> Result<Snapshot> {
    if paths.is_empty() || paths.len() > MAX_PATHS {
        return Err(ServiceRefusal::Unsupported);
    }
    let mut snapshot = Snapshot(Vec::new());
    let mut visited = 0;
    let mut owned_seen = 0;
    for root in paths {
        let path = Path::new(root);
        if !path.is_absolute()
            || root.len() > 4095
            || (root.len() > 1 && root.ends_with('/'))
            || root.contains("//")
            || root.split('/').any(|part| matches!(part, "." | ".."))
        {
            return Err(ServiceRefusal::Unsupported);
        }
        let (stamp, names) = directory(path, &mut visited, true)?;
        // Catch aliases not yet reflected in the manager's Names property.
        for name in &names {
            if name.as_bytes().ends_with(b".service") && name != super::UNIT {
                let entry = path.join(name);
                if metadata(&entry)?.is_some_and(|metadata| metadata.is_symlink()) {
                    match fs::metadata(&entry) {
                        Ok(metadata) if (metadata.dev(), metadata.ino()) == vendor => {
                            return Err(ServiceRefusal::Unsupported);
                        }
                        Ok(_) => {}
                        Err(error)
                            if error.kind() == io::ErrorKind::NotFound
                                || error.raw_os_error() == Some(libc::ELOOP) => {}
                        Err(_) => return Err(ServiceRefusal::Unavailable),
                    }
                }
            }
        }
        snapshot.0.push((path.to_owned(), stamp, names));
        let unit = path.join(super::UNIT);
        let unit_metadata = metadata(&unit)?;
        if let Some(metadata) = &unit_metadata {
            if !["/usr/lib/systemd/user", "/lib/systemd/user"].contains(&root.as_str())
                || !metadata.is_file()
                || metadata.uid() != 0
                || metadata.mode() & 0o022 != 0
                || (metadata.dev(), metadata.ino()) != vendor
            {
                return Err(ServiceRefusal::Unsupported);
            }
        }
        snapshot
            .0
            .push((unit, unit_metadata.as_ref().map(Stamp::from), Vec::new()));
        for suffix in DROP_INS {
            let drop_in = path.join(suffix);
            let (stamp, names) = directory(&drop_in, &mut visited, false)?;
            for name in names
                .iter()
                .filter(|name| name.as_bytes().ends_with(b".conf"))
            {
                let entry = drop_in.join(name);
                let allowed = owned
                    .filter(|owned| suffix == DROP_INS[0] && owned.path == entry)
                    .ok_or(ServiceRefusal::Unsupported)?;
                let current = metadata(&entry)?.ok_or(ServiceRefusal::Changed)?;
                let current = Stamp::from(&current);
                if current != allowed.stamp {
                    return Err(ServiceRefusal::Changed);
                }
                owned_seen += 1;
                snapshot.0.push((entry, Some(current), Vec::new()));
            }
            snapshot.0.push((drop_in, stamp, names));
        }
    }
    if owned.is_some() && owned_seen != 1 {
        return Err(ServiceRefusal::Unsupported);
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn only_the_pinned_owned_drop_in_is_allowed_and_fenced() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join(DROP_INS[0]);
        fs::create_dir(&directory).unwrap();
        let path = directory.join("90-lyn-native.conf");
        fs::write(&path, b"owned override").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let owned = OwnedDropIn::pin(&path, &fs::metadata(&path).unwrap()).unwrap();
        let paths = vec![root.path().to_str().unwrap().to_owned()];
        assert!(inspect_for(&paths, (0, 0), Some(&owned)).is_ok());
        assert_eq!(inspect(&paths, (0, 0)), Err(ServiceRefusal::Unsupported));
        fs::write(directory.join("foreign.conf"), b"keep").unwrap();
        assert_eq!(
            inspect_for(&paths, (0, 0), Some(&owned)),
            Err(ServiceRefusal::Unsupported)
        );
        fs::remove_file(directory.join("foreign.conf")).unwrap();
        fs::write(&path, b"edited override").unwrap();
        assert_eq!(
            inspect_for(&paths, (0, 0), Some(&owned)),
            Err(ServiceRefusal::Changed)
        );
    }

    #[test]
    fn unreloaded_prefix_override_is_refused_and_preserved() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("gnome-.service.d");
        fs::create_dir(&directory).unwrap();
        let file = directory.join("custom.conf");
        let bytes = b"[Service]\nExecStart=\nExecStart=/custom/server\n";
        fs::write(&file, bytes).unwrap();
        assert_eq!(
            inspect(&[root.path().to_str().unwrap().to_owned()], (0, 0)),
            Err(ServiceRefusal::Unsupported)
        );
        assert_eq!(fs::read(&file).unwrap(), bytes);
    }

    #[test]
    fn exact_prefix_and_type_wide_drop_ins_all_refuse_without_changes() {
        for suffix in DROP_INS {
            let root = tempfile::tempdir().unwrap();
            let directory = root.path().join(suffix);
            fs::create_dir(&directory).unwrap();
            let file = directory.join("unloaded.conf");
            fs::write(&file, b"user configuration").unwrap();
            assert_eq!(
                inspect(&[root.path().to_str().unwrap().to_owned()], (0, 0)),
                Err(ServiceRefusal::Unsupported)
            );
            assert_eq!(fs::read(&file).unwrap(), b"user configuration");
        }
    }

    #[test]
    fn unreloaded_alias_to_the_vendor_inode_is_not_adopted() {
        let root = tempfile::tempdir().unwrap();
        let units = root.path().join("units");
        fs::create_dir(&units).unwrap();
        let vendor = root.path().join("vendor");
        fs::write(&vendor, b"reference").unwrap();
        let identity = fs::metadata(&vendor).unwrap();
        let alias = units.join("custom.service");
        std::os::unix::fs::symlink(&vendor, &alias).unwrap();
        assert_eq!(
            inspect(
                &[units.to_str().unwrap().to_owned()],
                (identity.dev(), identity.ino())
            ),
            Err(ServiceRefusal::Unsupported)
        );
        assert_eq!(fs::read_link(&alias).unwrap(), vendor);
        assert_eq!(fs::read(&vendor).unwrap(), b"reference");
    }

    #[test]
    fn masks_replacements_and_unknown_directory_links_refuse() {
        for kind in 0..3 {
            let root = tempfile::tempdir().unwrap();
            let entry = root.path().join(if kind == 2 {
                DROP_INS[0]
            } else {
                super::super::UNIT
            });
            if kind == 0 {
                std::os::unix::fs::symlink("/dev/null", &entry).unwrap();
            } else if kind == 1 {
                fs::write(&entry, b"unknown unit").unwrap();
            } else {
                std::os::unix::fs::symlink(root.path(), &entry).unwrap();
            }
            let before = fs::symlink_metadata(&entry).unwrap();
            assert_eq!(
                inspect(&[root.path().to_str().unwrap().to_owned()], (0, 0)),
                Err(ServiceRefusal::Unsupported)
            );
            assert_eq!(
                Stamp::from(&fs::symlink_metadata(&entry).unwrap()),
                Stamp::from(&before)
            );
        }
    }

    #[test]
    fn missing_or_empty_paths_recheck_but_changed_disk_inventory_refuses() {
        let root = tempfile::tempdir().unwrap();
        let paths = vec![
            root.path().join("missing").to_str().unwrap().to_owned(),
            root.path().to_str().unwrap().to_owned(),
        ];
        let snapshot = inspect(&paths, (0, 0)).unwrap();
        snapshot.recheck(&paths, (0, 0)).unwrap();
        fs::write(root.path().join("readme.txt"), b"not a drop-in").unwrap();
        assert_eq!(
            snapshot.recheck(&paths, (0, 0)),
            Err(ServiceRefusal::Changed)
        );
        let snapshot = inspect(&paths, (0, 0)).unwrap();
        fs::create_dir(root.path().join("missing")).unwrap();
        assert_eq!(
            snapshot.recheck(&paths, (0, 0)),
            Err(ServiceRefusal::Changed)
        );
    }

    #[test]
    fn paths_and_total_directory_entries_are_bounded() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().to_str().unwrap().to_owned();
        for paths in [
            vec![],
            vec![path.clone(); MAX_PATHS + 1],
            vec!["relative".to_owned()],
            vec![format!("{path}/../unsafe")],
            vec![format!("{path}/")],
            vec![format!("{path}//nested")],
        ] {
            assert_eq!(inspect(&paths, (0, 0)), Err(ServiceRefusal::Unsupported));
        }
        for index in 0..MAX_ENTRIES {
            fs::write(root.path().join(format!("{index}.txt")), b"").unwrap();
        }
        assert!(inspect(&[path.clone()], (0, 0)).is_ok());
        fs::write(root.path().join("excess.txt"), b"").unwrap();
        assert_eq!(inspect(&[path], (0, 0)), Err(ServiceRefusal::Unsupported));
    }

    #[test]
    fn trailing_slash_cannot_bypass_refusal_of_a_directory_symlink() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        for path in [
            link.to_str().unwrap().to_owned(),
            format!("{}/", link.display()),
        ] {
            assert_eq!(inspect(&[path], (0, 0)), Err(ServiceRefusal::Unsupported));
        }
        assert_eq!(fs::read_link(link).unwrap(), target);
    }
}
