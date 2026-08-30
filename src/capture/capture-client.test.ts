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
      .mockResolvedValueOnce({ ok: true, data: { cancelled: true } });
    const client = createCaptureClient(invoke);

    await client.createStandaloneContext('Inbox');
    await client.cancel(session.sessionId);

    expect(invoke.mock.calls).toEqual([
      ['create_context', { input: { kind: 'standalone', name: 'Inbox' } }],
      ['cancel_capture_session', { input: { sessionId: 'session-1' } }],
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
