//! Rust-only contract for privacy-bounded local context providers.
#![allow(
    dead_code,
    reason = "provider contract is consumed by the T14 registry"
)]

use std::{
    fmt,
    path::{Path, PathBuf},
    time::Instant,
};

use uuid::Uuid;

use crate::{contract::ContextProviderKind, platform::WindowCorrelationToken};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CorrelationToken(Uuid);

impl CorrelationToken {
    pub(crate) fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub(crate) fn from_process_id(process_id: u32) -> Self {
        const PROCESS_NAMESPACE: u128 = 0x4c79_6e00_7072_6f63_0000_0000_0000_0000;
        Self(Uuid::from_u128(PROCESS_NAMESPACE | u128::from(process_id)))
    }

    pub(crate) fn from_session_id(session_id: Uuid) -> Self {
        Self(session_id)
    }

    pub(crate) fn from_terminal_id(terminal_id: u64) -> Self {
        const TERMINAL_NAMESPACE: u128 = 0x4c79_6e00_7465_726d_0000_0000_0000_0000;
        Self(Uuid::from_u128(
            TERMINAL_NAMESPACE | u128::from(terminal_id),
        ))
    }
}

impl fmt::Debug for CorrelationToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CorrelationToken(<opaque>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderSourceKind {
    VscodeWindow,
    VscodeIntegratedTerminal,
    ExternalTerminal,
    ShellSession,
    ForegroundWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObservationLiveness {
    Live,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderError {
    Unavailable,
}

pub(crate) struct ProviderObservation {
    provider: ContextProviderKind,
    source_kind: ProviderSourceKind,
    window: Option<WindowCorrelationToken>,
    process: Option<CorrelationToken>,
    session: Option<CorrelationToken>,
    directory: PathBuf,
    observed_at: Instant,
    liveness: ObservationLiveness,
}

impl ProviderObservation {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        provider: ContextProviderKind,
        source_kind: ProviderSourceKind,
        window: Option<WindowCorrelationToken>,
        process: Option<CorrelationToken>,
        session: Option<CorrelationToken>,
        directory: PathBuf,
        observed_at: Instant,
        liveness: ObservationLiveness,
    ) -> Self {
        Self {
            provider,
            source_kind,
            window,
            process,
            session,
            directory,
            observed_at,
            liveness,
        }
    }

    pub(crate) fn provider(&self) -> ContextProviderKind {
        self.provider
    }
    pub(crate) fn source_kind(&self) -> ProviderSourceKind {
        self.source_kind
    }
    pub(crate) fn window(&self) -> Option<WindowCorrelationToken> {
        self.window
    }
    pub(crate) fn process(&self) -> Option<CorrelationToken> {
        self.process
    }
    pub(crate) fn session(&self) -> Option<CorrelationToken> {
        self.session
    }
    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }
    pub(crate) fn observed_at(&self) -> Instant {
        self.observed_at
    }
    pub(crate) fn liveness(&self) -> ObservationLiveness {
        self.liveness
    }
}

pub(crate) trait ContextObservationProvider {
    fn observations(&mut self, now: Instant) -> Result<Vec<ProviderObservation>, ProviderError>;
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Instant};

    use crate::{contract::ContextProviderKind, platform::WindowCorrelationToken};

    use super::{
        ContextObservationProvider, CorrelationToken, ObservationLiveness, ProviderObservation,
        ProviderSourceKind,
    };

    struct FixtureProvider {
        observations: Vec<ProviderObservation>,
    }

    impl ContextObservationProvider for FixtureProvider {
        fn observations(
            &mut self,
            _now: Instant,
        ) -> Result<Vec<ProviderObservation>, super::ProviderError> {
            Ok(std::mem::take(&mut self.observations))
        }
    }

    fn observed(source: ProviderSourceKind) -> ProviderObservation {
        ProviderObservation::new(
            match source {
                ProviderSourceKind::VscodeWindow | ProviderSourceKind::VscodeIntegratedTerminal => {
                    ContextProviderKind::Vscode
                }
                ProviderSourceKind::ExternalTerminal | ProviderSourceKind::ShellSession => {
                    ContextProviderKind::Shell
                }
                ProviderSourceKind::ForegroundWindow => ContextProviderKind::ForegroundWindow,
            },
            source,
            Some(WindowCorrelationToken::from_native(42)),
            Some(CorrelationToken::new()),
            Some(CorrelationToken::new()),
            PathBuf::from("/tmp/lyn-provider-fixture"),
            Instant::now(),
            ObservationLiveness::Live,
        )
    }

    #[test]
    fn fixtures_keep_editor_integrated_terminal_and_external_terminal_distinct() {
        let mut provider = FixtureProvider {
            observations: vec![
                observed(ProviderSourceKind::VscodeWindow),
                observed(ProviderSourceKind::VscodeIntegratedTerminal),
                observed(ProviderSourceKind::ExternalTerminal),
            ],
        };

        let observations = provider.observations(Instant::now()).unwrap();

        assert_eq!(observations.len(), 3);
        assert_ne!(observations[0].source_kind(), observations[1].source_kind());
        assert_ne!(observations[1].source_kind(), observations[2].source_kind());
    }

    #[test]
    fn observation_contract_contains_only_correlation_directory_time_and_liveness_evidence() {
        let observation = observed(ProviderSourceKind::VscodeIntegratedTerminal);

        assert_eq!(observation.provider(), ContextProviderKind::Vscode);
        assert!(observation.window().is_some());
        assert!(observation.process().is_some());
        assert!(observation.session().is_some());
        assert_eq!(
            observation.directory(),
            PathBuf::from("/tmp/lyn-provider-fixture")
        );
        assert_eq!(observation.liveness(), ObservationLiveness::Live);
        assert!(observation.observed_at() <= Instant::now());
    }

    #[test]
    fn opaque_provider_tokens_are_redacted_in_diagnostics() {
        let token = CorrelationToken::new();

        assert_eq!(format!("{token:?}"), "CorrelationToken(<opaque>)");
    }
}
