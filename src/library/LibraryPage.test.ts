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
import { LibraryCommandError, type LibraryClient } from './library-client';
import type { SettingsClient } from '../settings/settings-client';

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

const audioCapture: CaptureSummary = {
  id: 'capture-audio',
  kind: 'audio',
  context: project,
  branchName: 'main',
  capturedAt: '2026-09-04T21:48:00Z',
  textExcerpt: null,
  caption: "Et l'autre, on se rend à revoir",
  captionSource: 'transcript_generated',
  media: {
    mediaId: 'media-audio',
    kind: 'audio',
    previewUri: 'lyn-media://capture/media-audio',
    durationMs: 16000,
    widthPx: null,
    heightPx: null,
    available: true,
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
          detail(
            id === textCapture.id
              ? textCapture
              : id === audioCapture.id
                ? audioCapture
                : imageCapture,
          ),
        ),
      ),
    playMedia: vi.fn().mockResolvedValue({ playing: true, durationMs: 1000 }),
    stopPlayback: vi
      .fn()
      .mockResolvedValue({ playing: false, durationMs: null }),
    openMedia: vi.fn().mockResolvedValue({ opened: true }),
    deleteCapture: vi.fn().mockResolvedValue({ deleted: true }),
    ...overrides,
  };
}

describe('responsive Library', () => {
  it('opens Settings from the persistent Library navigation', async () => {
    const settings: SettingsClient = {
      get: vi.fn().mockResolvedValue({
        globalShortcut: 'Control+Shift+Space',
        providerTieBreakOrder: [
          'vscode',
          'cursor',
          'browser',
          'shell',
          'foreground_window',
        ],
        theme: 'system',
        localSpeechEnabled: false,
        language: 'english',
      }),
      update: vi.fn(),
      version: vi.fn().mockResolvedValue({ version: '0.6.4' }),
    };
    render(LibraryPage, { client: createClient(), settings });
    await screen.findByRole('heading', { name: 'Recent' });

    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));

    expect(
      await screen.findByRole('heading', { name: 'Settings' }),
    ).toBeVisible();
    expect(settings.get).toHaveBeenCalledOnce();
  });

  it('translates sidebar text to French when set in settings', async () => {
    const updateMock = vi.fn().mockImplementation((patch) =>
      Promise.resolve({
        globalShortcut: 'Control+Shift+Space',
        providerTieBreakOrder: [
          'vscode',
          'cursor',
          'browser',
          'shell',
          'foreground_window',
        ],
        theme: 'system',
        localSpeechEnabled: false,
        language: 'english',
        ...patch,
      }),
    );
    const settings: SettingsClient = {
      get: vi.fn().mockResolvedValue({
        globalShortcut: 'Control+Shift+Space',
        providerTieBreakOrder: [
          'vscode',
          'cursor',
          'browser',
          'shell',
          'foreground_window',
        ],
        theme: 'system',
        localSpeechEnabled: false,
        language: 'english',
      }),
      update: updateMock,
      version: vi.fn().mockResolvedValue({ version: '0.6.4' }),
    };
    render(LibraryPage, { client: createClient(), settings });

    // Initial sidebar in English
    expect(await screen.findByRole('button', { name: 'Recent' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'All captures' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Search' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Settings' })).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Projects' })).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Contexts' })).toBeVisible();

    // Open settings and switch language to French
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    const frenchBtn = await screen.findByRole('button', { name: 'Français' });
    await fireEvent.click(frenchBtn);

    // Sidebar navigation should update to French
    expect(
      await screen.findByRole('button', { name: 'Récents' }),
    ).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Toutes les captures' }),
    ).toBeVisible();
    expect(screen.getByRole('button', { name: 'Recherche' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Paramètres' })).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Projets' })).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Contextes' })).toBeVisible();

    // Switch back to English
    const englishBtn = screen.getByRole('button', { name: 'English' });
    await fireEvent.click(englishBtn);

    expect(await screen.findByRole('button', { name: 'Recent' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'All captures' })).toBeVisible();
  });

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

    await screen.findByRole('heading', { name: 'First line' });
    expect(
      screen.getByRole('button', { name: 'Back to Recent' }),
    ).toBeVisible();
    expect(container.querySelector('.detail-text')).toHaveTextContent(
      'First line Second line',
    );
    expect(screen.getByText('Code')).toBeVisible();
    expect((await axe.run(container)).violations).toEqual([]);
    expect(screen.queryByText('Library')).not.toBeInTheDocument();
    expect(container.querySelector('.library-brand img')).toHaveAttribute(
      'src',
      expect.stringContaining('lyn-icon'),
    );
  });

  it('copies the untruncated body from the stream without opening the capture', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } });
    const client = createClient({
      listCaptures: vi.fn().mockResolvedValue({
        items: [{ ...textCapture, textExcerpt: 'First line…' }],
        nextCursor: null,
      }),
    });

    render(LibraryPage, { client });
    const copy = await screen.findByRole('button', {
      name: /Copy text — Lyn/,
    });
    await fireEvent.click(copy);

    // The row excerpt is truncated, so the exact body must come from detail.
    expect(client.getCapture).toHaveBeenCalledWith(textCapture.id);
    expect(writeText).toHaveBeenCalledWith('First line\nSecond line');
    await waitFor(() =>
      expect(
        screen
          .getAllByRole('status')
          .some((status) => status.textContent?.includes('Copied')),
      ).toBe(true),
    );
    // Copying must never take over the detail pane.
    expect(
      screen.queryByRole('heading', { name: 'First line' }),
    ).not.toBeInTheDocument();
    vi.unstubAllGlobals();
  });

  it('offers copy only for captures that carry text', async () => {
    const client = createClient({
      listCaptures: vi.fn().mockResolvedValue({
        items: [
          textCapture,
          { ...imageCapture, caption: null, id: 'capture-no-caption' },
        ],
        nextCursor: null,
      }),
    });

    render(LibraryPage, { client });
    await screen.findByRole('button', { name: /text capture in Lyn/i });

    expect(screen.getAllByRole('button', { name: /Copy text/ })).toHaveLength(
      1,
    );
  });

  it('copies the exact text body of the open capture to the clipboard', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } });

    render(LibraryPage, { client: createClient() });
    await fireEvent.click(
      await screen.findByRole('button', { name: /text capture in Lyn/i }),
    );

    const copy = await screen.findByRole('button', { name: 'Copy text' });
    await fireEvent.click(copy);

    expect(writeText).toHaveBeenCalledWith('First line\nSecond line');
    await waitFor(() =>
      expect(
        screen
          .getAllByRole('status')
          .some((status) => status.textContent?.includes('Copied')),
      ).toBe(true),
    );
    vi.unstubAllGlobals();
  });

  it('orders browsing oldest first and search newest first', async () => {
    const older = { ...textCapture, id: 'capture-older' };
    const newer = { ...imageCapture, id: 'capture-newer' };
    const client = createClient({
      // The backend always pages newest first.
      listCaptures: vi
        .fn()
        .mockResolvedValue({ items: [newer, older], nextCursor: null }),
      searchCaptures: vi.fn().mockResolvedValue({
        items: [
          { capture: newer, snippet: 'newer hit' },
          { capture: older, snippet: 'older hit' },
        ],
        nextCursor: null,
      }),
    });
    const { container } = render(LibraryPage, { client });

    await waitFor(() =>
      expect(
        Array.from(container.querySelectorAll('.capture-row')).map(
          (row) => row.id,
        ),
      ).toEqual(['capture-row-capture-older', 'capture-row-capture-newer']),
    );

    await fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await fireEvent.input(await screen.findByRole('searchbox'), {
      target: { value: 'hit' },
    });

    await waitFor(() => {
      const searching = Array.from(
        container.querySelectorAll('.capture-row'),
      ).map((row) => row.id);
      expect(searching).toEqual([
        'capture-row-capture-newer',
        'capture-row-capture-older',
      ]);
    });
  });

  it('deletes from the stream only after an inline confirmation', async () => {
    const deleteCapture = vi.fn().mockResolvedValue({ deleted: true });
    const client = createClient({
      deleteCapture,
      listCaptures: vi
        .fn()
        .mockResolvedValue({ items: [textCapture], nextCursor: null }),
    });
    render(LibraryPage, { client });

    const rowDelete = await screen.findByRole('button', {
      name: /Delete capture — Lyn, .*/,
    });
    await fireEvent.click(rowDelete);

    // A single click must never destroy a capture.
    expect(deleteCapture).not.toHaveBeenCalled();

    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() =>
      expect(deleteCapture).toHaveBeenCalledWith('capture-text'),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole('button', { name: /text capture in Lyn/i }),
      ).not.toBeInTheDocument(),
    );
  });

  it('abandons a stream deletion on cancel and on Escape', async () => {
    const deleteCapture = vi.fn().mockResolvedValue({ deleted: true });
    const client = createClient({
      deleteCapture,
      listCaptures: vi
        .fn()
        .mockResolvedValue({ items: [textCapture], nextCursor: null }),
    });
    render(LibraryPage, { client });

    const rowDelete = await screen.findByRole('button', {
      name: /Delete capture — Lyn, .*/,
    });
    await fireEvent.click(rowDelete);
    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('button', { name: 'Delete' })).toBeNull();

    await fireEvent.click(
      await screen.findByRole('button', { name: /Delete capture — Lyn, .*/ }),
    );
    await fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: 'Delete' })).toBeNull(),
    );
    expect(deleteCapture).not.toHaveBeenCalled();
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

    await fireEvent.click(
      screen.getByRole('button', { name: 'Load earlier captures' }),
    );
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
    await fireEvent.click(screen.getByRole('button', { name: 'Image' }));

    await waitFor(() => {
      const call = searchCaptures.mock.calls.at(-1);
      expect(call?.[1].contextId).toBe(project.id);
      expect(call?.[1].captureKinds).toEqual(['image']);
    });
    expect(screen.getByText('Filters · 2')).toBeVisible();
    expect((await axe.run(container)).violations).toEqual([]);
  });

  it('uses date presets, reveals custom dates only when needed, and clears filters', async () => {
    const searchCaptures = vi.fn().mockResolvedValue({
      items: [],
      nextCursor: null,
    });
    const client = createClient({ searchCaptures });
    render(LibraryPage, { client });
    await screen.findByRole('heading', { name: 'Recent' });
    await fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await fireEvent.click(screen.getByText('Filters'));

    expect(screen.queryByLabelText('From')).not.toBeInTheDocument();
    await fireEvent.change(screen.getByRole('combobox', { name: 'Date' }), {
      target: { value: 'custom' },
    });
    expect(screen.getByLabelText('From')).toBeVisible();
    expect(screen.getByLabelText('To')).toBeVisible();

    await fireEvent.click(screen.getByRole('button', { name: 'Audio' }));
    expect(screen.getByText('Filters · 2')).toBeVisible();
    await fireEvent.click(
      screen.getByRole('button', { name: 'Clear filters' }),
    );

    expect(screen.getByText('Filters')).toBeVisible();
    expect(screen.queryByLabelText('From')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Audio' })).toHaveAttribute(
      'aria-pressed',
      'false',
    );
  });

  it('closes the filter popover when focus moves to an outside pointer target', async () => {
    const client = createClient();
    const { container } = render(LibraryPage, { client });
    await screen.findByRole('heading', { name: 'Recent' });
    await fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await fireEvent.click(screen.getByText('Filters'));
    const filters =
      container.querySelector<HTMLDetailsElement>('.search-filters');
    expect(filters?.open).toBe(true);

    await fireEvent.pointerDown(
      screen.getByRole('heading', { name: 'Search' }),
    );

    expect(filters?.open).toBe(false);
  });

  it('closes filters with Escape and returns focus to the trigger', async () => {
    const client = createClient();
    const { container } = render(LibraryPage, { client });
    await screen.findByRole('heading', { name: 'Recent' });
    await fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    const trigger = screen.getByText('Filters');
    await fireEvent.click(trigger);

    await fireEvent.keyDown(document, { key: 'Escape' });

    expect(
      container.querySelector<HTMLDetailsElement>('.search-filters')?.open,
    ).toBe(false);
    expect(trigger).toHaveFocus();
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

  it('surfaces voice playback failures inside the open detail pane', async () => {
    const playMedia = vi.fn().mockRejectedValue(
      new LibraryCommandError({
        code: 'AUDIO_PLAYBACK_FAILED',
        message: 'The audio could not be played',
        retryable: true,
        details: {},
      }),
    );
    const client = createClient({
      listCaptures: vi.fn().mockResolvedValue({
        items: [audioCapture],
        nextCursor: null,
      }),
      playMedia,
    });
    const { container } = render(LibraryPage, { client });

    await fireEvent.click(
      await screen.findByRole('button', { name: /audio capture in Lyn/i }),
    );
    const play = await screen.findByRole('button', { name: 'Play voice note' });
    await fireEvent.click(play);
    await waitFor(() => expect(playMedia).toHaveBeenCalledWith('media-audio'));
    await waitFor(() =>
      expect(container.querySelector('.library-error')).toHaveTextContent(
        'The audio could not be played',
      ),
    );
    expect(
      screen.getByRole('button', { name: 'Play voice note' }),
    ).toBeVisible();
  });

  it('deletes capture after user confirmation and removes it from stream', async () => {
    const deleteCapture = vi.fn().mockResolvedValue({ deleted: true });
    const client = createClient({ deleteCapture });
    render(LibraryPage, { client });

    await fireEvent.click(
      await screen.findByRole('button', { name: /text capture in Lyn/i }),
    );
    expect(screen.getByRole('heading', { name: 'First line' })).toBeVisible();

    const deleteBtn = await screen.findByRole('button', {
      name: 'Delete capture',
    });
    await fireEvent.click(deleteBtn);

    // Confirmation appears
    expect(screen.getByText('Delete this capture permanently?')).toBeVisible();

    const confirmBtn = screen.getByRole('button', { name: 'Delete' });
    await fireEvent.click(confirmBtn);

    await waitFor(() =>
      expect(deleteCapture).toHaveBeenCalledWith('capture-text'),
    );

    // Detail closed, capture removed from list
    await waitFor(() =>
      expect(
        screen.queryByRole('button', { name: /text capture in Lyn/i }),
      ).not.toBeInTheDocument(),
    );
  });

  it('cancels deletion confirmation when cancel is clicked', async () => {
    const deleteCapture = vi.fn().mockResolvedValue({ deleted: true });
    const client = createClient({ deleteCapture });
    render(LibraryPage, { client });

    await fireEvent.click(
      await screen.findByRole('button', { name: /text capture in Lyn/i }),
    );
    const deleteBtn = await screen.findByRole('button', {
      name: 'Delete capture',
    });
    await fireEvent.click(deleteBtn);

    expect(screen.getByText('Delete this capture permanently?')).toBeVisible();

    const cancelBtn = screen.getByRole('button', { name: 'Cancel' });
    await fireEvent.click(cancelBtn);

    expect(
      screen.queryByText('Delete this capture permanently?'),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Delete capture' }),
    ).toBeVisible();
    expect(deleteCapture).not.toHaveBeenCalled();
  });
});
