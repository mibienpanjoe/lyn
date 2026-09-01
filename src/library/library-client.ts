import { invoke } from '@tauri-apps/api/core';

import type {
  AppError,
  AudioPlaybackResult,
  CaptureDetail,
  CaptureKind,
  CaptureSummary,
  CommandResult,
  ContextId,
  ContextRef,
  LibraryScope,
  ListContextsResult,
  OpenMediaResult,
  Page,
  Timestamp,
} from '../lib/ipc-types';

type Invoke = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

export interface CaptureFilters {
  branchName: string | null;
  captureKinds: CaptureKind[];
  capturedFrom: Timestamp | null;
  capturedTo: Timestamp | null;
}

export interface LibraryClient {
  listContexts(): Promise<ContextRef[]>;
  listCaptures(
    scope: LibraryScope,
    filters?: Partial<CaptureFilters>,
    cursor?: string | null,
  ): Promise<Page<CaptureSummary>>;
  getCapture(captureId: string): Promise<CaptureDetail>;
  playMedia(mediaId: string): Promise<AudioPlaybackResult>;
  stopPlayback(playbackTargetId: string): Promise<AudioPlaybackResult>;
  openMedia(mediaId: string): Promise<OpenMediaResult>;
}

export class LibraryCommandError extends Error {
  constructor(readonly appError: AppError) {
    super(appError.message);
    this.name = 'LibraryCommandError';
  }
}

async function command<T>(call: Invoke, name: string, input: unknown) {
  const result = await call<CommandResult<T>>(name, { input });
  if (!result.ok) throw new LibraryCommandError(result.error);
  return result.data;
}

export function createLibraryClient(call: Invoke = invoke): LibraryClient {
  return {
    listContexts: async () => {
      const result = await command<ListContextsResult>(call, 'list_contexts', {
        kind: null,
        query: null,
        limit: 100,
      });
      return result.contexts;
    },
    listCaptures: (scope, filters = {}, cursor = null) =>
      command<Page<CaptureSummary>>(call, 'list_captures', {
        scope,
        branchName: filters.branchName ?? null,
        captureKinds: filters.captureKinds ?? [],
        capturedFrom: filters.capturedFrom ?? null,
        capturedTo: filters.capturedTo ?? null,
        cursor,
        limit: 50,
      }),
    getCapture: (captureId) =>
      command<CaptureDetail>(call, 'get_capture', { captureId }),
    playMedia: (mediaId) =>
      command<AudioPlaybackResult>(call, 'play_media', { mediaId }),
    stopPlayback: (playbackTargetId) =>
      command<AudioPlaybackResult>(call, 'stop_audio_playback', {
        playbackTargetId,
      }),
    openMedia: (mediaId) =>
      command<OpenMediaResult>(call, 'open_media_external', { mediaId }),
  };
}

export const libraryClient = createLibraryClient();
