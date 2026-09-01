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
});
