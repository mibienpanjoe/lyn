//! First-install file preparation. No service reload/start/stop or UI paths.

#![allow(dead_code)] // Application command wiring follows private acceptance.

use super::{
    artifact::{self, Artifact},
    directories,
    managed::ManagedDirectory,
    module_store::ModuleStore,
    render_activation,
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

#[derive(Debug, PartialEq, Eq)]
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

trait ServiceChecks {
    fn stock(&mut self) -> Result<(), ServiceRefusal>;
    fn owned(&mut self, owned: &OwnedActivation) -> Result<DefinitionState, ServiceRefusal>;
    #[cfg(test)]
    fn after_activation_lock(&mut self) {}
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

fn prepare_at(
    paths: &Paths,
    artifact: &Artifact,
    service: &mut impl ServiceChecks,
) -> Result<Prepared, Failure> {
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
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

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
