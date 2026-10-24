//! Advisory action offers from existing authority and read-only retained bytes.

use super::{artifact, service::OwnedActivation};
use crate::contract::{GnomeActivationPhase as Phase, GnomeManagementAction as Action};

pub(super) fn actions(owned: &OwnedActivation, phase: Phase) -> Vec<Action> {
    match phase {
        Phase::Configured => {
            let mut actions = Vec::new();
            if owned.retained_module_available(owned.module_digest()) {
                if artifact::embedded()
                    .ok()
                    .flatten()
                    .is_some_and(|artifact| artifact.manifest.sha256 != owned.module_digest())
                {
                    actions.push(Action::Update);
                }
                actions.push(Action::Synchronize);
            }
            if owned
                .previous_module()
                .is_some_and(|digest| owned.retained_module_available(digest))
            {
                actions.push(Action::Rollback);
            }
            actions.push(Action::Uninstall);
            actions
        }
        Phase::Installing | Phase::FinishingUpdate | Phase::RestoringUpdate => {
            if !owned.retained_module_available(owned.module_digest()) {
                return Vec::new();
            }
            vec![match phase {
                Phase::Installing => Action::RecoverInstall,
                Phase::FinishingUpdate => Action::FinishUpdate,
                Phase::RestoringUpdate => Action::RestoreUpdate,
                _ => unreachable!(),
            }]
        }
        Phase::Removing => vec![Action::Uninstall],
        Phase::AwaitingRemovalConfirmation => vec![Action::ConfirmRemoval],
        _ => Vec::new(),
    }
}
