//! Deterministic invocation-bound context evidence ranking.

use crate::{
    context::provider::CorrelationToken,
    contract::{ContextProviderKind, ContextSourceId},
    platform::WindowCorrelationToken,
};

pub(crate) struct InvocationAssociations<'a> {
    pub(crate) foreground_window: Option<WindowCorrelationToken>,
    pub(crate) related_processes: &'a [CorrelationToken],
    pub(crate) related_sessions: &'a [CorrelationToken],
    pub(crate) inferred_windows: &'a [WindowCorrelationToken],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum EvidenceQuality {
    RecentForegroundInference,
    VerifiedRelation,
    ExactForeground,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolutionCandidate {
    source_id: ContextSourceId,
    provider: ContextProviderKind,
    quality: EvidenceQuality,
}

impl ResolutionCandidate {
    pub(crate) fn new(
        source_id: ContextSourceId,
        provider: ContextProviderKind,
        quality: EvidenceQuality,
    ) -> Self {
        Self {
            source_id,
            provider,
            quality,
        }
    }
}

pub(crate) fn classify(
    source_id: ContextSourceId,
    provider: ContextProviderKind,
    window: Option<WindowCorrelationToken>,
    process: Option<CorrelationToken>,
    session: Option<CorrelationToken>,
    invocation: &InvocationAssociations<'_>,
) -> Option<ResolutionCandidate> {
    let quality = if window.is_some() && window == invocation.foreground_window {
        EvidenceQuality::ExactForeground
    } else if process.is_some_and(|token| invocation.related_processes.contains(&token))
        || session.is_some_and(|token| invocation.related_sessions.contains(&token))
    {
        EvidenceQuality::VerifiedRelation
    } else if window.is_some_and(|token| invocation.inferred_windows.contains(&token)) {
        EvidenceQuality::RecentForegroundInference
    } else {
        return None;
    };
    Some(ResolutionCandidate::new(source_id, provider, quality))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolutionOutcome {
    Resolved(ContextSourceId),
    Ambiguous,
    Required,
}

pub(crate) fn resolve(
    candidates: &[ResolutionCandidate],
    provider_order: &[ContextProviderKind],
) -> ResolutionOutcome {
    let Some(best_quality) = candidates.iter().map(|candidate| candidate.quality).max() else {
        return ResolutionOutcome::Required;
    };
    let quality_matches: Vec<_> = candidates
        .iter()
        .filter(|candidate| candidate.quality == best_quality)
        .collect();
    let best_provider_rank = quality_matches
        .iter()
        .map(|candidate| provider_rank(candidate.provider, provider_order))
        .min()
        .unwrap_or(usize::MAX);
    let mut winners: Vec<_> = quality_matches
        .into_iter()
        .filter(|candidate| provider_rank(candidate.provider, provider_order) == best_provider_rank)
        .map(|candidate| candidate.source_id)
        .collect();
    winners.sort_by_key(ToString::to_string);
    winners.dedup();

    match winners.as_slice() {
        [source_id] => ResolutionOutcome::Resolved(*source_id),
        _ => ResolutionOutcome::Ambiguous,
    }
}

fn provider_rank(provider: ContextProviderKind, provider_order: &[ContextProviderKind]) -> usize {
    provider_order
        .iter()
        .position(|candidate| *candidate == provider)
        .unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use crate::{
        context::provider::CorrelationToken,
        contract::{ContextProviderKind, ContextSourceId},
        platform::WindowCorrelationToken,
    };

    use super::{
        EvidenceQuality, InvocationAssociations, ResolutionCandidate, ResolutionOutcome, classify,
        resolve,
    };

    fn candidate(provider: ContextProviderKind, quality: EvidenceQuality) -> ResolutionCandidate {
        ResolutionCandidate::new(ContextSourceId::new(), provider, quality)
    }

    #[test]
    fn exact_foreground_beats_an_unrelated_newer_quality_class() {
        let exact = candidate(ContextProviderKind::Shell, EvidenceQuality::ExactForeground);
        let unrelated = candidate(
            ContextProviderKind::Vscode,
            EvidenceQuality::RecentForegroundInference,
        );

        assert_eq!(
            resolve(
                &[unrelated, exact],
                &[ContextProviderKind::Vscode, ContextProviderKind::Shell],
            ),
            ResolutionOutcome::Resolved(exact.source_id)
        );
    }

    #[test]
    fn only_invocation_associated_observations_become_candidates() {
        let foreground = WindowCorrelationToken::from_native(10);
        let background = WindowCorrelationToken::from_native(11);
        let related_session = CorrelationToken::new();
        let associations = InvocationAssociations {
            foreground_window: Some(foreground),
            related_processes: &[],
            related_sessions: &[related_session],
            inferred_windows: &[],
        };

        let exact = classify(
            ContextSourceId::new(),
            ContextProviderKind::Vscode,
            Some(foreground),
            None,
            None,
            &associations,
        )
        .unwrap();
        let related = classify(
            ContextSourceId::new(),
            ContextProviderKind::Shell,
            Some(background),
            None,
            Some(related_session),
            &associations,
        )
        .unwrap();
        let unrelated = classify(
            ContextSourceId::new(),
            ContextProviderKind::Shell,
            Some(background),
            Some(CorrelationToken::new()),
            Some(CorrelationToken::new()),
            &associations,
        );

        assert_eq!(exact.quality, EvidenceQuality::ExactForeground);
        assert_eq!(related.quality, EvidenceQuality::VerifiedRelation);
        assert!(unrelated.is_none());
    }

    #[test]
    fn provider_preference_breaks_only_equal_quality_cross_provider_ties() {
        let exact_shell = candidate(ContextProviderKind::Shell, EvidenceQuality::ExactForeground);
        let related_vscode = candidate(
            ContextProviderKind::Vscode,
            EvidenceQuality::VerifiedRelation,
        );
        assert_eq!(
            resolve(
                &[exact_shell, related_vscode],
                &[ContextProviderKind::Vscode, ContextProviderKind::Shell],
            ),
            ResolutionOutcome::Resolved(exact_shell.source_id)
        );

        let related_shell = candidate(
            ContextProviderKind::Shell,
            EvidenceQuality::VerifiedRelation,
        );
        assert_eq!(
            resolve(
                &[related_shell, related_vscode],
                &[ContextProviderKind::Vscode, ContextProviderKind::Shell],
            ),
            ResolutionOutcome::Resolved(related_vscode.source_id)
        );
    }

    #[test]
    fn missing_is_required_and_equal_same_provider_evidence_is_ambiguous() {
        assert_eq!(resolve(&[], &[]), ResolutionOutcome::Required);

        let first = candidate(
            ContextProviderKind::Vscode,
            EvidenceQuality::ExactForeground,
        );
        let second = candidate(
            ContextProviderKind::Vscode,
            EvidenceQuality::ExactForeground,
        );
        assert_eq!(
            resolve(&[first, second], &[ContextProviderKind::Vscode]),
            ResolutionOutcome::Ambiguous
        );
    }

    #[test]
    fn resolution_is_invariant_under_candidate_permutation() {
        let exact = candidate(
            ContextProviderKind::Vscode,
            EvidenceQuality::ExactForeground,
        );
        let related = candidate(
            ContextProviderKind::Shell,
            EvidenceQuality::VerifiedRelation,
        );
        let inferred = candidate(
            ContextProviderKind::ForegroundWindow,
            EvidenceQuality::RecentForegroundInference,
        );
        let permutations = [
            [exact, related, inferred],
            [exact, inferred, related],
            [related, exact, inferred],
            [related, inferred, exact],
            [inferred, exact, related],
            [inferred, related, exact],
        ];

        for candidates in permutations {
            assert_eq!(
                resolve(
                    &candidates,
                    &[ContextProviderKind::Shell, ContextProviderKind::Vscode]
                ),
                ResolutionOutcome::Resolved(exact.source_id)
            );
        }
    }
}
