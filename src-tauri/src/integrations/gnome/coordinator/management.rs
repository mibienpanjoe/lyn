//! Closed path-free management of already recorded GNOME activation authority.

use super::*;
use crate::contract::{
    GnomeManagementAction as Action, ManageGnomeActivationInput, ManageGnomeActivationResult,
};

pub(in crate::integrations::gnome) fn manage(
    home: &Path,
    input: ManageGnomeActivationInput,
) -> Result<ManageGnomeActivationResult, Failure> {
    if !input.valid_revision() {
        return Err(failure(Stage::Preflight, Cause::StaleRevision));
    }
    manage_at(&Paths::from_home(home)?, input)
}

fn manage_at(
    paths: &Paths,
    input: ManageGnomeActivationInput,
) -> Result<ManageGnomeActivationResult, Failure> {
    let mut service = WithRevision {
        inner: RealService {
            target: paths.target.clone(),
        },
        expected: input.expected_revision,
    };
    match input.action {
        Action::Update => {
            let artifact = artifact::embedded()
                .map_err(|_| failure(Stage::Preflight, Cause::NoArtifact))?
                .ok_or_else(|| failure(Stage::Preflight, Cause::NoArtifact))?;
            update_at(paths, &artifact, &mut service)?;
        }
        Action::Rollback => {
            rollback_at(paths, &service.expected.clone(), &mut service)?;
        }
        Action::Remove => {
            remove_at(paths, &mut service)?;
        }
        Action::Uninstall => {
            let removed = match paths
                .inspect()
                .map_err(|error| failure(Stage::Preflight, Cause::Io(error)))?
            {
                ActivationFilesState::Owned => remove_at(paths, &mut service)?,
                ActivationFilesState::Pending => recover_removal_at(paths, &mut service)?,
                ActivationFilesState::Absent => {
                    acknowledge_removal_at_revision(paths, Some(&service.expected))?;
                    let files = paths.inspect().unwrap_or(ActivationFilesState::Unavailable);
                    return Ok(ManageGnomeActivationResult {
                        action: input.action,
                        activation: status::activation(&paths.state, &paths.target, files),
                    });
                }
                _ => return Err(failure(Stage::Preflight, Cause::Conflict)),
            };
            // Captured while removal still held its lock, never obtained from
            // the later advisory status read or an unrelated new authority.
            let retired = removed
                .revision
                .ok_or_else(|| failure(Stage::Applying, Cause::Conflict))?;
            acknowledge_removal_at_revision(paths, Some(&retired))?;
        }
        Action::RecoverInstall => {
            recover_install_at(paths, &mut service)?;
        }
        Action::FinishUpdate => {
            recover_committed_update_at(paths, &mut service)?;
        }
        Action::RestoreUpdate => {
            recover_uncommitted_update_at(paths, &mut service)?;
        }
        Action::RecoverRemoval => {
            recover_removal_at(paths, &mut service)?;
        }
        Action::Synchronize => {
            synchronize_at_revision(paths, Some(&service.expected))?;
        }
        Action::ConfirmRemoval => {
            acknowledge_removal_at_revision(paths, Some(&service.expected))?;
        }
    }
    let files = paths.inspect().unwrap_or(ActivationFilesState::Unavailable);
    Ok(ManageGnomeActivationResult {
        action: input.action,
        activation: status::activation(&paths.state, &paths.target, files),
    })
}

#[cfg(test)]
pub(in crate::integrations::gnome) fn manage_private(
    root: &Path,
    input: ManageGnomeActivationInput,
) -> Result<ManageGnomeActivationResult, Failure> {
    assert!(input.valid_revision());
    manage_at(
        &Paths {
            state: root.join("state"),
            target: root.join("units/gnome-terminal-server.service.d"),
        },
        input,
    )
}
