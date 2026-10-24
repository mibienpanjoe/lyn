//! Caller freshness is checked again under the mutation's installation lock.

use super::*;

pub(super) fn require_management_revision(
    owned: &OwnedActivation,
    expected: Option<&str>,
    stage: Stage,
) -> Result<(), Failure> {
    if let Some(expected) = expected {
        if owned
            .management_revision()
            .map_err(|error| failure(stage, Cause::Io(error)))?
            != expected
        {
            return Err(failure(stage, Cause::StaleRevision));
        }
    }
    Ok(())
}

pub(super) fn check_service_revision(
    service: &impl ServiceChecks,
    owned: &OwnedActivation,
    stage: Stage,
) -> Result<(), Failure> {
    // Postflight inspects the new configuration; it must not compare it to the
    // request's displaced authority. All pre-mutation stages remain checked.
    if stage != Stage::Verifying {
        require_management_revision(owned, service.expected_revision(), stage)?;
    }
    Ok(())
}

pub(super) struct WithRevision<S> {
    pub(super) inner: S,
    pub(super) expected: String,
}

impl<S: ServiceChecks> ServiceChecks for WithRevision<S> {
    fn expected_revision(&self) -> Option<&str> {
        Some(&self.expected)
    }
    fn stock(&mut self) -> Result<(), ServiceRefusal> {
        self.inner.stock()
    }
    fn owned(&mut self, owned: &OwnedActivation) -> Result<DefinitionState, ServiceRefusal> {
        self.inner.owned(owned)
    }
    #[cfg(test)]
    fn after_activation_lock(&mut self) {
        self.inner.after_activation_lock();
    }
    #[cfg(test)]
    fn before_retire(&mut self, transaction: &mut ActivationTransaction<'_>) {
        self.inner.before_retire(transaction);
    }
    #[cfg(test)]
    fn before_finish_update(&mut self, transaction: &mut ActivationTransaction<'_>) {
        self.inner.before_finish_update(transaction);
    }
    #[cfg(test)]
    fn before_update(&mut self, transaction: &mut ActivationTransaction<'_>) {
        self.inner.before_update(transaction);
    }
}
