import { describe, expect, it, vi } from 'vitest';

import { createLibraryClient } from './library-client';

describe('Library IPC client', () => {
  it('sends bounded camel-case list and opaque media requests', async () => {
    const call = vi.fn().mockResolvedValue({
      ok: true,
      data: { items: [], nextCursor: null },
    });
    const client = createLibraryClient(call);

    await client.listCaptures(
      { kind: 'context', contextId: 'context-1' },
      { branchName: 'main', captureKinds: ['text'] },
      'cursor-1',
    );

    expect(call).toHaveBeenCalledWith('list_captures', {
      input: {
        scope: { kind: 'context', contextId: 'context-1' },
        branchName: 'main',
        captureKinds: ['text'],
        capturedFrom: null,
        capturedTo: null,
        cursor: 'cursor-1',
        limit: 50,
      },
    });

    call.mockResolvedValueOnce({ ok: true, data: { playing: true } });
    await client.playMedia('media-1');
    expect(call).toHaveBeenLastCalledWith('play_media', {
      input: { mediaId: 'media-1' },
    });
  });

  it('deletes only an opaque project ID and preserves authoritative refusal', async () => {
    const call = vi
      .fn()
      .mockResolvedValue({ ok: true, data: { deleted: true } });
    const client = createLibraryClient(call);
    expect(await client.deleteProject('project-1')).toEqual({ deleted: true });
    expect(call).toHaveBeenCalledWith('delete_project', {
      input: { contextId: 'project-1' },
    });
    call.mockResolvedValueOnce({
      ok: false,
      error: {
        code: 'CONTEXT_NOT_EMPTY',
        message: 'Delete its captures first',
        retryable: false,
        details: {},
      },
    });
    await expect(client.deleteProject('project-1')).rejects.toMatchObject({
      appError: { code: 'CONTEXT_NOT_EMPTY' },
    });
  });

  it('never passes raw FTS options through the search boundary', async () => {
    const call = vi.fn().mockResolvedValue({
      ok: true,
      data: { items: [], nextCursor: null },
    });
    const client = createLibraryClient(call);

    await client.searchCaptures(
      'literal OR terms',
      {
        contextId: null,
        branchName: null,
        captureKinds: ['image'],
        capturedFrom: null,
        capturedTo: null,
      },
      null,
    );

    expect(call).toHaveBeenCalledWith('search_captures', {
      input: {
        query: 'literal OR terms',
        contextId: null,
        branchName: null,
        captureKinds: ['image'],
        capturedFrom: null,
        capturedTo: null,
        cursor: null,
        limit: 50,
      },
    });
  });

  it('delegates deleteCapture with captureId payload', async () => {
    const call = vi.fn().mockResolvedValue({
      ok: true,
      data: { deleted: true },
    });

    const client = createLibraryClient(call);
    const result = await client.deleteCapture('capture-to-delete');

    expect(result).toEqual({ deleted: true });
    expect(call).toHaveBeenCalledWith('delete_capture', {
      input: {
        captureId: 'capture-to-delete',
      },
    });
  });
});
