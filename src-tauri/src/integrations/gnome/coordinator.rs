//! Guarded activation-file preparation/update/removal. No service actions or UI paths.

#![allow(dead_code)] // Application command wiring follows private acceptance.

use super::{
    artifact::{self, Artifact},
    directories,
    managed::ManagedDirectory,
    module_store::{self, ModuleStore, StoredModule},
    render_activation, render_activation_with_previous,
    service::{self, DefinitionState, OwnedActivation, ServiceRefusal},
    status,
    transaction::{self, ActivationFilesState, ActivationTransaction},
};
use std::{
    io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

struct Paths {
    state: PathBuf,
    target: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    Preflight,
    Preparing,
    Applying,
    Verifying,
}

#[derive(Debug)]
pub(super) enum Cause {
    NoArtifact,
    InvalidLocation,
    Conflict,
    StaleRevision,
    Service(ServiceRefusal),
    Io(io::Error),
}

#[derive(Debug)]
pub(super) struct Failure {
    pub(super) stage: Stage,
    pub(super) cause: Cause,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Prepared {
    pub(super) definition: DefinitionState,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Removed;

#[derive(Debug)]
pub(super) struct RollbackOffer {
    pub(super) revision: String,
}

fn rollback_offer_at(paths: &Paths) -> Result<RollbackOffer, Failure> {
    let active = ExistingActivation::open(paths)?;
    let previous = active
        .owned
        .previous_module()
        .ok_or_else(|| failure(Stage::Preflight, Cause::Conflict))?;
    let target = PreviousModule::open_digest(paths, &active, previous)
        .map_err(|error| failure(Stage::Preflight, Cause::Io(error)))?;
    target
        .recheck(paths, &active)
        .map_err(|error| failure(Stage::Preflight, Cause::Io(error)))?;
    let revision = active
        .owned
        .revision()
        .map_err(|error| failure(Stage::Preflight, Cause::Io(error)))?;
    Ok(RollbackOffer { revision })
}

fn require_revision(
    active: &ExistingActivation,
    expected: &str,
    stage: Stage,
) -> Result<(), Failure> {
    if active
        .owned
        .revision()
        .map_err(|error| failure(stage, Cause::Io(error)))?
        != expected
    {
        return Err(failure(stage, Cause::StaleRevision));
    }
    Ok(())
}

fn rollback_at(
    paths: &Paths,
    expected_revision: &str,
    service: &mut impl ServiceChecks,
) -> Result<Prepared, Failure> {
    let active = ExistingActivation::open(paths)?;
    require_revision(&active, expected_revision, Stage::Preflight)?;
    let previous = active
        .owned
        .previous_module()
        .ok_or_else(|| failure(Stage::Preflight, Cause::Conflict))?;
    let target = PreviousModule::open_digest(paths, &active, previous)
        .map_err(|error| failure(Stage::Preflight, Cause::Io(error)))?;
    owned_definition(service, &active.owned, Stage::Preflight)?;
    let preparing = |error| failure(Stage::Preparing, Cause::Io(error));
    let mut transaction =
        ActivationTransaction::open_complete(&active.state, &active.target).map_err(preparing)?;
    #[cfg(test)]
    service.after_activation_lock();
    active.recheck(paths).map_err(preparing)?;
    require_revision(&active, expected_revision, Stage::Preparing)?;
    target.recheck(paths, &active).map_err(preparing)?;
    owned_definition(service, &active.owned, Stage::Applying)?;
    let applying = |error| failure(Stage::Applying, Cause::Io(error));
    active.recheck(paths).map_err(applying)?;
    target.recheck(paths, &active).map_err(applying)?;
    require_revision(&active, expected_revision, Stage::Applying)?;
    let module_path = paths
        .state
        .join("modules")
        .join(previous)
        .join(artifact::format::MODULE_NAME);
    let rendered = render_activation_with_previous(
        &module_path,
        &paths.state.join(transaction::LAUNCHER),
        Some(active.owned.module_digest()),
    )
    .map_err(|_| failure(Stage::Applying, Cause::Conflict))?;
    transaction.update(rendered).map_err(applying)?;
    target.recheck(paths, &active).map_err(applying)?;
    transaction.finish_update().map_err(applying)?;
    let verifying = |error| failure(Stage::Verifying, Cause::Io(error));
    let current = OwnedActivation::open(&paths.state, &paths.target).map_err(verifying)?;
    if current.module_path() != module_path
        || current.previous_module() != Some(active.owned.module_digest())
    {
        return Err(failure(Stage::Verifying, Cause::Conflict));
    }
    let definition = owned_definition(service, &current, Stage::Verifying)?;
    target.recheck(paths, &active).map_err(verifying)?;
    current.recheck().map_err(verifying)?;
    Ok(Prepared { definition })
}

struct ExistingActivation {
    state: ManagedDirectory,
    target: ManagedDirectory,
    owned: OwnedActivation,
}

impl ExistingActivation {
    fn open(paths: &Paths) -> Result<Self, Failure> {
        let preflight = |error| failure(Stage::Preflight, Cause::Io(error));
        if paths.inspect().map_err(preflight)? != ActivationFilesState::Owned {
            return Err(failure(Stage::Preflight, Cause::Conflict));
        }
        let state = directories::open_private(&paths.state, false).map_err(preflight)?;
        let target = directories::open_drop_ins(&paths.target, false).map_err(preflight)?;
        let owned = OwnedActivation::open(&paths.state, &paths.target).map_err(preflight)?;
        let result = Self {
            state,
            target,
            owned,
        };
        result.recheck(paths).map_err(preflight)?;
        Ok(result)
    }

    fn recheck(&self, paths: &Paths) -> io::Result<()> {
        paths.require_no_retirement()?;
        paths.recheck_activation(&self.state, &self.target)?;
        self.owned.recheck()
    }
}

struct PreviousModule {
    directory: ManagedDirectory,
    stored: StoredModule,
}

impl PreviousModule {
    fn open(paths: &Paths, active: &ExistingActivation) -> io::Result<Self> {
        Self::open_digest(paths, active, active.owned.module_digest())
    }

    fn open_digest(paths: &Paths, active: &ExistingActivation, digest: &str) -> io::Result<Self> {
        let directory = directories::open_private_child(&active.state, "modules", false)?;
        paths.recheck(&active.state, &active.target, &directory)?;
        let stored = module_store::read_existing(&directory, digest)?;
        Ok(Self { directory, stored })
    }

    // Valid after file replacement too: the prior activation files deliberately
    // changed, but the directory namespace and previous module must not.
    fn recheck(&self, paths: &Paths, active: &ExistingActivation) -> io::Result<()> {
        paths.recheck(&active.state, &active.target, &self.directory)?;
        module_store::verify_existing(&self.directory, &self.stored)
    }
}

fn owned_definition(
    service: &mut impl ServiceChecks,
    owned: &OwnedActivation,
    stage: Stage,
) -> Result<DefinitionState, Failure> {
    let definition = service
        .owned(owned)
        .map_err(|error| failure(stage, Cause::Service(error)))?;
    if definition == DefinitionState::Stock {
        return Err(failure(stage, Cause::Conflict));
    }
    Ok(definition)
}

fn update_at(
    paths: &Paths,
    artifact: &Artifact,
    service: &mut impl ServiceChecks,
) -> Result<Prepared, Failure> {
    let manifest = serde_json::to_vec(&artifact.manifest)
        .map_err(|_| failure(Stage::Preflight, Cause::NoArtifact))?;
    artifact::format::validate(&manifest, artifact.bytes)
        .map_err(|_| failure(Stage::Preflight, Cause::NoArtifact))?;
    let active = ExistingActivation::open(paths)?;
    let old = PreviousModule::open(paths, &active)
        .map_err(|error| failure(Stage::Preflight, Cause::Io(error)))?;
    owned_definition(service, &active.owned, Stage::Preflight)?;
    let preparing = |error| failure(Stage::Preparing, Cause::Io(error));
    let mut transaction =
        ActivationTransaction::open_complete(&active.state, &active.target).map_err(preparing)?;
    #[cfg(test)]
    service.after_activation_lock();
    active.recheck(paths).map_err(preparing)?;
    old.recheck(paths, &active).map_err(preparing)?;
    let store = ModuleStore::open(&old.directory).map_err(preparing)?;
    let candidate = store.retain(artifact).map_err(preparing)?;
    let mut definition = owned_definition(service, &active.owned, Stage::Applying)?;
    let applying = |error| failure(Stage::Applying, Cause::Io(error));
    active.recheck(paths).map_err(applying)?;
    old.recheck(paths, &active).map_err(applying)?;
    store.verify(&candidate, artifact).map_err(applying)?;
    let module_path = paths
        .state
        .join("modules")
        .join(&artifact.manifest.sha256)
        .join(artifact::format::MODULE_NAME);
    if active.owned.module_path() != module_path {
        let rendered = render_activation_with_previous(
            &module_path,
            &paths.state.join(transaction::LAUNCHER),
            Some(active.owned.module_digest()),
        )
        .map_err(|_| failure(Stage::Applying, Cause::InvalidLocation))?;
        transaction.update(rendered).map_err(applying)?;
        old.recheck(paths, &active).map_err(applying)?;
        store.verify(&candidate, artifact).map_err(applying)?;
        transaction.finish_update().map_err(applying)?;
        let verifying = |error| failure(Stage::Verifying, Cause::Io(error));
        let current = OwnedActivation::open(&paths.state, &paths.target).map_err(verifying)?;
        if current.module_path() != module_path
            || current.previous_module() != Some(active.owned.module_digest())
        {
            return Err(failure(Stage::Verifying, Cause::Conflict));
        }
        definition = owned_definition(service, &current, Stage::Verifying)?;
        current.recheck().map_err(verifying)?;
    } else {
        active.recheck(paths).map_err(applying)?;
    }
    let verifying = |error| failure(Stage::Verifying, Cause::Io(error));
    old.recheck(paths, &active).map_err(verifying)?;
    store.verify(&candidate, artifact).map_err(verifying)?;
    Ok(Prepared { definition })
}

fn remove_at(paths: &Paths, service: &mut impl ServiceChecks) -> Result<Removed, Failure> {
    let preflight = |error| failure(Stage::Preflight, Cause::Io(error));
    if paths.inspect().map_err(preflight)? == ActivationFilesState::Absent {
        return Ok(Removed);
    }
    let active = ExistingActivation::open(paths)?;
    owned_definition(service, &active.owned, Stage::Preflight)?;
    let preparing = |error| failure(Stage::Preparing, Cause::Io(error));
    let mut transaction =
        ActivationTransaction::open_complete(&active.state, &active.target).map_err(preparing)?;
    #[cfg(test)]
    service.after_activation_lock();
    active.recheck(paths).map_err(preparing)?;
    owned_definition(service, &active.owned, Stage::Applying)?;
    let applying = |error| failure(Stage::Applying, Cause::Io(error));
    active.recheck(paths).map_err(applying)?;
    #[cfg(test)]
    service.before_retire(&mut transaction);
    transaction
        .retire_activation(
            &paths.state,
            active.owned.module_digest(),
            active.owned.previous_module(),
        )
        .map_err(applying)?;
    let verifying = |error| failure(Stage::Verifying, Cause::Io(error));
    paths
        .recheck_activation(&active.state, &active.target)
        .map_err(verifying)?;
    if paths.inspect().map_err(verifying)? != ActivationFilesState::Absent {
        return Err(failure(Stage::Verifying, Cause::Conflict));
    }
    OwnedActivation::open_retired(&paths.state, &paths.target)
        .map_err(verifying)?
        .recheck()
        .map_err(verifying)?;
    Ok(Removed)
}

fn recover_install_at(
    paths: &Paths,
    service: &mut impl ServiceChecks,
) -> Result<Prepared, Failure> {
    let preflight = |error| failure(Stage::Preflight, Cause::Io(error));
    let active = ExistingActivation {
        state: directories::open_private(&paths.state, false).map_err(preflight)?,
        target: directories::open_drop_ins(&paths.target, false).map_err(preflight)?,
        owned: OwnedActivation::open_pending_install(&paths.state, &paths.target)
            .map_err(preflight)?,
    };
    active.recheck(paths).map_err(preflight)?;
    let module = PreviousModule::open(paths, &active).map_err(preflight)?;
    owned_definition(service, &active.owned, Stage::Preflight)?;
    let mut check_failure = None;
    let result =
        ActivationTransaction::resume_install_checked(&active.state, &active.target, || {
            #[cfg(test)]
            service.after_activation_lock();
            let check = (|| {
                let applying = |error| failure(Stage::Applying, Cause::Io(error));
                active.recheck(paths).map_err(applying)?;
                module.recheck(paths, &active).map_err(applying)?;
                owned_definition(service, &active.owned, Stage::Applying)?;
                active.recheck(paths).map_err(applying)?;
                module.recheck(paths, &active).map_err(applying)
            })();
            if let Err(error) = check {
                check_failure = Some(error);
                return Err(io::Error::other(
                    "native install recovery preflight refused",
                ));
            }
            Ok(())
        });
    let _transaction = result.map_err(|error| {
        check_failure.unwrap_or_else(|| failure(Stage::Applying, Cause::Io(error)))
    })?;
    let verifying = |error| failure(Stage::Verifying, Cause::Io(error));
    let current = OwnedActivation::open(&paths.state, &paths.target).map_err(verifying)?;
    if current.module_path() != active.owned.module_path() {
        return Err(failure(Stage::Verifying, Cause::Conflict));
    }
    let definition = owned_definition(service, &current, Stage::Verifying)?;
    current.recheck().map_err(verifying)?;
    module.recheck(paths, &active).map_err(verifying)?;
    Ok(Prepared { definition })
}

fn recover_removal_at(paths: &Paths, service: &mut impl ServiceChecks) -> Result<Removed, Failure> {
    let preflight = |error| failure(Stage::Preflight, Cause::Io(error));
    let state = directories::open_private(&paths.state, false).map_err(preflight)?;
    let target = directories::open_drop_ins(&paths.target, false).map_err(preflight)?;
    let owned = match paths.inspect().map_err(preflight)? {
        ActivationFilesState::Pending => {
            OwnedActivation::open_pending_removal(&paths.state, &paths.target)
        }
        ActivationFilesState::Absent => OwnedActivation::open_retired(&paths.state, &paths.target),
        _ => return Err(failure(Stage::Preflight, Cause::Conflict)),
    }
    .map_err(preflight)?;
    paths
        .recheck_activation(&state, &target)
        .map_err(preflight)?;
    owned.recheck().map_err(preflight)?;
    service
        .owned(&owned)
        .map_err(|error| failure(Stage::Preflight, Cause::Service(error)))?;
    let mut check_failure = None;
    let result =
        ActivationTransaction::resume_retirement_checked(&state, &target, &paths.state, || {
            #[cfg(test)]
            service.after_activation_lock();
            let check = (|| {
                let applying = |error| failure(Stage::Applying, Cause::Io(error));
                paths
                    .recheck_activation(&state, &target)
                    .map_err(applying)?;
                owned.recheck().map_err(applying)?;
                service
                    .owned(&owned)
                    .map_err(|error| failure(Stage::Applying, Cause::Service(error)))?;
                paths
                    .recheck_activation(&state, &target)
                    .map_err(applying)?;
                owned.recheck().map_err(applying)
            })();
            if let Err(error) = check {
                check_failure = Some(error);
                return Err(io::Error::other(
                    "native removal recovery preflight refused",
                ));
            }
            Ok(())
        });
    let _transaction = result.map_err(|error| {
        check_failure.unwrap_or_else(|| failure(Stage::Applying, Cause::Io(error)))
    })?;
    let verifying = |error| failure(Stage::Verifying, Cause::Io(error));
    let retired = OwnedActivation::open_retired(&paths.state, &paths.target).map_err(verifying)?;
    service
        .owned(&retired)
        .map_err(|error| failure(Stage::Verifying, Cause::Service(error)))?;
    paths
        .recheck_activation(&state, &target)
        .map_err(verifying)?;
    retired.recheck().map_err(verifying)?;
    Ok(Removed)
}

trait ServiceChecks {
    fn stock(&mut self) -> Result<(), ServiceRefusal>;
    fn owned(&mut self, owned: &OwnedActivation) -> Result<DefinitionState, ServiceRefusal>;
    #[cfg(test)]
    fn after_activation_lock(&mut self) {}
    #[cfg(test)]
    fn before_retire(&mut self, _transaction: &mut ActivationTransaction<'_>) {}
}

struct RealService {
    target: PathBuf,
}
impl ServiceChecks for RealService {
    fn stock(&mut self) -> Result<(), ServiceRefusal> {
        service::inspect_unmodified_service_at(&self.target)
    }
    fn owned(&mut self, owned: &OwnedActivation) -> Result<DefinitionState, ServiceRefusal> {
        service::inspect_owned_service(owned)
    }
}

fn failure(stage: Stage, cause: Cause) -> Failure {
    Failure { stage, cause }
}

fn same_directory(fresh: &ManagedDirectory, pinned: &ManagedDirectory) -> io::Result<()> {
    pinned.check()?;
    let fresh = fresh.directory.metadata()?;
    let pinned = pinned.directory.metadata()?;
    if (fresh.dev(), fresh.ino()) != (pinned.dev(), pinned.ino()) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "native activation directory changed",
        ));
    }
    Ok(())
}

impl Paths {
    fn require_no_retirement(&self) -> io::Result<()> {
        match directories::open_private(&self.state, false) {
            Ok(state) => state.require_absent(transaction::RETIRED_REMOVAL),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
    fn from_home(home: &Path) -> Result<Self, Failure> {
        let data = std::env::var_os("XDG_DATA_HOME");
        let config = std::env::var_os("XDG_CONFIG_HOME");
        let (state, target) = status::locations(home, data.as_deref(), config.as_deref())
            .ok_or_else(|| failure(Stage::Preflight, Cause::InvalidLocation))?;
        Ok(Self { state, target })
    }

    fn inspect(&self) -> io::Result<ActivationFilesState> {
        let state = match directories::open_private(&self.state, false) {
            Ok(value) => Some(value),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let target = match directories::open_drop_ins(&self.target, false) {
            Ok(value) => Some(value),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok(transaction::inspect_activation_files(
            state.as_ref(),
            target.as_ref(),
        ))
    }

    fn recheck_activation(
        &self,
        state: &ManagedDirectory,
        target: &ManagedDirectory,
    ) -> io::Result<()> {
        let fresh_state = directories::open_private(&self.state, false)?;
        let fresh_target = directories::open_drop_ins(&self.target, false)?;
        same_directory(&fresh_state, state)?;
        same_directory(&fresh_target, target)
    }

    fn recheck(
        &self,
        state: &ManagedDirectory,
        target: &ManagedDirectory,
        modules: &ManagedDirectory,
    ) -> io::Result<()> {
        self.recheck_activation(state, target)?;
        let fresh_modules = directories::open_private(&self.state.join("modules"), false)?;
        same_directory(&fresh_modules, modules)
    }
}

/// No runtime artifact, destination or service implementation is supplied by IPC.
/// Failure can have filesystem effects; the authoritative state must be inspected.
pub(super) fn prepare_first(home: &Path) -> Result<Prepared, Failure> {
    let artifact = artifact::embedded()
        .map_err(|_| failure(Stage::Preflight, Cause::NoArtifact))?
        .ok_or_else(|| failure(Stage::Preflight, Cause::NoArtifact))?;
    let paths = Paths::from_home(home)?;
    prepare_at(
        &paths,
        &artifact,
        &mut RealService {
            target: paths.target.clone(),
        },
    )
}

pub(super) fn prepare_update(home: &Path) -> Result<Prepared, Failure> {
    let artifact = artifact::embedded()
        .map_err(|_| failure(Stage::Preflight, Cause::NoArtifact))?
        .ok_or_else(|| failure(Stage::Preflight, Cause::NoArtifact))?;
    let paths = Paths::from_home(home)?;
    update_at(
        &paths,
        &artifact,
        &mut RealService {
            target: paths.target.clone(),
        },
    )
}

pub(super) fn rollback_offer(home: &Path) -> Result<RollbackOffer, Failure> {
    rollback_offer_at(&Paths::from_home(home)?)
}

pub(super) fn recover_install(home: &Path) -> Result<Prepared, Failure> {
    let paths = Paths::from_home(home)?;
    recover_install_at(
        &paths,
        &mut RealService {
            target: paths.target.clone(),
        },
    )
}

pub(super) fn recover_removal(home: &Path) -> Result<Removed, Failure> {
    let paths = Paths::from_home(home)?;
    recover_removal_at(
        &paths,
        &mut RealService {
            target: paths.target.clone(),
        },
    )
}

pub(super) fn rollback(home: &Path, expected_revision: &str) -> Result<Prepared, Failure> {
    let paths = Paths::from_home(home)?;
    rollback_at(
        &paths,
        expected_revision,
        &mut RealService {
            target: paths.target.clone(),
        },
    )
}

/// Only activation files are removed. No bundle is needed and retained module
/// data is never opened, repaired or deleted as a side effect of removal.
pub(super) fn remove(home: &Path) -> Result<Removed, Failure> {
    let paths = Paths::from_home(home)?;
    remove_at(
        &paths,
        &mut RealService {
            target: paths.target.clone(),
        },
    )
}

fn prepare_at(
    paths: &Paths,
    artifact: &Artifact,
    service: &mut impl ServiceChecks,
) -> Result<Prepared, Failure> {
    paths
        .require_no_retirement()
        .map_err(|error| failure(Stage::Preflight, Cause::Io(error)))?;
    let manifest = serde_json::to_vec(&artifact.manifest)
        .map_err(|_| failure(Stage::Preflight, Cause::NoArtifact))?;
    artifact::format::validate(&manifest, artifact.bytes)
        .map_err(|_| failure(Stage::Preflight, Cause::NoArtifact))?;
    if paths
        .inspect()
        .map_err(|error| failure(Stage::Preflight, Cause::Io(error)))?
        != ActivationFilesState::Absent
    {
        return Err(failure(Stage::Preflight, Cause::Conflict));
    }
    let module_path = paths
        .state
        .join("modules")
        .join(&artifact.manifest.sha256)
        .join(artifact::format::MODULE_NAME);
    let rendered = render_activation(&module_path, &paths.state.join(transaction::LAUNCHER))
        .map_err(|_| failure(Stage::Preflight, Cause::InvalidLocation))?;
    service
        .stock()
        .map_err(|error| failure(Stage::Preflight, Cause::Service(error)))?;
    let preparing = |error| failure(Stage::Preparing, Cause::Io(error));
    let state = directories::open_private(&paths.state, true).map_err(preparing)?;
    let target = directories::open_drop_ins(&paths.target, true).map_err(preparing)?;
    let mut transaction =
        ActivationTransaction::begin(&state, &target, rendered).map_err(preparing)?;
    #[cfg(test)]
    service.after_activation_lock();
    paths
        .recheck_activation(&state, &target)
        .map_err(preparing)?;
    let modules = directories::open_private_child(&state, "modules", true).map_err(preparing)?;
    paths
        .recheck(&state, &target, &modules)
        .map_err(preparing)?;
    let store = ModuleStore::open(&modules).map_err(preparing)?;
    let stored = store.retain(artifact).map_err(preparing)?;
    service
        .stock()
        .map_err(|error| failure(Stage::Applying, Cause::Service(error)))?;
    let applying = |error| failure(Stage::Applying, Cause::Io(error));
    paths.recheck(&state, &target, &modules).map_err(applying)?;
    store.verify(&stored, artifact).map_err(applying)?;
    transaction.apply().map_err(applying)?;
    let verifying = |error| failure(Stage::Verifying, Cause::Io(error));
    let owned = OwnedActivation::open(&paths.state, &paths.target).map_err(verifying)?;
    if owned.module_path() != module_path {
        return Err(failure(Stage::Verifying, Cause::Conflict));
    }
    let definition = service
        .owned(&owned)
        .map_err(|error| failure(Stage::Verifying, Cause::Service(error)))?;
    if definition == DefinitionState::Stock {
        return Err(failure(Stage::Verifying, Cause::Conflict));
    }
    paths
        .recheck(&state, &target, &modules)
        .map_err(verifying)?;
    store.verify(&stored, artifact).map_err(verifying)?;
    owned.recheck().map_err(verifying)?;
    Ok(Prepared { definition })
}

#[cfg(test)]
pub(super) fn prepare_private(root: &Path, artifact: &Artifact) -> Result<Prepared, Failure> {
    // The caller is the cfg(test) entry after its private-root validation.
    let paths = Paths {
        state: root.join("state"),
        target: root.join("units/gnome-terminal-server.service.d"),
    };
    prepare_at(
        &paths,
        artifact,
        &mut RealService {
            target: paths.target.clone(),
        },
    )
}

#[cfg(test)]
pub(super) fn update_private(root: &Path, artifact: &Artifact) -> Result<Prepared, Failure> {
    let paths = Paths {
        state: root.join("state"),
        target: root.join("units/gnome-terminal-server.service.d"),
    };
    update_at(
        &paths,
        artifact,
        &mut RealService {
            target: paths.target.clone(),
        },
    )
}

#[cfg(test)]
pub(super) fn recover_interrupted_private(
    root: &Path,
    checkpoint: u8,
) -> Result<Prepared, Failure> {
    // Called only after the cfg(test) entry's private filesystem-bus guard.
    // Exercise a dropped transaction, not a production fault-injection option.
    let paths = Paths {
        state: root.join("state"),
        target: root.join("units/gnome-terminal-server.service.d"),
    };
    let active = ExistingActivation::open(&paths)?;
    let rendered = render_activation(
        active.owned.module_path(),
        &paths.state.join(transaction::LAUNCHER),
    )
    .map_err(|_| failure(Stage::Preflight, Cause::Conflict))?;
    let mut service = RealService {
        target: paths.target.clone(),
    };
    {
        // Reset only this fixture's complete files; no production retirement.
        let mut transaction = ActivationTransaction::open_complete(&active.state, &active.target)
            .map_err(|error| failure(Stage::Preparing, Cause::Io(error)))?;
        owned_definition(&mut service, &active.owned, Stage::Applying)?;
        transaction
            .rollback()
            .map_err(|error| failure(Stage::Applying, Cause::Io(error)))?;
    }
    {
        let mut transaction = ActivationTransaction::begin(&active.state, &active.target, rendered)
            .map_err(|error| failure(Stage::Preparing, Cause::Io(error)))?;
        transaction.interrupt_install_after(checkpoint);
        assert!(
            transaction.apply().is_err(),
            "private checkpoint must interrupt installation"
        );
    }
    recover_install_at(&paths, &mut service)
}

#[cfg(test)]
pub(super) fn remove_private(root: &Path) -> Result<Removed, Failure> {
    let paths = Paths {
        state: root.join("state"),
        target: root.join("units/gnome-terminal-server.service.d"),
    };
    remove_at(
        &paths,
        &mut RealService {
            target: paths.target.clone(),
        },
    )
}

#[cfg(test)]
pub(super) fn interrupt_removal_private(root: &Path) -> Result<Removed, Failure> {
    struct Interrupt(RealService);
    impl ServiceChecks for Interrupt {
        fn stock(&mut self) -> Result<(), ServiceRefusal> {
            self.0.stock()
        }
        fn owned(&mut self, owned: &OwnedActivation) -> Result<DefinitionState, ServiceRefusal> {
            self.0.owned(owned)
        }
        fn before_retire(&mut self, transaction: &mut ActivationTransaction<'_>) {
            transaction.interrupt_removal_after(13, true);
        }
    }
    let paths = Paths {
        state: root.join("state"),
        target: root.join("units/gnome-terminal-server.service.d"),
    };
    remove_at(
        &paths,
        &mut Interrupt(RealService {
            target: paths.target.clone(),
        }),
    )
}

#[cfg(test)]
pub(super) fn recover_removal_private(root: &Path) -> Result<Removed, Failure> {
    let paths = Paths {
        state: root.join("state"),
        target: root.join("units/gnome-terminal-server.service.d"),
    };
    recover_removal_at(
        &paths,
        &mut RealService {
            target: paths.target.clone(),
        },
    )
}

#[cfg(test)]
pub(super) fn rollback_private(root: &Path) -> Result<Prepared, Failure> {
    let paths = Paths {
        state: root.join("state"),
        target: root.join("units/gnome-terminal-server.service.d"),
    };
    let offer = rollback_offer_at(&paths)?;
    rollback_at(
        &paths,
        &offer.revision,
        &mut RealService {
            target: paths.target.clone(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    #[derive(Default)]
    struct Checks {
        stock_calls: usize,
        owned_calls: usize,
        refuse_at: usize,
    }
    impl ServiceChecks for Checks {
        fn stock(&mut self) -> Result<(), ServiceRefusal> {
            self.stock_calls += 1;
            if self.stock_calls == self.refuse_at {
                Err(ServiceRefusal::Changed)
            } else {
                Ok(())
            }
        }
        fn owned(&mut self, owned: &OwnedActivation) -> Result<DefinitionState, ServiceRefusal> {
            owned.recheck().unwrap();
            self.owned_calls += 1;
            Ok(DefinitionState::NeedsReload)
        }
    }

    fn artifact() -> Artifact {
        use sha2::{Digest, Sha256};
        let mut bytes = vec![0; 32768];
        bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        bytes[16..18].copy_from_slice(&3_u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&62_u16.to_le_bytes());
        bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
        bytes[52..54].copy_from_slice(&64_u16.to_le_bytes());
        Artifact {
            manifest: artifact::format::Manifest {
                version: 1,
                protocol: 1,
                target: artifact::format::TARGET.into(),
                gtk: [3, 24, 33],
                vte: [0, 68, 0],
                length: bytes.len(),
                sha256: Sha256::digest(&bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            },
            bytes: bytes.leak(),
        }
    }

    fn fixture() -> (tempfile::TempDir, Paths) {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let paths = Paths {
            state: root.path().join("state"),
            target: root.path().join("units/gnome-terminal-server.service.d"),
        };
        (root, paths)
    }

    fn other_artifact() -> Artifact {
        use sha2::{Digest, Sha256};
        let mut other = artifact();
        let mut bytes = other.bytes.to_vec();
        bytes[63] ^= 1;
        other.manifest.sha256 = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        other.bytes = bytes.leak();
        other
    }

    fn interrupted_install(paths: &Paths, checkpoint: u8) {
        let artifact = artifact();
        let mut checks = Checks::default();
        prepare_at(paths, &artifact, &mut checks).unwrap();
        let state = directories::open_private(&paths.state, false).unwrap();
        let target = directories::open_drop_ins(&paths.target, false).unwrap();
        ActivationTransaction::open_complete(&state, &target)
            .unwrap()
            .rollback()
            .unwrap();
        let module = paths
            .state
            .join("modules")
            .join(&artifact.manifest.sha256)
            .join(artifact::format::MODULE_NAME);
        let rendered =
            render_activation(&module, &paths.state.join(transaction::LAUNCHER)).unwrap();
        let mut transaction = ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.interrupt_install_after(checkpoint);
        assert!(transaction.apply().is_err());
    }

    #[test]
    fn pending_install_recovery_uses_owned_history_at_each_publication_prefix() {
        for point in 1..=4 {
            let (_root, paths) = fixture();
            interrupted_install(&paths, point);
            assert_eq!(paths.inspect().unwrap(), ActivationFilesState::Pending);
            let mut checks = Checks::default();
            assert_eq!(
                recover_install_at(&paths, &mut checks).unwrap().definition,
                DefinitionState::NeedsReload
            );
            assert_eq!(checks.owned_calls, 3);
            assert_eq!(checks.stock_calls, 0);
            assert_eq!(paths.inspect().unwrap(), ActivationFilesState::Owned);
            assert!(recover_install_at(&paths, &mut checks).is_err());
            assert_eq!(checks.owned_calls, 3);
        }
    }

    #[test]
    fn pending_install_recovery_refuses_changed_module_before_service_or_publication() {
        let (_root, paths) = fixture();
        interrupted_install(&paths, 1);
        let module = paths
            .state
            .join("modules")
            .join(artifact().manifest.sha256)
            .join(artifact::format::MODULE_NAME);
        fs::set_permissions(&module, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&module, b"changed").unwrap();
        let mut checks = Checks::default();
        assert!(recover_install_at(&paths, &mut checks).is_err());
        assert_eq!(checks.owned_calls, 0);
        assert!(!paths.state.join(transaction::LAUNCHER).exists());
        assert!(!paths.target.join(transaction::DROP_IN).exists());
        assert_eq!(paths.inspect().unwrap(), ActivationFilesState::Pending);
    }

    #[test]
    fn pending_install_recovery_preserves_authority_on_service_or_namespace_refusal() {
        struct Refuse {
            calls: usize,
            refuse_at: usize,
            replace: Option<PathBuf>,
        }
        impl ServiceChecks for Refuse {
            fn stock(&mut self) -> Result<(), ServiceRefusal> {
                panic!("not stock recovery")
            }
            fn owned(
                &mut self,
                owned: &OwnedActivation,
            ) -> Result<DefinitionState, ServiceRefusal> {
                owned.recheck().unwrap();
                self.calls += 1;
                if self.calls == self.refuse_at {
                    Err(ServiceRefusal::Changed)
                } else {
                    Ok(DefinitionState::NeedsReload)
                }
            }
            fn after_activation_lock(&mut self) {
                if let Some(path) = self.replace.take() {
                    fs::rename(&path, path.with_extension("old")).unwrap();
                    fs::create_dir(&path).unwrap();
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
                }
            }
        }
        for refused in 1..=3 {
            let (_root, paths) = fixture();
            interrupted_install(&paths, 1);
            let mut checks = Refuse {
                calls: 0,
                refuse_at: refused,
                replace: (refused == 3).then(|| paths.state.clone()),
            };
            let error = recover_install_at(&paths, &mut checks).unwrap_err();
            assert_eq!(
                error.stage,
                if refused == 1 {
                    Stage::Preflight
                } else {
                    Stage::Applying
                }
            );
            if refused < 3 {
                assert!(matches!(
                    error.cause,
                    Cause::Service(ServiceRefusal::Changed)
                ));
                assert_eq!(checks.calls, refused);
                assert_eq!(paths.inspect().unwrap(), ActivationFilesState::Pending);
            } else {
                assert_eq!(checks.calls, 1);
                assert_eq!(fs::read_dir(&paths.state).unwrap().count(), 0);
                assert!(
                    paths
                        .state
                        .with_extension("old")
                        .join("installation-pending-v1.json")
                        .exists()
                );
            }
            assert!(!paths.state.join(transaction::LAUNCHER).exists());
            assert!(!paths.target.join(transaction::DROP_IN).exists());
        }
    }

    #[test]
    fn pending_install_recovery_refuses_occupied_slots_and_competing_intents() {
        for kind in ["occupied", "other-intent", "edited-journal"] {
            let (_root, paths) = fixture();
            interrupted_install(&paths, 1);
            match kind {
                "occupied" => {
                    fs::write(paths.target.join(transaction::DROP_IN), b"foreign").unwrap()
                }
                "other-intent" => {
                    fs::write(paths.state.join("uninstall-pending-v1.json"), b"{}").unwrap()
                }
                _ => fs::write(paths.state.join("installation-pending-v1.json"), b"{}").unwrap(),
            }
            let mut checks = Checks::default();
            assert!(recover_install_at(&paths, &mut checks).is_err(), "{kind}");
            assert_eq!(checks.owned_calls, 0);
            assert!(!paths.state.join(transaction::LAUNCHER).exists());
            if kind == "occupied" {
                assert_eq!(
                    fs::read(paths.target.join(transaction::DROP_IN)).unwrap(),
                    b"foreign"
                );
            }
        }
    }

    #[test]
    fn lifecycle_update_then_remove_keeps_versions_and_unrelated_files() {
        let (_root, paths) = fixture();
        let old = artifact();
        let new = other_artifact();
        let mut checks = Checks {
            stock_calls: 0,
            owned_calls: 0,
            refuse_at: 0,
        };
        prepare_at(&paths, &old, &mut checks).unwrap();
        fs::write(paths.target.join("preferences.txt"), b"user settings").unwrap();
        update_at(&paths, &new, &mut checks).unwrap();
        let expected = paths
            .state
            .join("modules")
            .join(&new.manifest.sha256)
            .join(artifact::format::MODULE_NAME);
        assert_eq!(
            OwnedActivation::open(&paths.state, &paths.target)
                .unwrap()
                .module_path(),
            expected
        );
        let inode = fs::metadata(paths.state.join(transaction::LAUNCHER))
            .unwrap()
            .ino();
        update_at(&paths, &new, &mut checks).unwrap();
        assert_eq!(
            fs::metadata(paths.state.join(transaction::LAUNCHER))
                .unwrap()
                .ino(),
            inode
        );
        remove_at(&paths, &mut checks).unwrap();
        assert_eq!(paths.inspect().unwrap(), ActivationFilesState::Absent);
        assert_eq!(
            fs::read(paths.target.join("preferences.txt")).unwrap(),
            b"user settings"
        );
        for artifact in [&old, &new] {
            let path = paths
                .state
                .join("modules")
                .join(&artifact.manifest.sha256)
                .join(artifact::format::MODULE_NAME);
            assert_eq!(fs::read(path).unwrap(), artifact.bytes);
        }
    }

    #[test]
    fn rollback_uses_persisted_previous_version_without_a_payload_selector() {
        let (_root, paths) = fixture();
        let original = artifact();
        let updated = other_artifact();
        let mut checks = Checks {
            stock_calls: 0,
            owned_calls: 0,
            refuse_at: 0,
        };
        prepare_at(&paths, &original, &mut checks).unwrap();
        update_at(&paths, &updated, &mut checks).unwrap();
        let offer = rollback_offer_at(&paths).unwrap();
        rollback_at(&paths, &offer.revision, &mut checks).unwrap();
        let owned = OwnedActivation::open(&paths.state, &paths.target).unwrap();
        assert_eq!(
            owned.module_path(),
            paths
                .state
                .join("modules")
                .join(&original.manifest.sha256)
                .join(artifact::format::MODULE_NAME)
        );
        assert_eq!(
            owned.previous_module(),
            Some(updated.manifest.sha256.as_str())
        );
    }

    #[test]
    fn rollback_rejects_duplicate_and_aba_requests_without_reversing_the_committed_result() {
        let (_root, paths) = fixture();
        let original = artifact();
        let updated = other_artifact();
        let mut checks = Checks {
            stock_calls: 0,
            owned_calls: 0,
            refuse_at: 0,
        };
        prepare_at(&paths, &original, &mut checks).unwrap();
        update_at(&paths, &updated, &mut checks).unwrap();
        let old_offer = rollback_offer_at(&paths).unwrap();
        rollback_at(&paths, &old_offer.revision, &mut checks).unwrap();
        for repeat in [false, true] {
            if repeat {
                let fresh_offer = rollback_offer_at(&paths).unwrap();
                rollback_at(&paths, &fresh_offer.revision, &mut checks).unwrap();
                assert_eq!(
                    OwnedActivation::open(&paths.state, &paths.target)
                        .unwrap()
                        .module_digest(),
                    updated.manifest.sha256
                );
            }
            let before = fs::read(paths.state.join(transaction::LAUNCHER)).unwrap();
            let calls = (checks.stock_calls, checks.owned_calls);
            assert!(matches!(
                rollback_at(&paths, &old_offer.revision, &mut checks)
                    .unwrap_err()
                    .cause,
                Cause::StaleRevision
            ));
            assert_eq!((checks.stock_calls, checks.owned_calls), calls);
            assert_eq!(
                fs::read(paths.state.join(transaction::LAUNCHER)).unwrap(),
                before
            );
        }
    }

    #[test]
    fn rollback_rejects_revision_changed_after_lock_before_commit() {
        let (_root, paths) = fixture();
        let mut checks = Checks {
            stock_calls: 0,
            owned_calls: 0,
            refuse_at: 0,
        };
        prepare_at(&paths, &artifact(), &mut checks).unwrap();
        update_at(&paths, &other_artifact(), &mut checks).unwrap();
        let offer = rollback_offer_at(&paths).unwrap();
        let before = fs::read(paths.state.join(transaction::LAUNCHER)).unwrap();
        struct Touching<'a>(&'a Paths, &'a str);
        impl ServiceChecks for Touching<'_> {
            fn stock(&mut self) -> Result<(), ServiceRefusal> {
                unreachable!()
            }
            fn owned(
                &mut self,
                owned: &OwnedActivation,
            ) -> Result<DefinitionState, ServiceRefusal> {
                owned.recheck().unwrap();
                Ok(DefinitionState::NeedsReload)
            }
            fn after_activation_lock(&mut self) {
                let path = self.0.state.join("installation-v1.json");
                fs::write(&path, fs::read(&path).unwrap()).unwrap();
                assert_ne!(
                    OwnedActivation::open(&self.0.state, &self.0.target)
                        .unwrap()
                        .revision()
                        .unwrap(),
                    self.1
                );
            }
        }
        let error = rollback_at(
            &paths,
            &offer.revision,
            &mut Touching(&paths, &offer.revision),
        )
        .unwrap_err();
        assert!(matches!(error.cause, Cause::StaleRevision));
        assert_eq!(
            fs::read(paths.state.join(transaction::LAUNCHER)).unwrap(),
            before
        );
    }

    #[test]
    fn rollback_keeps_history_on_idempotent_update_and_can_restore_over_bad_current_data() {
        let (_root, paths) = fixture();
        let original = artifact();
        let updated = other_artifact();
        let mut checks = Checks {
            stock_calls: 0,
            owned_calls: 0,
            refuse_at: 0,
        };
        prepare_at(&paths, &original, &mut checks).unwrap();
        update_at(&paths, &updated, &mut checks).unwrap();
        let before = fs::read(paths.state.join(transaction::LAUNCHER)).unwrap();
        update_at(&paths, &updated, &mut checks).unwrap();
        assert_eq!(
            fs::read(paths.state.join(transaction::LAUNCHER)).unwrap(),
            before
        );
        let current = paths
            .state
            .join("modules")
            .join(&updated.manifest.sha256)
            .join(artifact::format::MODULE_NAME);
        fs::set_permissions(&current, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&current, b"modified current module").unwrap();
        let offer = rollback_offer_at(&paths).unwrap();
        rollback_at(&paths, &offer.revision, &mut checks).unwrap();
        assert_eq!(fs::read(current).unwrap(), b"modified current module");
        assert_eq!(
            OwnedActivation::open(&paths.state, &paths.target)
                .unwrap()
                .module_digest(),
            original.manifest.sha256
        );
    }

    #[test]
    fn rollback_refuses_absent_history_bad_target_and_pending_state_before_service_calls() {
        for kind in ["no-history", "bad-target", "pending"] {
            let (_root, paths) = fixture();
            let original = artifact();
            let updated = other_artifact();
            let mut checks = Checks {
                stock_calls: 0,
                owned_calls: 0,
                refuse_at: 0,
            };
            prepare_at(&paths, &original, &mut checks).unwrap();
            if kind != "no-history" {
                update_at(&paths, &updated, &mut checks).unwrap();
            }
            if kind == "bad-target" {
                let old = paths
                    .state
                    .join("modules")
                    .join(&original.manifest.sha256)
                    .join(artifact::format::MODULE_NAME);
                fs::set_permissions(&old, fs::Permissions::from_mode(0o600)).unwrap();
                fs::write(old, b"changed rollback target").unwrap();
            }
            if kind == "pending" {
                fs::write(paths.state.join("installation-pending-v1.json"), b"pending").unwrap();
            }
            let before = fs::read(paths.state.join(transaction::LAUNCHER)).unwrap();
            let calls = (checks.stock_calls, checks.owned_calls);
            let revision = OwnedActivation::open(&paths.state, &paths.target)
                .and_then(|owned| owned.revision())
                .unwrap_or_default();
            assert!(
                rollback_at(&paths, &revision, &mut checks).is_err(),
                "{kind}"
            );
            assert_eq!((checks.stock_calls, checks.owned_calls), calls);
            assert_eq!(
                fs::read(paths.state.join(transaction::LAUNCHER)).unwrap(),
                before
            );
        }
    }

    #[test]
    fn removal_does_not_require_an_intact_module_and_absent_retry_does_not_query_service() {
        let (_root, paths) = fixture();
        let old = artifact();
        let mut checks = Checks {
            stock_calls: 0,
            owned_calls: 0,
            refuse_at: 0,
        };
        prepare_at(&paths, &old, &mut checks).unwrap();
        let module = paths
            .state
            .join("modules")
            .join(&old.manifest.sha256)
            .join(artifact::format::MODULE_NAME);
        fs::remove_file(module).unwrap();
        remove_at(&paths, &mut checks).unwrap();
        let calls = (checks.stock_calls, checks.owned_calls);
        remove_at(&paths, &mut checks).unwrap();
        assert_eq!((checks.stock_calls, checks.owned_calls), calls);
        assert_eq!(paths.inspect().unwrap(), ActivationFilesState::Absent);
        assert!(paths.state.join("modules").is_dir());
    }

    #[test]
    fn removal_retains_factory_authority_and_blocks_reinstall_until_manager_sync() {
        let (_root, paths) = fixture();
        let artifact = artifact();
        let mut checks = Checks::default();
        prepare_at(&paths, &artifact, &mut checks).unwrap();
        let expected = OwnedActivation::open(&paths.state, &paths.target)
            .unwrap()
            .module_path()
            .to_owned();
        remove_at(&paths, &mut checks).unwrap();
        let retired = OwnedActivation::open_retired(&paths.state, &paths.target).unwrap();
        assert_eq!(retired.module_path(), expected);
        retired.recheck().unwrap();
        let calls = (checks.stock_calls, checks.owned_calls);
        assert!(prepare_at(&paths, &artifact, &mut checks).is_err());
        assert_eq!((checks.stock_calls, checks.owned_calls), calls);
        assert!(!paths.state.join(transaction::LAUNCHER).exists());
        assert!(!paths.target.join(transaction::DROP_IN).exists());
    }

    #[test]
    fn pending_removal_recovery_finishes_all_prefixes_without_module_or_bundle() {
        for point in 12..=15 {
            let (_root, paths) = fixture();
            let mut checks = Checks::default();
            let artifact = artifact();
            prepare_at(&paths, &artifact, &mut checks).unwrap();
            let active = ExistingActivation::open(&paths).unwrap();
            {
                let mut transaction =
                    ActivationTransaction::open_complete(&active.state, &active.target).unwrap();
                transaction.interrupt_removal_after(point, false);
                assert!(
                    transaction
                        .retire_activation(
                            &paths.state,
                            active.owned.module_digest(),
                            active.owned.previous_module()
                        )
                        .is_err()
                );
            }
            fs::remove_file(active.owned.module_path()).unwrap();
            let mut recovery = Checks::default();
            recover_removal_at(&paths, &mut recovery).unwrap();
            assert_eq!(recovery.stock_calls, 0);
            assert_eq!(recovery.owned_calls, 3);
            assert_eq!(paths.inspect().unwrap(), ActivationFilesState::Absent);
            OwnedActivation::open_retired(&paths.state, &paths.target)
                .unwrap()
                .recheck()
                .unwrap();
            recover_removal_at(&paths, &mut recovery).unwrap();
            assert_eq!(recovery.owned_calls, 6);
        }
    }

    fn interrupted_removal(paths: &Paths, point: u8) {
        let mut checks = Checks::default();
        prepare_at(paths, &artifact(), &mut checks).unwrap();
        let active = ExistingActivation::open(paths).unwrap();
        let mut transaction =
            ActivationTransaction::open_complete(&active.state, &active.target).unwrap();
        transaction.interrupt_removal_after(point, false);
        assert!(
            transaction
                .retire_activation(
                    &paths.state,
                    active.owned.module_digest(),
                    active.owned.previous_module()
                )
                .is_err()
        );
    }

    #[test]
    fn pending_removal_recovery_refuses_changed_authority_and_survivors_before_service() {
        for kind in ["reference", "survivor", "retired", "other-intent"] {
            let (_root, paths) = fixture();
            interrupted_removal(&paths, 13);
            let journal = paths.state.join("uninstall-pending-v1.json");
            match kind {
                "reference" => {
                    let mut body: serde_json::Value =
                        serde_json::from_slice(&fs::read(&journal).unwrap()).unwrap();
                    body["activation"]["digest"] = "b".repeat(64).into();
                    fs::write(&journal, serde_json::to_vec(&body).unwrap()).unwrap();
                }
                "survivor" => {
                    fs::write(paths.state.join(transaction::LAUNCHER), b"foreign").unwrap()
                }
                "retired" => {
                    fs::write(paths.state.join(transaction::RETIRED_REMOVAL), b"foreign").unwrap()
                }
                _ => fs::write(paths.state.join("installation-pending-v1.json"), b"{}").unwrap(),
            }
            let journal_before = fs::read(&journal).unwrap();
            let launcher_before = fs::read(paths.state.join(transaction::LAUNCHER)).unwrap();
            let mut checks = Checks::default();
            assert!(recover_removal_at(&paths, &mut checks).is_err(), "{kind}");
            assert_eq!(checks.owned_calls, 0);
            assert_eq!(fs::read(&journal).unwrap(), journal_before);
            assert_eq!(
                fs::read(paths.state.join(transaction::LAUNCHER)).unwrap(),
                launcher_before
            );
            assert!(paths.state.join("installation-v1.json").is_file());
        }
    }

    #[test]
    fn pending_removal_recovery_preserves_survivors_on_late_service_or_reappearance() {
        struct Refuse {
            calls: usize,
            recreate: Option<PathBuf>,
        }
        impl ServiceChecks for Refuse {
            fn stock(&mut self) -> Result<(), ServiceRefusal> {
                panic!("not stock recovery")
            }
            fn owned(
                &mut self,
                owned: &OwnedActivation,
            ) -> Result<DefinitionState, ServiceRefusal> {
                owned.recheck().unwrap();
                self.calls += 1;
                if self.calls == 2 {
                    Err(ServiceRefusal::Changed)
                } else {
                    Ok(DefinitionState::NeedsReload)
                }
            }
            fn after_activation_lock(&mut self) {
                if let Some(path) = self.recreate.take() {
                    fs::write(path, b"foreign override").unwrap();
                }
            }
        }
        for recreate in [false, true] {
            let (_root, paths) = fixture();
            interrupted_removal(&paths, 13);
            let before = fs::read(paths.state.join(transaction::LAUNCHER)).unwrap();
            let mut checks = Refuse {
                calls: 0,
                recreate: recreate.then(|| paths.target.join(transaction::DROP_IN)),
            };
            let error = recover_removal_at(&paths, &mut checks).unwrap_err();
            assert_eq!(error.stage, Stage::Applying);
            assert_eq!(checks.calls, if recreate { 1 } else { 2 });
            assert_eq!(
                fs::read(paths.state.join(transaction::LAUNCHER)).unwrap(),
                before
            );
            assert!(paths.state.join("uninstall-pending-v1.json").is_file());
            assert!(!paths.state.join(transaction::RETIRED_REMOVAL).exists());
        }
    }

    #[test]
    fn retired_authority_refuses_changed_payload_namespace_and_recreated_roles() {
        for kind in ["payload", "namespace", "role", "pending", "schema"] {
            let (_root, paths) = fixture();
            let mut checks = Checks::default();
            prepare_at(&paths, &artifact(), &mut checks).unwrap();
            remove_at(&paths, &mut checks).unwrap();
            let retired = OwnedActivation::open_retired(&paths.state, &paths.target).unwrap();
            let record = paths.state.join(transaction::RETIRED_REMOVAL);
            match kind {
                "payload" | "schema" => {
                    let mut json: serde_json::Value =
                        serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
                    if kind == "payload" {
                        json["activation"]["digest"] = "b".repeat(64).into();
                    } else {
                        json["unexpected"] = true.into();
                    }
                    fs::write(&record, serde_json::to_vec(&json).unwrap()).unwrap();
                }
                "namespace" => {
                    fs::rename(&paths.target, paths.target.with_extension("old")).unwrap();
                    fs::create_dir(&paths.target).unwrap();
                    fs::set_permissions(&paths.target, fs::Permissions::from_mode(0o700)).unwrap();
                }
                "pending" => {
                    fs::write(paths.state.join("installation-pending-v1.json"), b"{}").unwrap()
                }
                _ => fs::write(paths.state.join(transaction::LAUNCHER), b"foreign").unwrap(),
            }
            assert!(retired.recheck().is_err(), "{kind}");
            assert!(
                OwnedActivation::open_retired(&paths.state, &paths.target).is_err(),
                "{kind}"
            );
        }
    }

    #[test]
    fn retirement_is_representable_for_long_quote_heavy_supported_paths() {
        let (root, mut paths) = fixture();
        let mut state = root.path().to_owned();
        for _ in 0..14 {
            state.push("'".repeat(220));
        }
        paths.state = state.join("state");
        let mut checks = Checks::default();
        prepare_at(&paths, &artifact(), &mut checks).unwrap();
        let launcher = fs::read_to_string(paths.state.join(transaction::LAUNCHER)).unwrap();
        let drop_in = fs::read_to_string(paths.target.join(transaction::DROP_IN)).unwrap();
        assert!(launcher.len() <= super::super::managed::MAX_CONTENT);
        assert!(drop_in.len() <= super::super::managed::MAX_CONTENT);
        // Duplicating these valid texts would exceed the shared journal cap.
        let duplicated_length = serde_json::to_vec(&(launcher, drop_in)).unwrap().len();
        assert!(
            duplicated_length > super::super::managed::MAX_CONTENT,
            "{duplicated_length}"
        );
        remove_at(&paths, &mut checks).unwrap();
        assert!(
            fs::read(paths.state.join(transaction::RETIRED_REMOVAL))
                .unwrap()
                .len()
                < 4096
        );
        OwnedActivation::open_retired(&paths.state, &paths.target)
            .unwrap()
            .recheck()
            .unwrap();
    }

    #[test]
    fn changed_old_module_refuses_update_before_new_data_or_service_calls() {
        let (_root, paths) = fixture();
        let old = artifact();
        let new = other_artifact();
        let mut checks = Checks {
            stock_calls: 0,
            owned_calls: 0,
            refuse_at: 0,
        };
        prepare_at(&paths, &old, &mut checks).unwrap();
        let original = fs::read(paths.state.join(transaction::LAUNCHER)).unwrap();
        let module = paths
            .state
            .join("modules")
            .join(&old.manifest.sha256)
            .join(artifact::format::MODULE_NAME);
        fs::set_permissions(&module, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&module, b"user changed module").unwrap();
        fs::set_permissions(&module, fs::Permissions::from_mode(0o400)).unwrap();
        let calls = (checks.stock_calls, checks.owned_calls);
        assert!(update_at(&paths, &new, &mut checks).is_err());
        assert_eq!((checks.stock_calls, checks.owned_calls), calls);
        assert!(
            !paths
                .state
                .join("modules")
                .join(&new.manifest.sha256)
                .exists()
        );
        assert_eq!(
            fs::read(paths.state.join(transaction::LAUNCHER)).unwrap(),
            original
        );
        // Removal preserves this changed module too; only activation is retired.
        remove_at(&paths, &mut checks).unwrap();
        assert_eq!(fs::read(module).unwrap(), b"user changed module");
    }

    #[test]
    fn pending_activation_is_not_recovered_by_update_or_removal() {
        let (_root, paths) = fixture();
        let mut checks = Checks {
            stock_calls: 0,
            owned_calls: 0,
            refuse_at: 0,
        };
        prepare_at(&paths, &artifact(), &mut checks).unwrap();
        let marker = paths.state.join("installation-pending-v1.json");
        fs::write(&marker, b"unknown pending authority").unwrap();
        let original = fs::read(paths.state.join(transaction::LAUNCHER)).unwrap();
        let calls = (checks.stock_calls, checks.owned_calls);
        assert!(update_at(&paths, &other_artifact(), &mut checks).is_err());
        assert!(remove_at(&paths, &mut checks).is_err());
        assert_eq!((checks.stock_calls, checks.owned_calls), calls);
        assert_eq!(fs::read(marker).unwrap(), b"unknown pending authority");
        assert_eq!(
            fs::read(paths.state.join(transaction::LAUNCHER)).unwrap(),
            original
        );
    }

    #[test]
    fn coordinator_prepares_exact_module_and_activation_with_two_service_checks() {
        let (_root, paths) = fixture();
        let artifact = artifact();
        let mut checks = Checks {
            stock_calls: 0,
            owned_calls: 0,
            refuse_at: 0,
        };
        assert_eq!(
            prepare_at(&paths, &artifact, &mut checks)
                .unwrap()
                .definition,
            DefinitionState::NeedsReload
        );
        assert_eq!((checks.stock_calls, checks.owned_calls), (2, 1));
        assert_eq!(paths.inspect().unwrap(), ActivationFilesState::Owned);
        let module = paths
            .state
            .join("modules")
            .join(&artifact.manifest.sha256)
            .join(artifact::format::MODULE_NAME);
        assert_eq!(fs::read(module).unwrap(), artifact.bytes);
    }

    #[test]
    fn stock_refusal_precedes_disk_writes_and_late_refusal_preserves_prepared_data() {
        for refuse_at in [1, 2] {
            let (root, paths) = fixture();
            let mut checks = Checks {
                stock_calls: 0,
                owned_calls: 0,
                refuse_at,
            };
            let error = prepare_at(&paths, &artifact(), &mut checks).unwrap_err();
            assert!(matches!(
                error.cause,
                Cause::Service(ServiceRefusal::Changed)
            ));
            assert_eq!(checks.stock_calls, refuse_at);
            assert_eq!(checks.owned_calls, 0);
            assert!(!paths.state.join(transaction::LAUNCHER).exists());
            assert!(!paths.target.join(transaction::DROP_IN).exists());
            if refuse_at == 1 {
                assert_eq!(error.stage, Stage::Preflight);
                assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
            } else {
                assert_eq!(error.stage, Stage::Applying);
                assert!(paths.state.join("modules").is_dir());
                assert_eq!(paths.inspect().unwrap(), ActivationFilesState::Absent);
            }
        }
    }

    #[test]
    fn foreign_activation_and_invalid_bundle_refuse_before_service_or_writes() {
        for invalid in [false, true] {
            let (root, paths) = fixture();
            let mut artifact = artifact();
            if invalid {
                artifact.manifest.protocol = 2;
            } else {
                directories::open_drop_ins(&paths.target, true).unwrap();
                fs::write(paths.target.join(transaction::DROP_IN), b"user override").unwrap();
            }
            let mut checks = Checks {
                stock_calls: 0,
                owned_calls: 0,
                refuse_at: 0,
            };
            assert!(prepare_at(&paths, &artifact, &mut checks).is_err());
            assert_eq!((checks.stock_calls, checks.owned_calls), (0, 0));
            assert!(!paths.state.exists());
            if invalid {
                assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
            } else {
                assert_eq!(
                    fs::read(paths.target.join(transaction::DROP_IN)).unwrap(),
                    b"user override"
                );
            }
        }
    }

    #[test]
    fn module_directory_replacement_during_second_probe_refuses_before_activation() {
        let (_root, paths) = fixture();
        struct Replacing<'a>(&'a Paths);
        impl ServiceChecks for Replacing<'_> {
            fn stock(&mut self) -> Result<(), ServiceRefusal> {
                let modules = self.0.state.join("modules");
                if modules.exists() {
                    fs::rename(&modules, self.0.state.join("original-modules")).unwrap();
                    fs::create_dir(&modules).unwrap();
                    fs::set_permissions(modules, fs::Permissions::from_mode(0o700)).unwrap();
                }
                Ok(())
            }
            fn owned(
                &mut self,
                _owned: &OwnedActivation,
            ) -> Result<DefinitionState, ServiceRefusal> {
                panic!("activation must not be published");
            }
        }
        let error = prepare_at(&paths, &artifact(), &mut Replacing(&paths)).unwrap_err();
        assert_eq!(error.stage, Stage::Applying);
        assert!(!paths.state.join(transaction::LAUNCHER).exists());
        assert_eq!(
            fs::read_dir(paths.state.join("modules")).unwrap().count(),
            0
        );
        assert!(paths.state.join("original-modules").is_dir());
    }

    #[test]
    fn state_replacement_after_activation_lock_receives_no_module_or_lock_writes() {
        let (root, paths) = fixture();
        struct Replacing<'a>(&'a Paths, &'a Path);
        impl ServiceChecks for Replacing<'_> {
            fn stock(&mut self) -> Result<(), ServiceRefusal> {
                Ok(())
            }
            fn owned(
                &mut self,
                _owned: &OwnedActivation,
            ) -> Result<DefinitionState, ServiceRefusal> {
                panic!("must refuse before activation");
            }
            fn after_activation_lock(&mut self) {
                fs::rename(&self.0.state, self.1.join("original-state")).unwrap();
                fs::create_dir(&self.0.state).unwrap();
                fs::set_permissions(&self.0.state, fs::Permissions::from_mode(0o700)).unwrap();
            }
        }
        assert!(prepare_at(&paths, &artifact(), &mut Replacing(&paths, root.path())).is_err());
        assert_eq!(fs::read_dir(&paths.state).unwrap().count(), 0);
        assert!(!paths.target.join(transaction::DROP_IN).exists());
    }

    #[test]
    fn postflight_refusal_reports_effects_without_discarding_owned_activation() {
        let (_root, paths) = fixture();
        struct Refusing;
        impl ServiceChecks for Refusing {
            fn stock(&mut self) -> Result<(), ServiceRefusal> {
                Ok(())
            }
            fn owned(
                &mut self,
                _owned: &OwnedActivation,
            ) -> Result<DefinitionState, ServiceRefusal> {
                Err(ServiceRefusal::Changed)
            }
        }
        let error = prepare_at(&paths, &artifact(), &mut Refusing).unwrap_err();
        assert_eq!(error.stage, Stage::Verifying);
        assert!(matches!(
            error.cause,
            Cause::Service(ServiceRefusal::Changed)
        ));
        assert_eq!(paths.inspect().unwrap(), ActivationFilesState::Owned);
    }

    #[cfg(not(lyn_gnome_native_embedded))]
    #[test]
    fn no_embedded_artifact_refuses_without_disk_or_bus_access() {
        let (root, _paths) = fixture();
        assert!(matches!(
            prepare_first(root.path()).unwrap_err().cause,
            Cause::NoArtifact
        ));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
