import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import axe from 'axe-core';
import { describe, expect, it, vi } from 'vitest';

import type {
  CaptureDetail,
  CaptureSummary,
  ContextRef,
  LibraryScope,
  SearchResultItem,
} from '../lib/ipc-types';
import LibraryPage from './LibraryPage.svelte';
import type { LibraryClient } from './library-client';

const project: ContextRef = { id: 'project-1', kind: 'project', name: 'Lyn' };
const inbox: ContextRef = {
  id: 'inbox-1',
  kind: 'standalone',
  name: 'Inbox',
};

const textCapture: CaptureSummary = {
  id: 'capture-text',
  kind: 'text',
  context: project,
  branchName: 'main',
  capturedAt: '2026-09-01T12:00:00Z',
  textExcerpt: 'First line\nSecond line',
  caption: null,
  captionSource: null,
  media: null,
};

const imageCapture: CaptureSummary = {
  id: 'capture-image',
  kind: 'image',
  context: project,
  branchName: 'feature/library',
  capturedAt: '2026-09-01T11:00:00Z',
  textExcerpt: null,
  caption: 'Exact screenshot caption',
  captionSource: 'user',
  media: {
    mediaId: 'media-image',
    kind: 'image',
    previewUri: 'lyn-media://capture/media-image',
    durationMs: null,
    widthPx: 640,
    heightPx: 480,
    available: false,
  },
};

function detail(capture: CaptureSummary): CaptureDetail {
  return {
    ...capture,
    textBody: capture.kind === 'text' ? 'First line\nSecond line' : null,
    sourceApp: 'Code',
    sourceWindowTitle: null,
    updatedAt: capture.capturedAt,
    enrichmentStatus: 'not_requested',
  };
}

function createClient(overrides: Partial<LibraryClient> = {}): LibraryClient {
  return {
    listContexts: vi.fn().mockResolvedValue([project, inbox]),
    listCaptures: vi.fn().mockResolvedValue({
      items: [textCapture, imageCapture],
      nextCursor: null,
    }),
    searchCaptures: vi.fn().mockResolvedValue({
      items: [],
      nextCursor: null,
    }),
    getCapture: vi
      .fn()
      .mockImplementation((id) =>
        Promise.resolve(
          detail(id === textCapture.id ? textCapture : imageCapture),
        ),
      ),
    playMedia: vi.fn().mockResolvedValue({ playing: true, durationMs: 1000 }),
    stopPlayback: vi
      .fn()
      .mockResolvedValue({ playing: false, durationMs: null }),
    openMedia: vi.fn().mockResolvedValue({ opened: true }),
    ...overrides,
  };
}

describe('responsive Library', () => {
  it('renders chronological navigation and faithful text detail accessibly', async () => {
    const client = createClient();
    const { container } = render(LibraryPage, { client });

    expect(
      await screen.findByRole('heading', { name: 'Recent' }),
    ).toBeVisible();
    const row = await screen.findByRole('button', {
      name: /text capture in Lyn/i,
    });
    await fireEvent.click(row);

    await screen.findByRole('heading', { name: 'Lyn' });
    expect(container.querySelector('.detail-text')).toHaveTextContent(
      'First line Second line',
    );
    expect(screen.getByText('Code')).toBeVisible();
    expect((await axe.run(container)).violations).toEqual([]);
  });

  it('keeps project branches in one stream and applies branch only as a filter', async () => {
    const listCaptures = vi.fn().mockResolvedValue({
      items: [textCapture, imageCapture],
      nextCursor: null,
    });
    const client = createClient({ listCaptures });
    render(LibraryPage, { client });
    await screen.findByRole('heading', { name: 'Recent' });

    await fireEvent.click(screen.getByRole('button', { name: 'Lyn' }));
    const branch = await screen.findByRole('combobox', { name: 'Branch' });
    await fireEvent.change(branch, { target: { value: 'feature/library' } });

    await waitFor(() => {
      const last = listCaptures.mock.calls.at(-1);
      expect(last?.[0]).toEqual({ kind: 'context', contextId: 'project-1' });
      expect(last?.[1]).toEqual({ branchName: 'feature/library' });
    });
  });

  it('pages without replacing rows and preserves missing-media metadata', async () => {
    const next = { ...imageCapture, id: 'capture-image-next' };
    const listCaptures = vi
      .fn()
      .mockResolvedValueOnce({ items: [textCapture], nextCursor: 'next-page' })
      .mockResolvedValueOnce({ items: [next], nextCursor: null });
    const client = createClient({ listCaptures });
    render(LibraryPage, { client });
    await screen.findByText(/First line Second line/);

    await fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
    await waitFor(() =>
      expect(screen.getAllByText(/First line Second line/)).toHaveLength(1),
    );
    const imageRow = await screen.findByRole('button', {
      name: /image capture.*Exact screenshot caption/i,
    });
    await fireEvent.click(imageRow);

    expect(await screen.findByText('Screenshot unavailable')).toBeVisible();
    expect(screen.getAllByText('Exact screenshot caption')).toHaveLength(2);
  });

  it('pages search results with the active literal query and filters', async () => {
    const next = { ...imageCapture, id: 'capture-image-next' };
    const searchCaptures = vi
      .fn()
      .mockResolvedValueOnce({
        items: [
          {
            capture: textCapture,
            matchedField: 'text_body',
            snippet: 'alpha first',
          },
        ],
        nextCursor: 'search-page-2',
      })
      .mockResolvedValueOnce({
        items: [
          {
            capture: next,
            matchedField: 'caption',
            snippet: 'alpha second',
          },
        ],
        nextCursor: null,
      });
    const client = createClient({ searchCaptures });
    render(LibraryPage, { client });
    await screen.findByRole('button', { name: /text capture in Lyn/i });
    await fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await fireEvent.input(
      screen.getByRole('searchbox', { name: 'Search captures' }),
      { target: { value: 'alpha' } },
    );
    await new Promise((resolve) => setTimeout(resolve, 300));

    await fireEvent.click(
      await screen.findByRole('button', { name: 'Load more' }),
    );

    await waitFor(() => {
      expect(searchCaptures).toHaveBeenLastCalledWith(
        'alpha',
        expect.objectContaining({ contextId: null }),
        'search-page-2',
      );
      expect(screen.getByText('second')).toBeVisible();
    });
  });

  it('renders empty and recoverable error states', async () => {
    const listCaptures = vi
      .fn()
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValueOnce({ items: [], nextCursor: null });
    const client = createClient({ listCaptures });
    render(LibraryPage, { client });

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Captures could not be loaded.',
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Retry' }));

    expect(
      await screen.findByRole('heading', { name: 'Nothing captured yet' }),
    ).toBeVisible();
    expect(listCaptures).toHaveBeenCalledTimes(2);
  });

  it('debounces literal search, highlights snippets, and applies accessible filters', async () => {
    const searchCaptures = vi.fn().mockResolvedValue({
      items: [
        {
          capture: textCapture,
          matchedField: 'text_body',
          snippet: 'First alpha result',
        },
      ],
      nextCursor: null,
    });
    const client = createClient({ searchCaptures });
    const { container } = render(LibraryPage, { client });
    await screen.findByRole('button', { name: /text capture in Lyn/i });

    await fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    const search = screen.getByRole('searchbox', { name: 'Search captures' });
    await fireEvent.input(search, { target: { value: 'alpha' } });
    await new Promise((resolve) => setTimeout(resolve, 300));

    await waitFor(() => expect(searchCaptures).toHaveBeenCalledOnce());
    const highlight = await screen.findByText('alpha');
    expect(highlight.tagName).toBe('MARK');

    await fireEvent.click(screen.getByText('Filters'));
    await fireEvent.change(screen.getByRole('combobox', { name: 'Context' }), {
      target: { value: project.id },
    });
    await fireEvent.click(screen.getByRole('checkbox', { name: 'image' }));

    await waitFor(() => {
      const call = searchCaptures.mock.calls.at(-1);
      expect(call?.[1].contextId).toBe(project.id);
      expect(call?.[1].captureKinds).toEqual(['image']);
    });
    expect((await axe.run(container)).violations).toEqual([]);
  });

  it('ignores a stale search response after a newer query completes', async () => {
    let resolveAlpha:
      | ((value: { items: SearchResultItem[]; nextCursor: null }) => void)
      | undefined;
    const alpha = new Promise<{
      items: SearchResultItem[];
      nextCursor: null;
    }>((resolve) => {
      resolveAlpha = resolve;
    });
    const searchCaptures = vi.fn().mockImplementation((query: string) => {
      if (query === 'alpha') return alpha;
      return Promise.resolve({
        items: [
          {
            capture: { ...textCapture, textExcerpt: 'beta result' },
            matchedField: 'text_body',
            snippet: 'beta result',
          },
        ],
        nextCursor: null,
      });
    });
    const client = createClient({ searchCaptures });
    render(LibraryPage, { client });
    await screen.findByRole('button', { name: /text capture in Lyn/i });
    await fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    const search = screen.getByRole('searchbox', { name: 'Search captures' });

    await fireEvent.input(search, { target: { value: 'alpha' } });
    await new Promise((resolve) => setTimeout(resolve, 300));
    await fireEvent.input(search, { target: { value: 'beta' } });
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(await screen.findByText('beta')).toBeVisible();

    resolveAlpha?.({
      items: [
        {
          capture: { ...textCapture, textExcerpt: 'stale alpha' },
          matchedField: 'text_body',
          snippet: 'stale alpha',
        },
      ],
      nextCursor: null,
    });
    await Promise.resolve();

    expect(screen.getByText('beta')).toBeVisible();
    expect(screen.queryByText('stale alpha')).not.toBeInTheDocument();
  });
});
