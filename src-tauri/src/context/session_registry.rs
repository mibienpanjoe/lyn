//! Ephemeral, validated live context sources.
#![allow(
    dead_code,
    reason = "registry query API is consumed by the T15 resolver and T16 commands"
)]

use std::{
    collections::HashMap,
    fs,
    path::Path,
    time::{Duration, Instant},
};

use crate::{
    context::{ProjectDirectoryIdentity, inspect_project_directory},
    contract::{ContextId, ContextKind, ContextProviderKind, ContextRef, ContextSourceId},
    platform::WindowCorrelationToken,
};

use super::provider::{
    ContextObservationProvider, CorrelationToken, ObservationLiveness, ProviderObservation,
    ProviderSourceKind,
};

pub(crate) const LIVE_SOURCE_TTL: Duration = Duration::from_secs(30);
const MAX_SAFE_LABEL_CHARS: usize = 100;

#[derive(Clone)]
pub(crate) struct LiveContextSource {
    source_id: ContextSourceId,
    provider: ContextProviderKind,
    source_kind: ProviderSourceKind,
    window: Option<WindowCorrelationToken>,
    process: Option<CorrelationToken>,
    session: Option<CorrelationToken>,
    identity: ProjectDirectoryIdentity,
    context: ContextRef,
    application_name: &'static str,
    label: String,
    observed_at: Instant,
    expires_at: Instant,
    #[cfg(target_os = "linux")]
    process_identity: Option<(u32, u64)>,
    #[cfg(target_os = "linux")]
    native: Option<std::sync::Arc<super::gnome_broker::NativeSourceLease>>,
}

impl LiveContextSource {
    pub(crate) fn is_native(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.native.is_some()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn from_native(lease: super::gnome_broker::NativeSourceLease) -> Option<Self> {
        let identity = inspect_project_directory(lease.directory()).ok()?;
        let name = safe_directory_name(lease.directory());
        if !lease.revalidate() {
            return None;
        }
        let now = Instant::now();
        Some(Self {
            source_id: ContextSourceId::new(),
            provider: ContextProviderKind::Shell,
            source_kind: ProviderSourceKind::ExternalTerminal,
            window: Some(WindowCorrelationToken::from_native(u64::from(
                lease.window(),
            ))),
            process: None,
            session: Some(CorrelationToken::new()),
            identity,
            context: ContextRef {
                id: ContextId::new(),
                kind: ContextKind::Project,
                name: name.clone(),
            },
            application_name: "GNOME Terminal",
            label: name,
            observed_at: now,
            expires_at: now + LIVE_SOURCE_TTL,
            process_identity: None,
            native: Some(std::sync::Arc::new(lease)),
        })
    }

    /// Operates on an owned snapshot: never invoke while holding the registry
    /// or capture-session mutex. Native sources retain their lifetime pins.
    pub(crate) fn revalidated(mut self) -> Option<Self> {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::MetadataExt;
            if let Some(lease) = &self.native {
                let refreshed = inspect_project_directory(lease.directory()).ok()?;
                if refreshed.project_key != self.identity.project_key
                    || refreshed.project_path != self.identity.project_path
                    || !lease.revalidate()
                {
                    return None;
                }
                self.identity = refreshed;
            } else if let Some(pid) = self.process.and_then(CorrelationToken::native_process_id) {
                let path = format!("/proc/{pid}");
                let before = fs::metadata(&path).ok()?;
                let process_identity = (before.uid(), before.ino());
                if Some(process_identity) != self.process_identity
                    || before.uid() != fs::metadata("/proc/self").ok()?.uid()
                {
                    return None;
                }
                let directory = fs::canonicalize(format!("{path}/cwd")).ok()?;
                let refreshed = inspect_project_directory(&directory).ok()?;
                let after = fs::metadata(&path).ok()?;
                if process_identity != (after.uid(), after.ino())
                    || refreshed.project_key != self.identity.project_key
                    || refreshed.project_path != self.identity.project_path
                {
                    return None;
                }
                self.identity = refreshed;
            }
        }
        Some(self)
    }

    pub(crate) fn revalidate_native_lifetime(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.native.as_ref().is_none_or(|lease| lease.revalidate())
        }
        #[cfg(not(target_os = "linux"))]
        {
            true
        }
    }

    pub(crate) fn source_id(&self) -> ContextSourceId {
        self.source_id
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
    pub(crate) fn identity(&self) -> &ProjectDirectoryIdentity {
        &self.identity
    }
    pub(crate) fn context(&self) -> &ContextRef {
        &self.context
    }
    pub(crate) fn application_name(&self) -> &'static str {
        self.application_name
    }
    pub(crate) fn label(&self) -> &str {
        &self.label
    }
    pub(crate) fn observed_at(&self) -> Instant {
        self.observed_at
    }
}

#[derive(Default)]
pub(crate) struct ContextSourceRegistry {
    sources: HashMap<ContextSourceId, LiveContextSource>,
    #[cfg(target_os = "linux")]
    native_session: Option<crate::contract::CaptureSessionId>,
}

impl ContextSourceRegistry {
    pub(crate) fn refresh_from_provider(
        &mut self,
        provider: &mut impl ContextObservationProvider,
        now: Instant,
    ) -> usize {
        let Ok(observations) = provider.observations(now) else {
            self.expire(now);
            return 0;
        };
        observations
            .into_iter()
            .filter_map(|observation| self.register(observation, now))
            .count()
    }

    pub(crate) fn register(
        &mut self,
        observation: ProviderObservation,
        now: Instant,
    ) -> Option<ContextSourceId> {
        self.expire(now);
        if !valid_contract(&observation)
            || observation.observed_at() > now
            || now.duration_since(observation.observed_at()) >= LIVE_SOURCE_TTL
        {
            return None;
        }

        if observation.liveness() == ObservationLiveness::Ended {
            self.sources
                .retain(|_, source| !same_correlation(source, &observation));
            return None;
        }

        let canonical_directory = fs::canonicalize(observation.directory()).ok()?;
        if !canonical_directory.is_dir() {
            return None;
        }
        let identity = inspect_project_directory(&canonical_directory).ok()?;
        let application_name = application_name(observation.source_kind());
        let context_name = safe_directory_name(&canonical_directory);
        let label = context_name.clone();

        if let Some(source_id) = self
            .sources
            .values()
            .find(|source| same_correlation(source, &observation))
            .map(|source| source.source_id)
        {
            let same_project = self.sources.get(&source_id).is_some_and(|source| {
                source.identity.project_key == identity.project_key
                    && source.identity.project_path == identity.project_path
            });
            if same_project {
                let source = self
                    .sources
                    .get_mut(&source_id)
                    .expect("source id was found");
                source.identity = identity;
                source.context.name = context_name;
                source.label = label;
                source.observed_at = observation.observed_at();
                source.expires_at = observation.observed_at() + LIVE_SOURCE_TTL;
                return Some(source.source_id);
            }
            self.sources.remove(&source_id);
        }

        let source_id = ContextSourceId::new();
        self.sources.insert(
            source_id,
            LiveContextSource {
                source_id,
                provider: observation.provider(),
                source_kind: observation.source_kind(),
                window: observation.window(),
                process: observation.process(),
                session: observation.session(),
                identity,
                context: ContextRef {
                    id: ContextId::new(),
                    kind: ContextKind::Project,
                    name: context_name,
                },
                application_name,
                label,
                observed_at: observation.observed_at(),
                expires_at: observation.observed_at() + LIVE_SOURCE_TTL,
                #[cfg(target_os = "linux")]
                process_identity: observation.process().and_then(|token| {
                    use std::os::unix::fs::MetadataExt;
                    let metadata =
                        fs::metadata(format!("/proc/{}", token.native_process_id()?)).ok()?;
                    Some((metadata.uid(), metadata.ino()))
                }),
                #[cfg(target_os = "linux")]
                native: None,
            },
        );
        Some(source_id)
    }

    pub(crate) fn get(
        &mut self,
        source_id: ContextSourceId,
        now: Instant,
    ) -> Option<&LiveContextSource> {
        self.expire(now);
        self.sources.get(&source_id)
    }

    pub(crate) fn live_sources(&mut self, now: Instant) -> Vec<&LiveContextSource> {
        self.expire(now);
        let mut sources: Vec<_> = self.sources.values().collect();
        sources.sort_by_key(|source| source.source_id.to_string());
        sources
    }

    /// Owned invocation evidence: later heartbeats cannot alter this snapshot.
    pub(crate) fn snapshot_live_sources(&mut self, now: Instant) -> Vec<LiveContextSource> {
        self.live_sources(now)
            .into_iter()
            .filter(|source| !source.is_native())
            .cloned()
            .collect()
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn register_native(
        &mut self,
        session_id: crate::contract::CaptureSessionId,
        source: LiveContextSource,
    ) {
        assert!(source.is_native());
        // One active capture, one retained native lease. Call only after its
        // invocation ticket is accepted, so stale jobs cannot retire a source.
        self.sources.retain(|_, source| !source.is_native());
        self.sources.insert(source.source_id, source);
        self.native_session = Some(session_id);
    }

    pub(crate) fn release_native(&mut self, session_id: crate::contract::CaptureSessionId) {
        #[cfg(target_os = "linux")]
        if self.native_session == Some(session_id) {
            self.sources.retain(|_, source| !source.is_native());
            self.native_session = None;
        }
        #[cfg(not(target_os = "linux"))]
        let _ = session_id;
    }

    /// Revalidate native shell identity before consuming a cached selection.
    #[cfg(test)]
    pub(crate) fn revalidate(
        &mut self,
        source_id: ContextSourceId,
        now: Instant,
    ) -> Option<&LiveContextSource> {
        self.expire(now);
        let source = self.sources.get(&source_id)?.clone();
        match source.revalidated() {
            Some(source) => {
                self.sources.insert(source_id, source);
            }
            None => {
                self.sources.remove(&source_id);
                return None;
            }
        }
        self.sources.get(&source_id)
    }

    pub(crate) fn end_window_kind(
        &mut self,
        window: WindowCorrelationToken,
        source_kind: ProviderSourceKind,
    ) -> bool {
        let before = self.sources.len();
        self.sources.retain(|_, source| {
            !(source.window == Some(window) && source.source_kind == source_kind)
        });
        self.sources.len() != before
    }

    fn expire(&mut self, now: Instant) {
        self.sources
            .retain(|_, source| source.is_native() || source.expires_at > now);
    }
}

fn valid_contract(observation: &ProviderObservation) -> bool {
    let provider_matches = matches!(
        (observation.provider(), observation.source_kind()),
        (
            ContextProviderKind::Vscode,
            ProviderSourceKind::VscodeWindow
        ) | (
            ContextProviderKind::Vscode,
            ProviderSourceKind::VscodeIntegratedTerminal
        ) | (
            ContextProviderKind::Cursor,
            ProviderSourceKind::CursorWindow
        ) | (
            ContextProviderKind::Cursor,
            ProviderSourceKind::CursorIntegratedTerminal
        ) | (ContextProviderKind::Browser, ProviderSourceKind::BrowserTab)
            | (
                ContextProviderKind::Shell,
                ProviderSourceKind::ExternalTerminal
            )
            | (ContextProviderKind::Shell, ProviderSourceKind::ShellSession)
            | (
                ContextProviderKind::ForegroundWindow,
                ProviderSourceKind::ForegroundWindow
            )
    );
    let correlations_present = match observation.source_kind() {
        ProviderSourceKind::VscodeWindow
        | ProviderSourceKind::CursorWindow
        | ProviderSourceKind::BrowserTab
        | ProviderSourceKind::ForegroundWindow => observation.window().is_some(),
        ProviderSourceKind::VscodeIntegratedTerminal
        | ProviderSourceKind::CursorIntegratedTerminal
        | ProviderSourceKind::ExternalTerminal => {
            observation.window().is_some() && observation.session().is_some()
        }
        ProviderSourceKind::ShellSession => {
            observation.process().is_some() && observation.session().is_some()
        }
    };
    provider_matches && correlations_present
}

fn same_correlation(source: &LiveContextSource, observation: &ProviderObservation) -> bool {
    source.provider == observation.provider()
        && source.source_kind == observation.source_kind()
        && source.window == observation.window()
        && source.process == observation.process()
        && source.session == observation.session()
}

fn application_name(source_kind: ProviderSourceKind) -> &'static str {
    match source_kind {
        ProviderSourceKind::VscodeWindow | ProviderSourceKind::VscodeIntegratedTerminal => {
            "VS Code"
        }
        ProviderSourceKind::CursorWindow | ProviderSourceKind::CursorIntegratedTerminal => "Cursor",
        ProviderSourceKind::BrowserTab => "Browser",
        ProviderSourceKind::ExternalTerminal => "Terminal",
        ProviderSourceKind::ShellSession => "Shell",
        ProviderSourceKind::ForegroundWindow => "Foreground window",
    }
}

fn safe_directory_name(directory: &Path) -> String {
    let directory_name = directory
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let safe: String = directory_name
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_SAFE_LABEL_CHARS)
        .collect();
    if safe.is_empty() {
        "Local context".to_owned()
    } else {
        safe
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };

    use tempfile::tempdir;

    use crate::{
        context::provider::{
            ContextObservationProvider, CorrelationToken, ObservationLiveness, ProviderError,
            ProviderObservation, ProviderSourceKind,
        },
        contract::ContextProviderKind,
        platform::WindowCorrelationToken,
    };

    use super::{ContextSourceRegistry, LIVE_SOURCE_TTL};

    struct UnavailableProvider;

    impl ContextObservationProvider for UnavailableProvider {
        fn observations(
            &mut self,
            _now: Instant,
        ) -> Result<Vec<ProviderObservation>, ProviderError> {
            Err(ProviderError::Unavailable)
        }
    }

    fn observation(
        directory: PathBuf,
        source_kind: ProviderSourceKind,
        session: Option<CorrelationToken>,
        observed_at: Instant,
    ) -> ProviderObservation {
        let provider = match source_kind {
            ProviderSourceKind::VscodeWindow | ProviderSourceKind::VscodeIntegratedTerminal => {
                ContextProviderKind::Vscode
            }
            ProviderSourceKind::CursorWindow | ProviderSourceKind::CursorIntegratedTerminal => {
                ContextProviderKind::Cursor
            }
            ProviderSourceKind::BrowserTab => ContextProviderKind::Browser,
            ProviderSourceKind::ExternalTerminal | ProviderSourceKind::ShellSession => {
                ContextProviderKind::Shell
            }
            ProviderSourceKind::ForegroundWindow => ContextProviderKind::ForegroundWindow,
        };
        ProviderObservation::new(
            provider,
            source_kind,
            Some(WindowCorrelationToken::from_native(9)),
            Some(CorrelationToken::new()),
            session,
            directory,
            observed_at,
            ObservationLiveness::Live,
        )
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn queued_invocation_snapshot_excludes_a_later_heartbeat() {
        use super::super::resolution_worker::ResolutionWorker;
        use std::sync::mpsc;
        let directory = tempdir().unwrap();
        let now = Instant::now();
        let mut registry = ContextSourceRegistry::default();
        let worker = ResolutionWorker::start().unwrap();
        let (started, wait_started) = mpsc::channel();
        let (release, wait_release) = mpsc::channel();
        let (finished, wait_finished) = mpsc::channel();
        worker.submit(Box::new(move || {
            started.send(()).unwrap();
            wait_release.recv_timeout(Duration::from_secs(2)).unwrap();
        }));
        wait_started.recv_timeout(Duration::from_secs(2)).unwrap();
        let snapshot = registry.snapshot_live_sources(now);
        worker.submit(Box::new(move || {
            finished.send(snapshot.len()).unwrap();
        }));
        registry
            .register(
                observation(
                    directory.path().to_path_buf(),
                    ProviderSourceKind::VscodeWindow,
                    None,
                    now,
                ),
                now,
            )
            .unwrap();
        assert_eq!(registry.live_sources(now).len(), 1);
        release.send(()).unwrap();
        assert_eq!(
            wait_finished.recv_timeout(Duration::from_secs(2)).unwrap(),
            0
        );
    }

    #[test]
    fn distinguishable_sessions_remain_separate_even_for_one_directory() {
        let directory = tempdir().unwrap();
        let now = Instant::now();
        let mut registry = ContextSourceRegistry::default();

        let first = registry
            .register(
                observation(
                    directory.path().to_path_buf(),
                    ProviderSourceKind::VscodeIntegratedTerminal,
                    Some(CorrelationToken::new()),
                    now,
                ),
                now,
            )
            .unwrap();
        let second = registry
            .register(
                observation(
                    directory.path().to_path_buf(),
                    ProviderSourceKind::VscodeIntegratedTerminal,
                    Some(CorrelationToken::new()),
                    now,
                ),
                now,
            )
            .unwrap();

        assert_ne!(first, second);
        assert_eq!(registry.live_sources(now).len(), 2);
    }

    #[test]
    fn expiry_is_deterministic_at_the_ttl_boundary() {
        let directory = tempdir().unwrap();
        let now = Instant::now();
        let mut registry = ContextSourceRegistry::default();
        let source_id = registry
            .register(
                observation(
                    directory.path().to_path_buf(),
                    ProviderSourceKind::VscodeWindow,
                    None,
                    now,
                ),
                now,
            )
            .unwrap();

        assert!(
            registry
                .get(source_id, now + LIVE_SOURCE_TTL - Duration::from_nanos(1))
                .is_some()
        );
        assert!(registry.get(source_id, now + LIVE_SOURCE_TTL).is_none());
        assert!(registry.live_sources(now + LIVE_SOURCE_TTL).is_empty());
    }

    #[test]
    fn malformed_observations_are_ignored_without_poisoning_the_registry() {
        let directory = tempdir().unwrap();
        let now = Instant::now();
        let mut registry = ContextSourceRegistry::default();
        let valid = observation(
            directory.path().to_path_buf(),
            ProviderSourceKind::ShellSession,
            Some(CorrelationToken::new()),
            now,
        );
        let mismatched = ProviderObservation::new(
            ContextProviderKind::Shell,
            ProviderSourceKind::VscodeWindow,
            Some(WindowCorrelationToken::from_native(8)),
            None,
            None,
            directory.path().to_path_buf(),
            now,
            ObservationLiveness::Live,
        );
        let missing_session = observation(
            directory.path().to_path_buf(),
            ProviderSourceKind::ExternalTerminal,
            None,
            now,
        );
        let unavailable = observation(
            directory.path().join("missing"),
            ProviderSourceKind::VscodeWindow,
            None,
            now,
        );

        assert!(registry.register(mismatched, now).is_none());
        assert!(registry.register(missing_session, now).is_none());
        assert!(registry.register(unavailable, now).is_none());
        assert!(registry.register(valid, now).is_some());
        assert_eq!(registry.live_sources(now).len(), 1);
    }

    #[test]
    fn unavailable_provider_does_not_remove_other_live_sources() {
        let directory = tempdir().unwrap();
        let now = Instant::now();
        let mut registry = ContextSourceRegistry::default();
        registry
            .register(
                observation(
                    directory.path().to_path_buf(),
                    ProviderSourceKind::VscodeWindow,
                    None,
                    now,
                ),
                now,
            )
            .unwrap();

        assert_eq!(
            registry.refresh_from_provider(&mut UnavailableProvider, now),
            0
        );
        assert_eq!(registry.live_sources(now).len(), 1);
    }

    #[test]
    fn safe_labels_are_derived_without_exposing_full_paths() {
        let parent = tempdir().unwrap();
        let directory = parent.path().join("lyn\nprivate");
        std::fs::create_dir(&directory).unwrap();
        let now = Instant::now();
        let mut registry = ContextSourceRegistry::default();
        let source_id = registry
            .register(
                observation(
                    directory.clone(),
                    ProviderSourceKind::VscodeWindow,
                    None,
                    now,
                ),
                now,
            )
            .unwrap();
        let source = registry.get(source_id, now).unwrap();

        assert_eq!(source.application_name(), "VS Code");
        assert!(!source.label().chars().any(char::is_control));
        assert!(
            !source
                .label()
                .contains(&parent.path().display().to_string())
        );
        assert!(source.label().chars().count() <= 100);
    }

    #[test]
    fn changed_project_identity_replaces_the_source_id() {
        let first_directory = tempdir().unwrap();
        let second_directory = tempdir().unwrap();
        let now = Instant::now();
        let window = WindowCorrelationToken::from_native(14);
        let process = CorrelationToken::new();
        let session = CorrelationToken::new();
        let make_observation = |directory: PathBuf, observed_at| {
            ProviderObservation::new(
                ContextProviderKind::Vscode,
                ProviderSourceKind::VscodeIntegratedTerminal,
                Some(window),
                Some(process),
                Some(session),
                directory,
                observed_at,
                ObservationLiveness::Live,
            )
        };
        let mut registry = ContextSourceRegistry::default();

        let first = registry
            .register(
                make_observation(first_directory.path().to_path_buf(), now),
                now,
            )
            .unwrap();
        let second = registry
            .register(
                make_observation(second_directory.path().to_path_buf(), now),
                now,
            )
            .unwrap();

        assert_ne!(first, second);
        assert!(registry.get(first, now).is_none());
        assert!(registry.get(second, now).is_some());
    }
}
