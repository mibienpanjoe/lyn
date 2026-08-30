import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type {
  AppError,
  CancelCaptureSessionResult,
  CaptureSession,
  CommandResult,
  ContextId,
  ContextRef,
  CaptureSessionId,
  CreateContextResult,
  DismissCapturePopupResult,
  ListContextsResult,
  SaveCaptureResult,
} from '../lib/ipc-types';

type Invoke = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

export interface CaptureClient {
  getActiveSession(): Promise<CaptureSession>;
  listContexts(): Promise<ContextRef[]>;
  createStandaloneContext(name: string): Promise<ContextRef>;
  selectContext(
    sessionId: CaptureSessionId,
    contextId: ContextId,
  ): Promise<CaptureSession>;
  saveText(
    sessionId: CaptureSessionId,
    textBody: string,
  ): Promise<SaveCaptureResult>;
  cancel(sessionId: CaptureSessionId): Promise<CancelCaptureSessionResult>;
  dismissPopup(): Promise<DismissCapturePopupResult>;
  onSessionReady(
    listener: (session: CaptureSession) => void,
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
    saveText: (sessionId, textBody) =>
      command<SaveCaptureResult>(call, 'save_text_capture', {
        sessionId,
        textBody,
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
  };
}

export const captureClient = createCaptureClient();
