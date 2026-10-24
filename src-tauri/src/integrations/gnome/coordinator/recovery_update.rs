//! Recover the recorded update decision, without replaying another intent.

use super::*;

pub(super) fn recover_committed_update_at(
    paths: &Paths,
    service: &mut impl ServiceChecks,
) -> Result<Prepared, Failure> {
    recover_update_at(paths, service, Decision::Committed)
}

pub(super) fn recover_uncommitted_update_at(
    paths: &Paths,
    service: &mut impl ServiceChecks,
) -> Result<Prepared, Failure> {
    recover_update_at(paths, service, Decision::Uncommitted)
}

#[derive(Clone, Copy)]
enum Decision {
    Committed,
    Uncommitted,
}

fn recover_update_at(
    paths: &Paths,
    service: &mut impl ServiceChecks,
    decision: Decision,
) -> Result<Prepared, Failure> {
    let preflight = |error| failure(Stage::Preflight, Cause::Io(error));
    let active = ExistingActivation {
        state: directories::open_private(&paths.state, false).map_err(preflight)?,
        target: directories::open_drop_ins(&paths.target, false).map_err(preflight)?,
        owned: match decision {
            Decision::Committed => {
                OwnedActivation::open_committed_update(&paths.state, &paths.target)
            }
            Decision::Uncommitted => {
                OwnedActivation::open_uncommitted_update(&paths.state, &paths.target)
            }
        }
        .map_err(preflight)?,
    };
    active.recheck(paths).map_err(preflight)?;
    let module = PreviousModule::open(paths, &active).map_err(preflight)?;
    owned_definition(service, &active.owned, Stage::Preflight)?;
    let mut check_failure = None;
    let check = || {
        #[cfg(test)]
        service.after_activation_lock();
        let checked = (|| {
            let applying = |error| failure(Stage::Applying, Cause::Io(error));
            active.recheck(paths).map_err(applying)?;
            module.recheck(paths, &active).map_err(applying)?;
            owned_definition(service, &active.owned, Stage::Applying)?;
            active.recheck(paths).map_err(applying)?;
            module.recheck(paths, &active).map_err(applying)
        })();
        if let Err(error) = checked {
            check_failure = Some(error);
            return Err(io::Error::other("update recovery preflight refused"));
        }
        Ok(())
    };
    let result = match decision {
        Decision::Committed => ActivationTransaction::resume_committed_update_checked(
            &active.state,
            &active.target,
            check,
        ),
        Decision::Uncommitted => ActivationTransaction::resume_uncommitted_update_checked(
            &active.state,
            &active.target,
            check,
        ),
    };
    let _transaction = result.map_err(|error| {
        check_failure.unwrap_or_else(|| failure(Stage::Applying, Cause::Io(error)))
    })?;
    let verifying = |error| failure(Stage::Verifying, Cause::Io(error));
    let current = OwnedActivation::open(&paths.state, &paths.target).map_err(verifying)?;
    if current.module_path() != active.owned.module_path()
        || current.previous_module() != active.owned.previous_module()
    {
        return Err(failure(Stage::Verifying, Cause::Conflict));
    }
    let definition = owned_definition(service, &current, Stage::Verifying)?;
    current.recheck().map_err(verifying)?;
    module.recheck(paths, &active).map_err(verifying)?;
    Ok(Prepared { definition })
}
