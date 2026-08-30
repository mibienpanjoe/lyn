import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type {
  AppError,
  AudioPlaybackResult,
  CancelCaptureSessionResult,
  CaptureSession,
  CommandResult,
  ContextId,
  ContextRef,
  ContextSourceId,
  ListCaptureContextSourcesResult,
  CaptureSessionId,
  CreateContextResult,
  DismissCapturePopupResult,
  ListContextsResult,
  SaveCaptureResult,
  StagedMedia,
  RecordingState,
} from '../lib/ipc-types';

type Invoke = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

export interface CaptureClient {
  getActiveSession(): Promise<CaptureSession>;
  listContexts(): Promise<ContextRef[]>;
  listContextSources(
    sessionId: CaptureSessionId,
    query?: string | null,
  ): Promise<ListCaptureContextSourcesResult>;
  createStandaloneContext(name: string): Promise<ContextRef>;
  selectContext(
    sessionId: CaptureSessionId,
    contextId: ContextId,
  ): Promise<CaptureSession>;
  selectLiveSource(
    sessionId: CaptureSessionId,
    sourceId: ContextSourceId,
  ): Promise<CaptureSession>;
  saveText(
    sessionId: CaptureSessionId,
    textBody: string,
  ): Promise<SaveCaptureResult>;
  stageClipboardImage(sessionId: CaptureSessionId): Promise<StagedMedia>;
  discardStagedMedia(
    sessionId: CaptureSessionId,
    stagedMediaId: string,
  ): Promise<CaptureSession>;
  saveImage(
    sessionId: CaptureSessionId,
    stagedMediaId: string,
    caption: string | null,
  ): Promise<SaveCaptureResult>;
  startAudioRecording(
    sessionId: CaptureSessionId,
    inputDeviceId?: string | null,
  ): Promise<RecordingState>;
  stopAudioRecording(sessionId: CaptureSessionId): Promise<StagedMedia>;
  playStagedAudio(
    sessionId: CaptureSessionId,
    stagedMediaId: string,
  ): Promise<AudioPlaybackResult>;
  stopAudioPlayback(playbackTargetId: string): Promise<AudioPlaybackResult>;
  saveAudio(
    sessionId: CaptureSessionId,
    stagedMediaId: string,
    caption: string | null,
  ): Promise<SaveCaptureResult>;
  cancel(sessionId: CaptureSessionId): Promise<CancelCaptureSessionResult>;
  dismissPopup(): Promise<DismissCapturePopupResult>;
  onSessionReady(
    listener: (session: CaptureSession) => void,
  ): Promise<UnlistenFn>;
  onContextSourcesChanged(
    listener: (sessionId: string) => void,
  ): Promise<UnlistenFn>;
}

export class CaptureCommandError extends Error {
  constructor(readonly appError: AppError) {
    super(appError.message);
    this.name = 'CaptureCommandError';
  }
}

async function command<T>(
  call: Invoke,
  name: string,
  input: unknown,
): Promise<T> {
  const result = await call<CommandResult<T>>(name, { input });
  if (!result.ok) {
    throw new CaptureCommandError(result.error);
  }
  return result.data;
}

export function createCaptureClient(call: Invoke = invoke): CaptureClient {
  return {
    getActiveSession: () =>
      command<CaptureSession>(call, 'get_active_capture_session', {}),
    listContexts: async () => {
      const result = await command<ListContextsResult>(call, 'list_contexts', {
        kind: null,
        query: null,
        limit: 100,
      });
      return result.contexts;
    },
    listContextSources: (sessionId, query = null) =>
      command<ListCaptureContextSourcesResult>(
        call,
        'list_capture_context_sources',
        { sessionId, query, limit: 100 },
      ),
    createStandaloneContext: async (name) => {
      const result = await command<CreateContextResult>(
        call,
        'create_context',
        {
          kind: 'standalone',
          name,
        },
      );
      return result.context;
    },
    selectContext: (sessionId, contextId) =>
      command<CaptureSession>(call, 'select_capture_context_source', {
        sessionId,
        selection: { kind: 'saved_context', contextId },
      }),
    selectLiveSource: (sessionId, sourceId) =>
      command<CaptureSession>(call, 'select_capture_context_source', {
        sessionId,
        selection: { kind: 'live_source', sourceId },
      }),
    saveText: (sessionId, textBody) =>
      command<SaveCaptureResult>(call, 'save_text_capture', {
        sessionId,
        textBody,
      }),
    stageClipboardImage: (sessionId) =>
      command<StagedMedia>(call, 'stage_clipboard_image', { sessionId }),
    discardStagedMedia: (sessionId, stagedMediaId) =>
      command<CaptureSession>(call, 'discard_staged_media', {
        sessionId,
        stagedMediaId,
      }),
    saveImage: (sessionId, stagedMediaId, caption) =>
      command<SaveCaptureResult>(call, 'save_image_capture', {
        sessionId,
        stagedMediaId,
        caption,
      }),
    startAudioRecording: (sessionId, inputDeviceId = null) =>
      command<RecordingState>(call, 'start_audio_recording', {
        sessionId,
        inputDeviceId,
      }),
    stopAudioRecording: (sessionId) =>
      command<StagedMedia>(call, 'stop_audio_recording', { sessionId }),
    playStagedAudio: (sessionId, stagedMediaId) =>
      command<AudioPlaybackResult>(call, 'play_staged_audio', {
        sessionId,
        stagedMediaId,
      }),
    stopAudioPlayback: (playbackTargetId) =>
      command<AudioPlaybackResult>(call, 'stop_audio_playback', {
        playbackTargetId,
      }),
    saveAudio: (sessionId, stagedMediaId, caption) =>
      command<SaveCaptureResult>(call, 'save_audio_capture', {
        sessionId,
        stagedMediaId,
        caption,
      }),
    cancel: (sessionId) =>
      command<CancelCaptureSessionResult>(call, 'cancel_capture_session', {
        sessionId,
      }),
    dismissPopup: () =>
      command<DismissCapturePopupResult>(call, 'dismiss_capture_popup', {}),
    onSessionReady: (listener) => {
      if (!('__TAURI_INTERNALS__' in window)) {
        return Promise.resolve(() => undefined);
      }
      return listen<CaptureSession>('capture://session-ready', (event) =>
        listener(event.payload),
      );
    },
    onContextSourcesChanged: (listener) => {
      if (!('__TAURI_INTERNALS__' in window)) {
        return Promise.resolve(() => undefined);
      }
      return listen<{ sessionId: string }>(
        'context://sources-changed',
        (event) => listener(event.payload.sessionId),
      );
    },
  };
}

export const captureClient = createCaptureClient();
