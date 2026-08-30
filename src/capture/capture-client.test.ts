import { describe, expect, it, vi } from 'vitest';

import type {
  CaptureSession,
  CommandResult,
  ContextRef,
  SaveCaptureResult,
} from '../lib/ipc-types';
import { CaptureCommandError, createCaptureClient } from './capture-client';

const context: ContextRef = {
  id: 'context-1',
  kind: 'standalone',
  name: 'Inbox',
};

const session: CaptureSession = {
  sessionId: 'session-1',
  contextResolution: {
    state: 'required',
    candidate: null,
    selection: null,
  },
  stagedMedia: null,
  recordingState: { state: 'idle' },
};

describe('capture client', () => {
  it('wraps capture commands in the typed input envelope', async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, data: session })
      .mockResolvedValueOnce({ ok: true, data: { contexts: [context] } })
      .mockResolvedValueOnce({
        ok: true,
        data: {
          ...session,
          contextResolution: {
            state: 'resolved',
            candidate: {
              context,
              branchName: null,
              provider: 'manual',
              requiresConfirmation: false,
            },
            selection: { kind: 'saved_context', contextId: context.id },
          },
        },
      })
      .mockResolvedValueOnce({
        ok: true,
        data: {
          captureId: 'capture-1',
          capturedAt: '2026-08-29T10:00:00Z',
          enrichmentScheduled: false,
        } satisfies SaveCaptureResult,
      });
    const client = createCaptureClient(invoke);

    await client.getActiveSession();
    await client.listContexts();
    await client.selectContext(session.sessionId, context.id);
    await client.saveText(session.sessionId, '  exact draft\n');

    expect(invoke.mock.calls).toEqual([
      ['get_active_capture_session', { input: {} }],
      ['list_contexts', { input: { kind: null, query: null, limit: 100 } }],
      [
        'select_capture_context_source',
        {
          input: {
            sessionId: 'session-1',
            selection: {
              kind: 'saved_context',
              contextId: 'context-1',
            },
          },
        },
      ],
      [
        'save_text_capture',
        { input: { sessionId: 'session-1', textBody: '  exact draft\n' } },
      ],
    ]);
  });

  it('supports standalone context creation and cancellation', async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, data: { context } })
      .mockResolvedValueOnce({ ok: true, data: { cancelled: true } })
      .mockResolvedValueOnce({
        ok: true,
        data: { dismissed: true, focusRestored: true },
      });
    const client = createCaptureClient(invoke);

    await client.createStandaloneContext('Inbox');
    await client.cancel(session.sessionId);
    await client.dismissPopup();

    expect(invoke.mock.calls).toEqual([
      ['create_context', { input: { kind: 'standalone', name: 'Inbox' } }],
      ['cancel_capture_session', { input: { sessionId: 'session-1' } }],
      ['dismiss_capture_popup', { input: {} }],
    ]);
  });

  it('uses opaque screenshot staging and save inputs', async () => {
    const staged = {
      stagedMediaId: 'staged-1',
      kind: 'image' as const,
      previewUri: 'lyn-media://staged/staged-1',
      mimeType: 'image/png' as const,
      byteSize: 42,
      durationMs: null,
      widthPx: 1,
      heightPx: 1,
    };
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, data: staged })
      .mockResolvedValueOnce({
        ok: true,
        data: {
          captureId: 'capture-1',
          capturedAt: '2026-08-29T10:00:00Z',
          enrichmentScheduled: false,
        },
      });
    const client = createCaptureClient(invoke);

    await client.stageClipboardImage('session-1');
    await client.saveImage('session-1', 'staged-1', 'caption');

    expect(invoke.mock.calls).toEqual([
      ['stage_clipboard_image', { input: { sessionId: 'session-1' } }],
      [
        'save_image_capture',
        {
          input: {
            sessionId: 'session-1',
            stagedMediaId: 'staged-1',
            caption: 'caption',
          },
        },
      ],
    ]);
  });

  it('uses typed session-scoped voice recording, playback, and save inputs', async () => {
    const invoke = vi.fn().mockResolvedValue({
      ok: true,
      data: { state: 'recording', elapsedMs: 0 },
    });
    const client = createCaptureClient(invoke);

    await client.startAudioRecording('session-1');
    await client.stopAudioRecording('session-1');
    await client.playStagedAudio('session-1', 'staged-audio-1');
    await client.stopAudioPlayback('staged-audio-1');
    await client.saveAudio('session-1', 'staged-audio-1', 'caption');

    expect(invoke.mock.calls).toEqual([
      [
        'start_audio_recording',
        { input: { sessionId: 'session-1', inputDeviceId: null } },
      ],
      ['stop_audio_recording', { input: { sessionId: 'session-1' } }],
      [
        'play_staged_audio',
        {
          input: {
            sessionId: 'session-1',
            stagedMediaId: 'staged-audio-1',
          },
        },
      ],
      [
        'stop_audio_playback',
        { input: { playbackTargetId: 'staged-audio-1' } },
      ],
      [
        'save_audio_capture',
        {
          input: {
            sessionId: 'session-1',
            stagedMediaId: 'staged-audio-1',
            caption: 'caption',
          },
        },
      ],
    ]);
  });

  it('lists and selects opaque live context sources', async () => {
    const sourceResult = {
      liveSources: [],
      savedContexts: [context],
    };
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, data: sourceResult })
      .mockResolvedValueOnce({ ok: true, data: session });
    const client = createCaptureClient(invoke);

    await client.listContextSources(session.sessionId, 'lyn');
    await client.selectLiveSource(session.sessionId, 'source-1');

    expect(invoke.mock.calls).toEqual([
      [
        'list_capture_context_sources',
        { input: { sessionId: 'session-1', query: 'lyn', limit: 100 } },
      ],
      [
        'select_capture_context_source',
        {
          input: {
            sessionId: 'session-1',
            selection: { kind: 'live_source', sourceId: 'source-1' },
          },
        },
      ],
    ]);
  });

  it('throws the structured application error without flattening it', async () => {
    const failure: CommandResult<CaptureSession> = {
      ok: false,
      error: {
        code: 'STORAGE_UNAVAILABLE',
        message: 'Contexts are temporarily unavailable',
        retryable: true,
        details: { operation: 'list' },
      },
    };
    const client = createCaptureClient(vi.fn().mockResolvedValue(failure));

    await expect(client.getActiveSession()).rejects.toEqual(
      expect.objectContaining({
        name: 'CaptureCommandError',
        appError: failure.error,
      }),
    );
    await expect(client.getActiveSession()).rejects.toBeInstanceOf(
      CaptureCommandError,
    );
  });
});
