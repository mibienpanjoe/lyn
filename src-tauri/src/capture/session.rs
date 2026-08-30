use std::collections::VecDeque;

use crate::contract::{
    CaptureId, CaptureSession, CaptureSessionId, ContextResolution, RecordingState, StagedMedia,
    StagedMediaId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionStateError {
    StaleSession,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SaveOnceResult<Value> {
    Saved { capture_id: CaptureId, value: Value },
    AlreadySaved(CaptureId),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SaveOnceError<PersistenceError> {
    Session(SessionStateError),
    Persistence(PersistenceError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StagingCleanupRequest {
    pub(crate) session_id: CaptureSessionId,
    pub(crate) staged_media_id: StagedMediaId,
}

#[derive(Default)]
pub(crate) struct CaptureSessionService {
    active: Option<CaptureSession>,
    last_cancelled: Option<CaptureSessionId>,
    last_completed: Option<(CaptureSessionId, CaptureId)>,
    cleanup_requests: VecDeque<StagingCleanupRequest>,
}

impl CaptureSessionService {
    pub(crate) fn get_or_prepare(&mut self) -> CaptureSession {
        if let Some(active) = &self.active {
            return active.clone();
        }

        let session = CaptureSession {
            session_id: CaptureSessionId::new(),
            context_resolution: ContextResolution::Required {
                candidate: (),
                selection: (),
            },
            staged_media: None,
            recording_state: RecordingState::Idle,
        };
        self.active = Some(session.clone());
        session
    }

    pub(crate) fn active_session(&self) -> Option<CaptureSession> {
        self.active.clone()
    }

    pub(crate) fn set_context_resolution(
        &mut self,
        session_id: CaptureSessionId,
        resolution: ContextResolution,
    ) -> Result<CaptureSession, SessionStateError> {
        let session = self.active_mut(session_id)?;
        session.context_resolution = resolution;
        Ok(session.clone())
    }

    #[allow(dead_code, reason = "media staging is connected after T08")]
    pub(crate) fn set_staged_media(
        &mut self,
        session_id: CaptureSessionId,
        staged_media: StagedMedia,
    ) -> Result<CaptureSession, SessionStateError> {
        let session = self.active_mut(session_id)?;
        session.staged_media = Some(staged_media);
        Ok(session.clone())
    }

    pub(crate) fn start_recording(
        &mut self,
        session_id: CaptureSessionId,
    ) -> Result<CaptureSession, SessionStateError> {
        let session = self.active_mut(session_id)?;
        if !matches!(session.recording_state, RecordingState::Idle) {
            return Err(SessionStateError::StaleSession);
        }
        session.recording_state = RecordingState::Recording { elapsed_ms: 0 };
        Ok(session.clone())
    }

    pub(crate) fn stop_recording(
        &mut self,
        session_id: CaptureSessionId,
        staged_media: StagedMedia,
    ) -> Result<CaptureSession, SessionStateError> {
        let session = self.active_mut(session_id)?;
        if !matches!(session.recording_state, RecordingState::Recording { .. }) {
            return Err(SessionStateError::StaleSession);
        }
        let duration_ms = staged_media
            .duration_ms
            .ok_or(SessionStateError::StaleSession)?;
        session.recording_state = RecordingState::Stopped {
            elapsed_ms: duration_ms,
            staged_media_id: staged_media.staged_media_id,
        };
        session.staged_media = Some(staged_media);
        Ok(session.clone())
    }

    pub(crate) fn reset_recording(
        &mut self,
        session_id: CaptureSessionId,
    ) -> Result<CaptureSession, SessionStateError> {
        let session = self.active_mut(session_id)?;
        session.recording_state = RecordingState::Idle;
        Ok(session.clone())
    }

    pub(crate) fn cancel(&mut self, session_id: CaptureSessionId) -> Result<(), SessionStateError> {
        if self.last_cancelled == Some(session_id) {
            return Ok(());
        }
        if self.active.as_ref().map(|session| session.session_id) != Some(session_id) {
            return Err(SessionStateError::StaleSession);
        }

        let cancelled = self.active.take().expect("active session was checked");
        if let Some(staged_media) = cancelled.staged_media {
            self.cleanup_requests.push_back(StagingCleanupRequest {
                session_id,
                staged_media_id: staged_media.staged_media_id,
            });
        }
        self.last_cancelled = Some(session_id);
        Ok(())
    }

    pub(crate) fn save_once<Value, PersistenceError>(
        &mut self,
        session_id: CaptureSessionId,
        persist: impl FnOnce(&CaptureSession) -> Result<(CaptureId, Value), PersistenceError>,
    ) -> Result<SaveOnceResult<Value>, SaveOnceError<PersistenceError>> {
        if let Some((completed_session_id, capture_id)) = self.last_completed
            && completed_session_id == session_id
        {
            return Ok(SaveOnceResult::AlreadySaved(capture_id));
        }
        let session = self
            .active
            .as_ref()
            .filter(|session| session.session_id == session_id)
            .ok_or(SaveOnceError::Session(SessionStateError::StaleSession))?;
        let (capture_id, value) = persist(session).map_err(SaveOnceError::Persistence)?;

        self.active = None;
        self.last_completed = Some((session_id, capture_id));
        Ok(SaveOnceResult::Saved { capture_id, value })
    }

    #[allow(dead_code, reason = "the media cleanup worker is connected after T08")]
    pub(crate) fn take_cleanup_request(&mut self) -> Option<StagingCleanupRequest> {
        self.cleanup_requests.pop_front()
    }

    fn active_mut(
        &mut self,
        session_id: CaptureSessionId,
    ) -> Result<&mut CaptureSession, SessionStateError> {
        self.active
            .as_mut()
            .filter(|session| session.session_id == session_id)
            .ok_or(SessionStateError::StaleSession)
    }
}

#[cfg(test)]
mod tests {
    use crate::contract::{
        CaptureId, ContextCandidate, ContextId, ContextKind, ContextProviderKind, ContextRef,
        ContextResolution, MediaKind, MediaMimeType, RecordingState, StagedMedia, StagedMediaId,
    };

    use super::{CaptureSessionService, SaveOnceError, SaveOnceResult, SessionStateError};

    fn resolved_context() -> ContextResolution {
        ContextResolution::Resolved {
            candidate: ContextCandidate {
                context: ContextRef {
                    id: ContextId::new(),
                    kind: ContextKind::Standalone,
                    name: "Notes".to_owned(),
                },
                branch_name: None,
                provider: ContextProviderKind::Manual,
                requires_confirmation: false,
            },
            selection: None,
        }
    }

    fn staged_image() -> StagedMedia {
        StagedMedia {
            staged_media_id: StagedMediaId::new(),
            kind: MediaKind::Image,
            preview_uri: "lyn-media://staged/image".to_owned(),
            mime_type: MediaMimeType::ImagePng,
            byte_size: 10,
            duration_ms: None,
            width_px: Some(2),
            height_px: Some(5),
        }
    }

    #[test]
    fn duplicate_preparation_returns_one_required_session() {
        let mut service = CaptureSessionService::default();

        let first = service.get_or_prepare();
        let repeated = service.get_or_prepare();

        assert_eq!(repeated, first);
        assert!(matches!(
            first.context_resolution,
            ContextResolution::Required { .. }
        ));
        assert_eq!(first.recording_state, RecordingState::Idle);
        assert_eq!(first.staged_media, None);
    }

    #[test]
    fn resolution_transitions_preserve_all_other_session_state() {
        let mut service = CaptureSessionService::default();
        let prepared = service.get_or_prepare();
        let staged = staged_image();
        let with_media = service
            .set_staged_media(prepared.session_id, staged.clone())
            .unwrap();

        let ambiguous = service
            .set_context_resolution(
                prepared.session_id,
                ContextResolution::Ambiguous {
                    candidate: (),
                    selection: (),
                },
            )
            .unwrap();
        let resolved = service
            .set_context_resolution(prepared.session_id, resolved_context())
            .unwrap();

        assert_eq!(ambiguous.session_id, with_media.session_id);
        assert_eq!(ambiguous.staged_media, with_media.staged_media);
        assert_eq!(ambiguous.recording_state, with_media.recording_state);
        assert_eq!(resolved.session_id, with_media.session_id);
        assert_eq!(resolved.staged_media, Some(staged));
        assert_eq!(resolved.recording_state, with_media.recording_state);
    }

    #[test]
    fn cancellation_is_idempotent_and_queues_only_scoped_cleanup() {
        let mut service = CaptureSessionService::default();
        let session = service.get_or_prepare();
        let staged = staged_image();
        service
            .set_staged_media(session.session_id, staged.clone())
            .unwrap();

        service.cancel(session.session_id).unwrap();
        service.cancel(session.session_id).unwrap();

        let cleanup = service.take_cleanup_request().unwrap();
        assert_eq!(cleanup.session_id, session.session_id);
        assert_eq!(cleanup.staged_media_id, staged.staged_media_id);
        assert!(service.take_cleanup_request().is_none());
        assert!(service.active_session().is_none());
    }

    #[test]
    fn stale_session_cannot_cancel_or_mutate_the_active_session() {
        let mut service = CaptureSessionService::default();
        let stale = service.get_or_prepare();
        service.cancel(stale.session_id).unwrap();
        let active = service.get_or_prepare();

        assert_eq!(
            service.set_context_resolution(stale.session_id, resolved_context()),
            Err(SessionStateError::StaleSession)
        );
        assert_eq!(service.active_session(), Some(active.clone()));
        assert_eq!(service.cancel(stale.session_id), Ok(()));
        assert_eq!(service.active_session(), Some(active));
    }

    #[test]
    fn save_runs_once_and_returns_the_original_capture_on_replay() {
        let mut service = CaptureSessionService::default();
        let session = service.get_or_prepare();
        let capture_id = CaptureId::new();
        let mut writes = 0;

        let saved = service
            .save_once(session.session_id, |_| {
                writes += 1;
                Ok::<_, ()>((capture_id, "saved"))
            })
            .unwrap();
        let next_session = service.get_or_prepare();
        let replayed = service
            .save_once(session.session_id, |_| {
                writes += 1;
                Ok::<_, ()>((CaptureId::new(), "duplicate"))
            })
            .unwrap();

        assert_eq!(
            saved,
            SaveOnceResult::Saved {
                capture_id,
                value: "saved",
            }
        );
        assert_eq!(replayed, SaveOnceResult::AlreadySaved(capture_id));
        assert_eq!(writes, 1);
        assert_eq!(service.active_session(), Some(next_session));
    }

    #[test]
    fn failed_save_preserves_the_active_session_for_retry() {
        let mut service = CaptureSessionService::default();
        let session = service.get_or_prepare();

        let failed =
            service.save_once(session.session_id, |_| Err::<(CaptureId, ()), _>("offline"));

        assert_eq!(failed, Err(SaveOnceError::Persistence("offline")));
        assert_eq!(service.active_session(), Some(session));
    }

    #[test]
    fn cancelled_and_unknown_sessions_cannot_save() {
        let mut service = CaptureSessionService::default();
        let cancelled = service.get_or_prepare();
        service.cancel(cancelled.session_id).unwrap();

        let cancelled_save = service.save_once(cancelled.session_id, |_| {
            Ok::<_, ()>((CaptureId::new(), ()))
        });
        let unknown_save = service.save_once(crate::contract::CaptureSessionId::new(), |_| {
            Ok::<_, ()>((CaptureId::new(), ()))
        });

        assert_eq!(
            cancelled_save,
            Err(SaveOnceError::Session(SessionStateError::StaleSession))
        );
        assert_eq!(
            unknown_save,
            Err(SaveOnceError::Session(SessionStateError::StaleSession))
        );
    }
}
