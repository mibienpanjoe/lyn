//! Read-only authority for a durable, explicitly committed update decision.

use super::super::super::{Place, recovery, refused, removal};
use super::super::{COMPENSATION_PENDING, EXCHANGE_NAMES, PRECOMMIT_PENDING};
use super::{CLEANUP_PENDING, CleanupRecord, validate_manifest};
use crate::integrations::gnome::{
    lock::InstallationLock,
    managed::{ManagedDirectory, OwnedFile},
    ownership::Receipt,
};
use std::io;

pub(in crate::integrations::gnome) struct CommittedUpdateSnapshot {
    journal: OwnedFile,
    pub(in crate::integrations::gnome) launcher: OwnedFile,
    pub(in crate::integrations::gnome) drop_in: OwnedFile,
    manifest: OwnedFile,
    backups: Vec<(Place, OwnedFile)>,
    exchanges: Vec<OwnedFile>,
    precommit: Option<OwnedFile>,
    absent: Vec<(Place, String)>,
}

fn no_competing_intent(state: &ManagedDirectory) -> io::Result<()> {
    for name in [
        recovery::PENDING_INSTALL,
        removal::PENDING_REMOVAL,
        removal::RETIRED_REMOVAL,
        COMPENSATION_PENDING,
    ] {
        state.require_absent(name)?;
    }
    Ok(())
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

fn optional_receipt(
    state: &ManagedDirectory,
    target: &ManagedDirectory,
    place: Place,
    receipt: &Receipt,
    absent: &mut Vec<(Place, String)>,
) -> io::Result<Option<OwnedFile>> {
    match receipt.reopen(directory(place, state, target)) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            absent.push((place, receipt.filename().into()));
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

impl CommittedUpdateSnapshot {
    pub(in crate::integrations::gnome) fn revision(&self) -> io::Result<String> {
        crate::integrations::gnome::ownership::revision(
            b"lyn-native-committed-update-revision-v1\0",
            &self.journal,
        )
    }
    /// No locks, directory creation, synchronization or recovery occurs here.
    pub(in crate::integrations::gnome) fn open(
        state: &ManagedDirectory,
        target: &ManagedDirectory,
    ) -> io::Result<Self> {
        no_competing_intent(state)?;
        let journal = state.read_state(CLEANUP_PENDING)?;
        let body: CleanupRecord =
            serde_json::from_slice(&journal.content).map_err(|_| refused())?;
        body.validate()?;
        let launcher = body.current.launcher.reopen(state)?;
        let drop_in = body.current.drop_in.reopen(target)?;
        let manifest = body.current.manifest.reopen(state)?;
        validate_manifest(state, target, &manifest)?;
        let mut absent = Vec::new();
        let precommit = if let Some(receipt) = &body.precommit {
            optional_receipt(state, target, Place::State, receipt, &mut absent)?
        } else {
            state.require_absent(PRECOMMIT_PENDING)?;
            absent.push((Place::State, PRECOMMIT_PENDING.into()));
            None
        };
        let mut exchanges = Vec::new();
        for (role, name) in EXCHANGE_NAMES.into_iter().enumerate() {
            if let Some(receipt) = body.exchanges.get(role).and_then(Option::as_ref) {
                if let Some(file) =
                    optional_receipt(state, target, Place::State, receipt, &mut absent)?
                {
                    exchanges.push(file);
                }
            } else {
                state.require_absent(name)?;
                absent.push((Place::State, name.into()));
            }
        }
        let mut backups = Vec::new();
        for (place, receipt) in [
            (Place::State, &body.backups.manifest),
            (Place::DropIns, &body.backups.drop_in),
            (Place::State, &body.backups.launcher),
        ] {
            if let Some(file) = optional_receipt(state, target, place, receipt, &mut absent)? {
                backups.push((place, file));
            }
        }
        let result = Self {
            journal,
            launcher,
            drop_in,
            manifest,
            backups,
            exchanges,
            precommit,
            absent,
        };
        result.recheck(state, target)?;
        Ok(result)
    }

    pub(in crate::integrations::gnome) fn recheck(
        &self,
        state: &ManagedDirectory,
        target: &ManagedDirectory,
    ) -> io::Result<()> {
        no_competing_intent(state)?;
        for file in [&self.journal, &self.launcher, &self.manifest] {
            state.verify(file)?;
        }
        target.verify(&self.drop_in)?;
        validate_manifest(state, target, &self.manifest)?;
        for (place, file) in &self.backups {
            directory(*place, state, target).verify(file)?;
        }
        for file in &self.exchanges {
            state.verify(file)?;
        }
        if let Some(file) = &self.precommit {
            state.verify(file)?;
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
        // The intent may have become visible before its original directory sync
        // failed. Establish durability before deleting any recorded backup.
        state.sync_directory()?;
        for (place, file) in self.backups {
            lock.check()?;
            directory(place, state, target).remove(&file)?;
        }
        state.sync_directory()?;
        target.sync_directory()?;
        for file in self.exchanges.into_iter().rev() {
            lock.check()?;
            state.remove(&file)?;
        }
        if let Some(file) = self.precommit {
            lock.check()?;
            state.remove(&file)?;
        }
        lock.check()?;
        state.remove(&self.journal)
    }
}
