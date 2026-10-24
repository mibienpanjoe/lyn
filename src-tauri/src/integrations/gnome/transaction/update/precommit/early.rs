//! Inverse replay for recorded exchanges before explicit update commitment.

use super::super::super::{DROP_IN, LAUNCHER, MANIFEST, Manifest, Place, refused};
use super::{
    COMPENSATION_PENDING, EXCHANGE_NAMES, ExchangeRecord, PRECOMMIT_PENDING, PreparedUpdate,
};
use crate::integrations::gnome::{
    lock::InstallationLock,
    managed::{ManagedDirectory, OwnedFile, PublishFailure},
    ownership::Receipt,
};
use std::io;

struct Slot {
    original: OwnedFile,
    replacement: Option<OwnedFile>,
    swapped: bool,
}

fn optional_state(directory: &ManagedDirectory, name: &str) -> io::Result<Option<OwnedFile>> {
    match directory.read_state(name) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn load_slot(
    directory: &ManagedDirectory,
    previous: &Receipt,
    exchange: Option<&ExchangeRecord>,
    restored: Option<&Receipt>,
    destination: &str,
) -> io::Result<Slot> {
    if exchange.is_none() {
        // Without a recorded permutation, require the original timestamp or a
        // strict fresh receipt from the proven canonical restored manifest.
        let original = restored.unwrap_or(previous).reopen(directory)?;
        return Ok(Slot {
            original,
            replacement: None,
            swapped: false,
        });
    }
    if let Ok(original) = previous.reopen_exchange_slot(directory, destination) {
        let replacement = if let Some(exchange) = exchange {
            match exchange
                .candidate
                .reopen_exchange_slot(directory, exchange.candidate.filename())
            {
                Ok(file) => Some(file),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        return Ok(Slot {
            original,
            replacement,
            swapped: false,
        });
    }
    let exchange = exchange.ok_or_else(refused)?;
    let original = previous.reopen_exchange_slot(directory, exchange.candidate.filename())?;
    let replacement = exchange
        .candidate
        .reopen_exchange_slot(directory, destination)?;
    Ok(Slot {
        original,
        replacement: Some(replacement),
        swapped: true,
    })
}

fn canonical(
    state: &ManagedDirectory,
    target: &ManagedDirectory,
    launcher: &OwnedFile,
    drop_in: &OwnedFile,
) -> io::Result<Vec<u8>> {
    serde_json::to_vec(&Manifest {
        version: 1,
        launcher: Receipt::record(state, launcher)?,
        drop_in: Receipt::record(target, drop_in)?,
    })
    .map_err(io::Error::other)
}

fn restored_receipts(
    state: &ManagedDirectory,
    target: &ManagedDirectory,
    prepared: &PreparedUpdate,
    current: Option<&OwnedFile>,
    manifest_exchange: Option<&ExchangeRecord>,
) -> io::Result<Option<[Receipt; 2]>> {
    let Some(current) = current else {
        return Ok(None);
    };
    let current_receipt = Receipt::record(state, current)?;
    if prepared.previous_manifest.same_inode(&current_receipt) {
        if manifest_exchange.is_some() {
            prepared
                .previous_manifest
                .reopen_exchange_slot(state, MANIFEST)?;
        } else {
            prepared.previous_manifest.reopen(state)?;
        }
        return Ok(None);
    }
    let Ok(manifest) = serde_json::from_slice::<Manifest>(&current.content) else {
        return Ok(None);
    };
    if manifest.version != 1
        || !manifest
            .launcher
            .same_payload_identity(&prepared.previous_launcher)
        || !manifest
            .drop_in
            .same_payload_identity(&prepared.previous_drop_in)
    {
        return Ok(None);
    }
    let launcher = manifest.launcher.reopen(state)?;
    let drop_in = manifest.drop_in.reopen(target)?;
    if current.content != canonical(state, target, &launcher, &drop_in)? {
        return Ok(None);
    }
    Ok(Some([manifest.launcher, manifest.drop_in]))
}

fn manifest_nodes(
    state: &ManagedDirectory,
    prepared: &PreparedUpdate,
    exchange: Option<&ExchangeRecord>,
    compensation: Option<&ExchangeRecord>,
    already_canonical: bool,
    undo_name: &str,
) -> io::Result<(Option<OwnedFile>, Option<OwnedFile>, Option<OwnedFile>)> {
    let mut original = None;
    let mut replacement = None;
    let mut restored = None;
    let mut names = vec![MANIFEST, undo_name];
    if let Some(exchange) = exchange {
        names.push(exchange.candidate.filename());
    }
    if let Some(compensation) = compensation {
        names.push(compensation.candidate.filename());
    }
    for name in names {
        if name == MANIFEST && already_canonical {
            continue;
        }
        if optional_state(state, name)?.is_none() {
            continue;
        }
        let old = if name == MANIFEST && exchange.is_none() {
            prepared.previous_manifest.reopen(state)
        } else {
            prepared.previous_manifest.reopen_exchange_slot(state, name)
        };
        if let Ok(file) = old {
            if original.replace(file).is_some() {
                return Err(refused());
            }
        } else if name != undo_name {
            if let Some(file) = exchange
                .and_then(|exchange| exchange.candidate.reopen_exchange_slot(state, name).ok())
            {
                if replacement.replace(file).is_some() {
                    return Err(refused());
                }
            } else {
                let file = compensation
                    .ok_or_else(refused)?
                    .candidate
                    .reopen_exchange_slot(state, name)?;
                if restored.replace(file).is_some() {
                    return Err(refused());
                }
            }
        } else {
            return Err(refused());
        }
    }
    if !already_canonical && original.is_none() {
        return Err(refused());
    }
    Ok((original, replacement, restored))
}

pub(in crate::integrations::gnome) struct UncommittedUpdateSnapshot {
    header: OwnedFile,
    exchanges: Vec<Option<(OwnedFile, ExchangeRecord)>>,
    compensation: Option<(OwnedFile, ExchangeRecord)>,
    current: Option<OwnedFile>,
    launcher: Slot,
    drop_in: Slot,
    already_canonical: bool,
    undo_name: String,
    original_manifest: Option<OwnedFile>,
    replacement_manifest: Option<OwnedFile>,
    compensation_manifest: Option<OwnedFile>,
    absent: Vec<(Place, String)>,
}

fn no_competing_intent(state: &ManagedDirectory) -> io::Result<()> {
    use super::super::super::{recovery, removal};
    for name in [
        recovery::PENDING_INSTALL,
        removal::PENDING_REMOVAL,
        removal::RETIRED_REMOVAL,
        super::super::CLEANUP_PENDING,
    ] {
        state.require_absent(name)?;
    }
    Ok(())
}

impl UncommittedUpdateSnapshot {
    pub(in crate::integrations::gnome) fn open(
        state: &ManagedDirectory,
        target: &ManagedDirectory,
    ) -> io::Result<Self> {
        no_competing_intent(state)?;
        let header = state.read_state(PRECOMMIT_PENDING)?;
        let prepared: PreparedUpdate =
            serde_json::from_slice(&header.content).map_err(|_| refused())?;
        prepared.validate()?;
        let mut exchanges = Vec::new();
        for (role, name) in EXCHANGE_NAMES.into_iter().enumerate() {
            let entry = if let Some(file) = optional_state(state, name)? {
                let body: ExchangeRecord =
                    serde_json::from_slice(&file.content).map_err(|_| refused())?;
                let expected = match role {
                    0 => &prepared.previous_launcher,
                    1 => &prepared.previous_drop_in,
                    2 => &prepared.previous_manifest,
                    _ => return Err(refused()),
                };
                let mode = if role == 0 { 0o700 } else { 0o600 };
                body.candidate.validate()?;
                if body.version != 1
                    || body.role != role as u8
                    || body.previous != *expected
                    || !body.precommit.is_for(PRECOMMIT_PENDING, 0o600)
                    || !super::super::cleanup::candidate(&body.candidate, mode)
                {
                    return Err(refused());
                }
                let _authority = body.precommit.reopen(state)?;
                Some((file, body))
            } else {
                None
            };
            exchanges.push(entry);
        }
        let current = optional_state(state, MANIFEST)?;
        let manifest_exchange = exchanges[2].as_ref().map(|entry| &entry.1);
        let compensation = if let Some(file) = optional_state(state, COMPENSATION_PENDING)? {
            let body: ExchangeRecord =
                serde_json::from_slice(&file.content).map_err(|_| refused())?;
            super::compensation::validate(state, &body, &prepared, manifest_exchange)?;
            Some((file, body))
        } else {
            None
        };
        if let Some((_, body)) = &compensation {
            // With compensation authority, content alone cannot authorize an
            // unknown active inode. Both permissible active identities are logged.
            if body
                .candidate
                .reopen_exchange_slot(state, MANIFEST)
                .is_err()
            {
                body.previous.reopen_exchange_slot(state, MANIFEST)?;
            }
        }
        let restored = restored_receipts(
            state,
            target,
            &prepared,
            current.as_ref(),
            manifest_exchange,
        )?;
        let launcher = load_slot(
            state,
            &prepared.previous_launcher,
            exchanges[0].as_ref().map(|entry| &entry.1),
            restored.as_ref().map(|receipts| &receipts[0]),
            LAUNCHER,
        )?;
        let drop_in = load_slot(
            target,
            &prepared.previous_drop_in,
            exchanges[1].as_ref().map(|entry| &entry.1),
            restored.as_ref().map(|receipts| &receipts[1]),
            DROP_IN,
        )?;
        let mut already_canonical = if !launcher.swapped && !drop_in.swapped {
            if let Some(file) = &current {
                file.content == canonical(state, target, &launcher.original, &drop_in.original)?
            } else {
                false
            }
        } else {
            false
        };
        if let Some((_, body)) = &compensation {
            if launcher.swapped || drop_in.swapped {
                return Err(refused());
            }
            already_canonical = body.candidate.reopen_exchange_slot(state, MANIFEST).is_ok();
            if already_canonical
                && current.as_ref().ok_or_else(refused)?.content
                    != canonical(state, target, &launcher.original, &drop_in.original)?
            {
                return Err(refused());
            }
        }
        let undo_name = format!(".lyn-undo-{}-manifest", prepared.nonce);
        let (original_manifest, replacement_manifest, compensation_manifest) = manifest_nodes(
            state,
            &prepared,
            manifest_exchange,
            compensation.as_ref().map(|entry| &entry.1),
            already_canonical,
            &undo_name,
        )?;
        state.verify(&header)?;
        for (file, _) in exchanges.iter().flatten() {
            state.verify(file)?;
        }
        if let Some((file, _)) = &compensation {
            state.verify(file)?;
        }
        if compensation.is_some() && !already_canonical {
            let candidate = compensation_manifest.as_ref().ok_or_else(refused)?;
            if candidate.content != canonical(state, target, &launcher.original, &drop_in.original)?
            {
                return Err(refused());
            }
        }

        let mut result = Self {
            header,
            exchanges,
            compensation,
            current,
            launcher,
            drop_in,
            already_canonical,
            undo_name,
            original_manifest,
            replacement_manifest,
            compensation_manifest,
            absent: Vec::new(),
        };
        let present: Vec<_> = result
            .files()
            .into_iter()
            .map(|(place, file)| {
                file.name
                    .to_str()
                    .map(|name| (place, name.to_owned()))
                    .map_err(|_| refused())
            })
            .collect::<io::Result<_>>()?;
        for (place, name) in result.names() {
            if !present.iter().any(|(p, n)| *p == place && n == &name) {
                directory(place, state, target).require_absent(&name)?;
                result.absent.push((place, name));
            }
        }
        result.recheck(state, target)?;
        Ok(result)
    }

    fn files(&self) -> Vec<(Place, &OwnedFile)> {
        use super::super::super::Place::{DropIns, State};
        let mut files = vec![
            (State, &self.header),
            (State, &self.launcher.original),
            (DropIns, &self.drop_in.original),
        ];
        for (file, _) in self.exchanges.iter().flatten() {
            files.push((State, file));
        }
        if let Some((file, _)) = &self.compensation {
            files.push((State, file));
        }
        if let Some(file) = &self.launcher.replacement {
            files.push((State, file));
        }
        if let Some(file) = &self.drop_in.replacement {
            files.push((DropIns, file));
        }
        for file in [
            &self.current,
            &self.original_manifest,
            &self.replacement_manifest,
            &self.compensation_manifest,
        ]
        .into_iter()
        .flatten()
        {
            files.push((State, file));
        }
        files
    }

    fn names(&self) -> Vec<(Place, String)> {
        use super::super::super::Place::{DropIns, State};
        let mut names = vec![
            (State, PRECOMMIT_PENDING.into()),
            (State, COMPENSATION_PENDING.into()),
            (State, LAUNCHER.into()),
            (State, MANIFEST.into()),
            (State, self.undo_name.clone()),
            (DropIns, DROP_IN.into()),
        ];
        names.extend(EXCHANGE_NAMES.into_iter().map(|name| (State, name.into())));
        for (role, entry) in self.exchanges.iter().enumerate() {
            if let Some((_, body)) = entry {
                names.push((
                    if role == 1 { DropIns } else { State },
                    body.candidate.filename().into(),
                ));
            }
        }
        if let Some((_, body)) = &self.compensation {
            names.push((State, body.candidate.filename().into()));
        }
        names
    }

    pub(in crate::integrations::gnome) fn contents(&self) -> (&[u8], &[u8]) {
        (
            &self.launcher.original.content,
            &self.drop_in.original.content,
        )
    }

    pub(in crate::integrations::gnome) fn replacement_contents(&self) -> Option<(&[u8], &[u8])> {
        if self.launcher.replacement.is_none() && self.drop_in.replacement.is_none() {
            return None;
        }
        Some((
            &self
                .launcher
                .replacement
                .as_ref()
                .unwrap_or(&self.launcher.original)
                .content,
            &self
                .drop_in
                .replacement
                .as_ref()
                .unwrap_or(&self.drop_in.original)
                .content,
        ))
    }

    pub(in crate::integrations::gnome) fn drop_in_metadata(
        &self,
    ) -> io::Result<Option<std::fs::Metadata>> {
        let file = if self.drop_in.swapped {
            self.drop_in.replacement.as_ref().ok_or_else(refused)?
        } else {
            &self.drop_in.original
        };
        file.file.metadata().map(Some)
    }

    pub(in crate::integrations::gnome) fn recheck(
        &self,
        state: &ManagedDirectory,
        target: &ManagedDirectory,
    ) -> io::Result<()> {
        no_competing_intent(state)?;
        for (place, file) in self.files() {
            directory(place, state, target).verify(file)?;
        }
        for (place, name) in &self.absent {
            directory(*place, state, target).require_absent(name)?;
        }
        no_competing_intent(state)
    }

    pub(in crate::integrations::gnome::transaction) fn finish(
        self,
        state: &ManagedDirectory,
        target: &ManagedDirectory,
        lock: &InstallationLock<'_>,
    ) -> io::Result<()> {
        lock.check()?;
        self.recheck(state, target)?;
        let Self {
            header,
            exchanges,
            compensation,
            mut launcher,
            mut drop_in,
            mut already_canonical,
            undo_name,
            mut original_manifest,
            mut replacement_manifest,
            mut compensation_manifest,
            ..
        } = self;
        let manifest_exchange = exchanges[2].as_ref().map(|entry| &entry.1);
        state.sync_directory()?;
        if compensation.is_some() && !already_canonical {
            let previous = if original_manifest
                .as_ref()
                .is_some_and(|file| file.name.to_bytes() == MANIFEST.as_bytes())
            {
                original_manifest.as_mut().ok_or_else(refused)?
            } else {
                replacement_manifest.as_mut().ok_or_else(refused)?
            };
            lock.check()?;
            state
                .exchange(
                    previous,
                    compensation_manifest.as_mut().ok_or_else(refused)?,
                )?
                .durability?;
            // The recorded candidate is now the active canonical manifest; no
            // unjournaled manifest publication is needed during recovery.
            compensation_manifest = None;
            already_canonical = true;
        }
        for (directory, slot) in [(target, &mut drop_in), (state, &mut launcher)] {
            lock.check()?;
            if slot.swapped {
                directory
                    .exchange(
                        &mut slot.original,
                        slot.replacement.as_mut().ok_or_else(refused)?,
                    )?
                    .durability?;
                slot.swapped = false;
            }
        }
        let bytes = canonical(state, target, &launcher.original, &drop_in.original)?;
        if !already_canonical {
            if let Some(new) = &mut replacement_manifest {
                if new.name.to_bytes() == MANIFEST.as_bytes() {
                    lock.check()?;
                    let old = original_manifest.as_mut().ok_or_else(refused)?;
                    let candidate_name =
                        manifest_exchange.ok_or_else(refused)?.candidate.filename();
                    if old.name.to_bytes() == candidate_name.as_bytes() {
                        state.exchange(old, new)?.durability?;
                    } else {
                        state.move_to_unoccupied(new, candidate_name)?.durability?;
                    }
                }
            }
            if let Some(file) = &mut original_manifest {
                lock.check()?;
                if file.name.to_bytes() != undo_name.as_bytes() {
                    state.move_to_unoccupied(file, &undo_name)?.durability?;
                }
            }
            state.require_absent(MANIFEST)?;
            match state.publish(MANIFEST, &bytes, false) {
                Ok(_) => {}
                Err(
                    PublishFailure::Published { error, .. } | PublishFailure::Unpublished(error),
                ) => {
                    return Err(error);
                }
                Err(PublishFailure::Staged {
                    error,
                    cleanup_error,
                    ..
                }) => {
                    return Err(io::Error::other(format!(
                        "{error}; staging cleanup failed: {cleanup_error}"
                    )));
                }
            }
        }
        state.sync_directory()?;
        target.sync_directory()?;
        let mut copies = Vec::new();
        if let Some(file) = compensation_manifest {
            if file.name.to_bytes() == MANIFEST.as_bytes() {
                return Err(refused());
            }
            copies.push((state, file));
        }
        if let Some(file) = launcher.replacement {
            copies.push((state, file));
        }
        if let Some(file) = drop_in.replacement {
            copies.push((target, file));
        }
        if let Some(file) = original_manifest {
            if file.name.to_bytes() == MANIFEST.as_bytes() {
                return Err(refused());
            }
            copies.push((state, file));
        }
        if let Some(file) = replacement_manifest {
            if file.name.to_bytes() == MANIFEST.as_bytes() {
                return Err(refused());
            }
            copies.push((state, file));
        }
        for (directory, file) in &copies {
            directory.verify(file)?;
        }
        for (directory, file) in copies {
            lock.check()?;
            directory.remove(&file)?;
        }
        state.sync_directory()?;
        target.sync_directory()?;
        if let Some((file, _)) = compensation {
            lock.check()?;
            state.remove(&file)?;
        }
        for (file, _) in exchanges.into_iter().flatten().rev() {
            lock.check()?;
            state.remove(&file)?;
        }
        lock.check()?;
        state.remove(&header)
    }
}

fn directory<'a>(
    place: Place,
    state: &'a ManagedDirectory,
    target: &'a ManagedDirectory,
) -> &'a ManagedDirectory {
    match place {
        Place::State => state,
        Place::DropIns => target,
    }
}

pub(in crate::integrations::gnome::transaction) fn recover_early(
    state: &ManagedDirectory,
    target: &ManagedDirectory,
    lock: &InstallationLock<'_>,
) -> io::Result<()> {
    lock.check()?;
    if optional_state(state, PRECOMMIT_PENDING)?.is_none() {
        state.require_absent(COMPENSATION_PENDING)?;
        for name in EXCHANGE_NAMES {
            state.require_absent(name)?;
        }
        return state.sync_directory();
    }
    UncommittedUpdateSnapshot::open(state, target)?.finish(state, target, lock)
}

#[cfg(test)]
mod tests {
    use super::super::super::{ActivationTransaction, PendingUpdate, Phase, Place};
    use crate::integrations::gnome::transaction::{DROP_IN, LAUNCHER, tests::fixture};
    use std::fs;

    #[test]
    fn uncommitted_snapshot_reads_original_roles_and_fences_a_new_exchange_intent() {
        use super::*;
        let (root, state, target, initial) = fixture();
        let old_launcher = initial.launcher.clone();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        transaction.stop_after = Some((6, false));
        assert!(
            transaction
                .update(
                    crate::integrations::gnome::render_activation(
                        &root.path().join("v2.so"),
                        &root.path().join("state").join(LAUNCHER)
                    )
                    .unwrap()
                )
                .is_err()
        );
        drop(transaction);
        let before = fs::read_dir(root.path().join("state")).unwrap().count();
        let snapshot = UncommittedUpdateSnapshot::open(&state, &target).unwrap();
        assert_eq!(snapshot.contents().0, old_launcher.as_bytes());
        snapshot.recheck(&state, &target).unwrap();
        assert_eq!(
            fs::read_dir(root.path().join("state")).unwrap().count(),
            before
        );
        fs::write(
            root.path().join("state").join(EXCHANGE_NAMES[1]),
            b"unexpected",
        )
        .unwrap();
        assert!(snapshot.recheck(&state, &target).is_err());
        assert!(root.path().join("state").join(PRECOMMIT_PENDING).is_file());
    }

    #[test]
    fn first_exchange_can_be_restored_from_disk_without_accepting_the_new_version() {
        let (root, state, target, initial) = fixture();
        let old_launcher = initial.launcher.clone();
        let old_drop = initial.drop_in.clone();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        transaction.update = Some(PendingUpdate::default());
        transaction.phase = Phase::Updating;
        transaction.prepare_precommit().unwrap();
        transaction
            .replace_role(0, Place::State, b"new launcher", true)
            .unwrap();
        drop(transaction);
        let mut restored = ActivationTransaction::resume(&state, &target).unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
            old_launcher
        );
        assert_eq!(
            fs::read_to_string(root.path().join("drop-ins").join(DROP_IN)).unwrap(),
            old_drop
        );
        restored.rollback().unwrap();
    }

    #[test]
    fn abrupt_early_child() {
        use std::{os::unix::fs::MetadataExt, path::Path};
        let Some(root) = std::env::var_os("LYN_PRIVATE_INSTALL_RECOVERY_ROOT") else {
            return;
        };
        let root = Path::new(&root);
        let metadata = fs::symlink_metadata(root).unwrap();
        assert!(
            metadata.is_dir()
                && metadata.uid() == unsafe { libc::geteuid() }
                && metadata.mode() & 0o7777 == 0o700
        );
        assert_eq!(root.parent().unwrap(), std::env::temp_dir());
        assert!(
            root.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("lyn-install-recovery-")
        );
        let checkpoint: u8 = std::env::var("LYN_PRIVATE_INSTALL_RECOVERY_POINT")
            .unwrap()
            .parse()
            .unwrap();
        assert!((5..=8).contains(&checkpoint));
        let state_path = root.join("state");
        let state =
            crate::integrations::gnome::directories::open_private(&state_path, false).unwrap();
        let target =
            crate::integrations::gnome::directories::open_drop_ins(&root.join("drop-ins"), false)
                .unwrap();
        let initial = crate::integrations::gnome::render_activation(
            &root.join("native.so"),
            &state_path.join(LAUNCHER),
        )
        .unwrap();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = crate::integrations::gnome::render_activation(
            &root.join("native-v2.so"),
            &state_path.join(LAUNCHER),
        )
        .unwrap();
        transaction.stop_after = Some((checkpoint, true));
        transaction.update(updated).unwrap();
        panic!("early checkpoint did not exit");
    }

    #[test]
    fn four_precommit_process_exits_restore_exact_old_files_and_keep_user_settings() {
        use std::os::unix::fs::PermissionsExt;
        for checkpoint in 5..=8 {
            let root = tempfile::Builder::new()
                .prefix("lyn-install-recovery-")
                .tempdir()
                .unwrap();
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
            let state_path = root.path().join("state");
            let target_path = root.path().join("drop-ins");
            let state =
                crate::integrations::gnome::directories::open_private(&state_path, true).unwrap();
            drop(
                crate::integrations::gnome::directories::open_drop_ins(&target_path, true).unwrap(),
            );
            fs::set_permissions(&target_path, fs::Permissions::from_mode(0o755)).unwrap();
            let target =
                crate::integrations::gnome::directories::open_drop_ins(&target_path, false)
                    .unwrap();
            fs::write(target_path.join("user.conf"), b"untouched").unwrap();
            crate::integrations::gnome::transaction::recovery::tests::abrupt_exit(
                root.path(),
                checkpoint,
                "integrations::gnome::transaction::update::precommit::early::tests::abrupt_early_child",
            );
            let mut restored = ActivationTransaction::resume(&state, &target).unwrap();
            let old = crate::integrations::gnome::render_activation(
                &root.path().join("native.so"),
                &state_path.join(LAUNCHER),
            )
            .unwrap();
            assert_eq!(
                fs::read_to_string(state_path.join(LAUNCHER)).unwrap(),
                old.launcher
            );
            assert_eq!(
                fs::read_to_string(target_path.join(DROP_IN)).unwrap(),
                old.drop_in
            );
            assert_eq!(
                fs::read(target_path.join("user.conf")).unwrap(),
                b"untouched"
            );
            assert_eq!(fs::read_dir(&state_path).unwrap().count(), 3);
            assert_eq!(fs::read_dir(&target_path).unwrap().count(), 2);
            assert_eq!(
                fs::metadata(&target_path).unwrap().permissions().mode() & 0o7777,
                0o755
            );
            restored.rollback().unwrap();
        }
    }

    #[test]
    fn edited_other_role_refuses_before_restoring_the_launcher() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        transaction.update = Some(PendingUpdate::default());
        transaction.phase = Phase::Updating;
        transaction.prepare_precommit().unwrap();
        transaction
            .replace_role(0, Place::State, b"new launcher", true)
            .unwrap();
        drop(transaction);
        fs::write(root.path().join("drop-ins").join(DROP_IN), b"user override").unwrap();
        let before_state = fs::read_dir(root.path().join("state")).unwrap().count();
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        assert_eq!(
            fs::read(root.path().join("state").join(LAUNCHER)).unwrap(),
            b"new launcher"
        );
        assert_eq!(
            fs::read(root.path().join("drop-ins").join(DROP_IN)).unwrap(),
            b"user override"
        );
        assert_eq!(
            fs::read_dir(root.path().join("state")).unwrap().count(),
            before_state
        );
    }

    #[test]
    fn inverse_sync_failures_retry_both_renamed_slots_and_saved_manifest() {
        for successes in [2, 3] {
            let (root, state, target, initial) = fixture();
            let old_launcher = initial.launcher.clone();
            let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
            transaction.apply().unwrap();
            transaction.update = Some(PendingUpdate::default());
            transaction.phase = Phase::Updating;
            transaction.prepare_precommit().unwrap();
            transaction
                .replace_role(0, Place::State, b"new launcher", true)
                .unwrap();
            drop(transaction);
            state.fail_directory_sync_after(successes);
            assert!(ActivationTransaction::resume(&state, &target).is_err());
            assert_eq!(
                fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
                old_launcher
            );
            if successes == 3 {
                assert!(
                    !root
                        .path()
                        .join("state")
                        .join(super::super::super::super::MANIFEST)
                        .exists()
                );
            }
            state.fail_directory_sync(false);
            let mut restored = ActivationTransaction::resume(&state, &target).unwrap();
            assert_eq!(
                fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
                old_launcher
            );
            assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 3);
            restored.rollback().unwrap();
        }
    }

    #[test]
    fn header_without_exchange_intent_refuses_an_identical_original_rewrite() {
        let (root, state, target, initial) = fixture();
        let old_launcher = initial.launcher.clone();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        transaction.update = Some(PendingUpdate::default());
        transaction.phase = Phase::Updating;
        transaction.prepare_precommit().unwrap();
        drop(transaction);
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(root.path().join("state").join(LAUNCHER), old_launcher).unwrap();
        assert!(ActivationTransaction::resume(&state, &target).is_err());
    }

    #[test]
    fn original_manifest_identical_rewrite_refuses_before_inverse_exchange() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        transaction.update = Some(PendingUpdate::default());
        transaction.phase = Phase::Updating;
        transaction.prepare_precommit().unwrap();
        transaction
            .replace_role(0, Place::State, b"new launcher", true)
            .unwrap();
        drop(transaction);
        let path = root
            .path()
            .join("state")
            .join(super::super::super::super::MANIFEST);
        let bytes = fs::read(&path).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(path, bytes).unwrap();
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        assert_eq!(
            fs::read(root.path().join("state").join(LAUNCHER)).unwrap(),
            b"new launcher"
        );
    }

    #[test]
    fn restored_canonical_manifest_proves_retry_after_exchange_intent_retirement() {
        let (root, state, target, initial) = fixture();
        let old_launcher = initial.launcher.clone();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        transaction.update = Some(PendingUpdate::default());
        transaction.phase = Phase::Updating;
        transaction.prepare_precommit().unwrap();
        transaction
            .replace_role(0, Place::State, b"new launcher", true)
            .unwrap();
        drop(transaction);
        // Reach unlink of the single exchange intent, after all copies were
        // removed and the old canonical manifest was durably published.
        state.fail_directory_sync_after(9);
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        assert!(
            !root
                .path()
                .join("state")
                .join(super::super::EXCHANGE_NAMES[0])
                .exists()
        );
        assert!(
            root.path()
                .join("state")
                .join(super::super::PRECOMMIT_PENDING)
                .exists()
        );
        state.fail_directory_sync(false);
        let mut restored = ActivationTransaction::resume(&state, &target).unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
            old_launcher
        );
        assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 3);
        restored.rollback().unwrap();
    }

    #[test]
    fn final_manifest_inverse_and_undo_sync_failures_can_resume_without_partial_commit() {
        for successes in [3, 4] {
            let (root, state, target, initial) = fixture();
            let old_launcher = initial.launcher.clone();
            let old_drop = initial.drop_in.clone();
            let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
            transaction.apply().unwrap();
            let updated = crate::integrations::gnome::render_activation(
                &root.path().join("native-v2.so"),
                &root.path().join("state").join(LAUNCHER),
            )
            .unwrap();
            transaction.update(updated).unwrap();
            drop(transaction);
            state.fail_directory_sync_after(successes);
            assert!(ActivationTransaction::resume(&state, &target).is_err());
            assert_eq!(
                fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
                old_launcher
            );
            if successes == 4 {
                assert!(
                    !root
                        .path()
                        .join("state")
                        .join(super::super::super::super::MANIFEST)
                        .exists()
                );
            }
            state.fail_directory_sync(false);
            let mut restored = ActivationTransaction::resume(&state, &target).unwrap();
            assert_eq!(
                fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
                old_launcher
            );
            assert_eq!(
                fs::read_to_string(root.path().join("drop-ins").join(DROP_IN)).unwrap(),
                old_drop
            );
            assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 3);
            restored.rollback().unwrap();
        }
    }

    #[test]
    fn legacy_missing_manifest_intent_refuses_before_restoring_other_roles() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = crate::integrations::gnome::render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        let new_launcher = updated.launcher.clone();
        let new_drop = updated.drop_in.clone();
        transaction.update(updated).unwrap();
        drop(transaction);
        fs::remove_file(
            root.path()
                .join("state")
                .join(super::super::EXCHANGE_NAMES[2]),
        )
        .unwrap();
        let before_state = fs::read_dir(root.path().join("state")).unwrap().count();
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        assert_eq!(
            fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
            new_launcher
        );
        assert_eq!(
            fs::read_to_string(root.path().join("drop-ins").join(DROP_IN)).unwrap(),
            new_drop
        );
        assert_eq!(
            fs::read_dir(root.path().join("state")).unwrap().count(),
            before_state
        );
    }

    #[test]
    fn changed_final_manifest_refuses_before_inverse_work() {
        let (root, state, target, initial) = fixture();
        let mut transaction = ActivationTransaction::begin(&state, &target, initial).unwrap();
        transaction.apply().unwrap();
        let updated = crate::integrations::gnome::render_activation(
            &root.path().join("native-v2.so"),
            &root.path().join("state").join(LAUNCHER),
        )
        .unwrap();
        let new_launcher = updated.launcher.clone();
        transaction.update(updated).unwrap();
        drop(transaction);
        let path = root
            .path()
            .join("state")
            .join(super::super::super::super::MANIFEST);
        fs::write(&path, b"user manifest").unwrap();
        let before_state = fs::read_dir(root.path().join("state")).unwrap().count();
        let before_target = fs::read_dir(root.path().join("drop-ins")).unwrap().count();
        assert!(ActivationTransaction::resume(&state, &target).is_err());
        assert_eq!(
            fs::read_to_string(root.path().join("state").join(LAUNCHER)).unwrap(),
            new_launcher
        );
        assert_eq!(fs::read(path).unwrap(), b"user manifest");
        assert_eq!(
            fs::read_dir(root.path().join("state")).unwrap().count(),
            before_state
        );
        assert_eq!(
            fs::read_dir(root.path().join("drop-ins")).unwrap().count(),
            before_target
        );
    }
}
