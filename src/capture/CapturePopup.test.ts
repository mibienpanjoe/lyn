import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import axe from 'axe-core';
import { describe, expect, it, vi } from 'vitest';

import type {
  AppError,
  CaptureSession,
  ContextRef,
  ContextSourceOption,
} from '../lib/ipc-types';
import CapturePopup from './CapturePopup.svelte';
import { CaptureCommandError, type CaptureClient } from './capture-client';

const inbox: ContextRef = {
  id: 'context-inbox',
  kind: 'standalone',
  name: 'Inbox',
};

const requiredSession: CaptureSession = {
  sessionId: 'session-1',
  contextResolution: {
    state: 'required',
    candidate: null,
    selection: null,
  },
  stagedMedia: null,
  recordingState: { state: 'idle' },
};

const liveSource: ContextSourceOption = {
  sourceId: 'source-1',
  kind: 'integrated_terminal',
  provider: 'vscode',
  applicationName: 'VS Code',
  label: 'Lyn · main',
  context: { id: 'context-lyn', kind: 'project', name: 'Lyn' },
  branchName: 'main',
  isForeground: true,
};

function resolvedSession(context = inbox): CaptureSession {
  return {
    ...requiredSession,
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
  };
}

function createClient(overrides: Partial<CaptureClient> = {}): CaptureClient {
  return {
    getActiveSession: vi.fn().mockResolvedValue(resolvedSession()),
    listContexts: vi.fn().mockResolvedValue([inbox]),
    listContextSources: vi.fn().mockResolvedValue({
      liveSources: [],
      savedContexts: [inbox],
    }),
    createStandaloneContext: vi.fn().mockResolvedValue(inbox),
    selectContext: vi.fn().mockResolvedValue(resolvedSession()),
    selectLiveSource: vi.fn().mockResolvedValue(resolvedSession()),
    saveText: vi.fn().mockResolvedValue({
      captureId: 'capture-1',
      capturedAt: '2026-08-29T10:00:00Z',
      enrichmentScheduled: false,
    }),
    stageClipboardImage: vi.fn().mockResolvedValue({
      stagedMediaId: 'staged-image-1',
      kind: 'image',
      previewUri: 'lyn-media://staged/staged-image-1',
      mimeType: 'image/png',
      byteSize: 128,
      durationMs: null,
      widthPx: 2,
      heightPx: 1,
    }),
    saveImage: vi.fn().mockResolvedValue({
      captureId: 'capture-image-1',
      capturedAt: '2026-08-29T10:00:00Z',
      enrichmentScheduled: false,
    }),
    cancel: vi.fn().mockResolvedValue({ cancelled: true }),
    dismissPopup: vi.fn().mockResolvedValue({
      dismissed: true,
      focusRestored: true,
    }),
    onSessionReady: vi.fn().mockResolvedValue(vi.fn()),
    onContextSourcesChanged: vi.fn().mockResolvedValue(vi.fn()),
    ...overrides,
  };
}

describe('quick-capture popup', () => {
  it('focuses the labelled capture input immediately', async () => {
    const client = createClient();

    const { container } = render(CapturePopup, {
      client,
      dismiss: vi.fn(),
    });

    expect(screen.getByRole('textbox', { name: 'Capture text' })).toHaveFocus();
    await waitFor(() => expect(client.getActiveSession).toHaveBeenCalledOnce());
    expect((await axe.run(container)).violations).toEqual([]);
  });

  it('saves the exact draft with Enter and dismisses only after success', async () => {
    const client = createClient();
    const dismiss = vi.fn();
    render(CapturePopup, { client, dismiss });
    const input = screen.getByRole('textbox', { name: 'Capture text' });

    await fireEvent.input(input, { target: { value: '  exact draft\n' } });
    await fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() =>
      expect(client.saveText).toHaveBeenCalledWith(
        'session-1',
        '  exact draft\n',
      ),
    );
    expect(dismiss).toHaveBeenCalledOnce();
  });

  it('stages an image paste without sending bytes and saves the draft as its caption', async () => {
    const client = createClient();
    const dismiss = vi.fn();
    render(CapturePopup, { client, dismiss });
    const input = screen.getByRole('textbox', { name: 'Capture text' });
    await fireEvent.input(input, { target: { value: '  Manual caption  ' } });

    await fireEvent.paste(input, {
      clipboardData: { items: [{ type: 'image/png' }] },
    });

    expect(
      await screen.findByRole('img', { name: 'Screenshot ready to save' }),
    ).toHaveAttribute('src', 'lyn-media://staged/staged-image-1');
    expect(client.stageClipboardImage).toHaveBeenCalledWith('session-1');
    expect(input).toHaveValue('  Manual caption  ');

    await fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() =>
      expect(client.saveImage).toHaveBeenCalledWith(
        'session-1',
        'staged-image-1',
        '  Manual caption  ',
      ),
    );
    expect(client.saveText).not.toHaveBeenCalled();
    expect(dismiss).toHaveBeenCalledOnce();
  });

  it('keeps the screenshot preview and caption when image save fails', async () => {
    const failure: AppError = {
      code: 'MEDIA_FINALIZE_FAILED',
      message: 'The screenshot could not be finalized',
      retryable: true,
      details: {},
    };
    const client = createClient({
      saveImage: vi.fn().mockRejectedValue(new CaptureCommandError(failure)),
    });
    render(CapturePopup, { client, dismiss: vi.fn() });
    const input = screen.getByRole('textbox', { name: 'Capture text' });
    await fireEvent.input(input, { target: { value: 'Keep caption' } });
    await fireEvent.paste(input, {
      clipboardData: { items: [{ type: 'image/png' }] },
    });
    await screen.findByRole('img', { name: 'Screenshot ready to save' });

    await fireEvent.keyDown(input, { key: 'Enter' });

    expect(await screen.findByRole('alert')).toHaveTextContent('finalized');
    expect(
      screen.getByRole('img', { name: 'Screenshot ready to save' }),
    ).toBeVisible();
    expect(input).toHaveValue('Keep caption');
  });

  it('leaves Shift+Enter to insert a newline without saving', async () => {
    const client = createClient();
    render(CapturePopup, { client, dismiss: vi.fn() });
    const input = screen.getByRole('textbox', { name: 'Capture text' });
    const event = new KeyboardEvent('keydown', {
      key: 'Enter',
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });

    input.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(false);
    expect(client.saveText).not.toHaveBeenCalled();
  });

  it('ignores save and cancel shortcuts during IME composition', async () => {
    const client = createClient();
    const dismiss = vi.fn();
    render(CapturePopup, { client, dismiss });
    const input = screen.getByRole('textbox', { name: 'Capture text' });

    await fireEvent.compositionStart(input);
    await fireEvent.keyDown(input, { key: 'Enter' });
    await fireEvent.keyDown(input, { key: 'Escape' });
    await fireEvent.compositionEnd(input);

    expect(client.saveText).not.toHaveBeenCalled();
    expect(client.cancel).not.toHaveBeenCalled();
    expect(dismiss).not.toHaveBeenCalled();
  });

  it('preserves the exact draft and exposes an actionable retry after failure', async () => {
    const storageError: AppError = {
      code: 'STORAGE_WRITE_FAILED',
      message: 'The capture could not be saved',
      retryable: true,
      details: {},
    };
    const saveText = vi
      .fn()
      .mockRejectedValueOnce(new CaptureCommandError(storageError))
      .mockResolvedValueOnce({
        captureId: 'capture-1',
        capturedAt: '2026-08-29T10:00:00Z',
        enrichmentScheduled: false,
      });
    const client = createClient({ saveText });
    const dismiss = vi.fn();
    render(CapturePopup, { client, dismiss });
    const input = screen.getByRole('textbox', { name: 'Capture text' });

    await fireEvent.input(input, { target: { value: '  keep me\n' } });
    await fireEvent.keyDown(input, { key: 'Enter' });

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The capture could not be saved',
    );
    expect(input).toHaveValue('  keep me\n');
    expect(dismiss).not.toHaveBeenCalled();

    await fireEvent.click(screen.getByRole('button', { name: 'Retry save' }));

    await waitFor(() => expect(saveText).toHaveBeenCalledTimes(2));
    expect(saveText).toHaveBeenLastCalledWith('session-1', '  keep me\n');
    expect(dismiss).toHaveBeenCalledOnce();
  });

  it('resolves a required context without changing the draft', async () => {
    const client = createClient({
      getActiveSession: vi.fn().mockResolvedValue(requiredSession),
    });
    render(CapturePopup, { client, dismiss: vi.fn() });
    const input = screen.getByRole('textbox', { name: 'Capture text' });
    await fireEvent.input(input, { target: { value: 'Draft stays' } });

    const contextButton = await screen.findByRole('button', {
      name: 'Choose context',
    });
    await fireEvent.click(contextButton);
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Use context Inbox' }),
    );

    await waitFor(() =>
      expect(client.selectContext).toHaveBeenCalledWith(
        'session-1',
        'context-inbox',
      ),
    );
    expect(input).toHaveValue('Draft stays');
    expect(contextButton).toHaveFocus();
  });

  it('creates and selects a standalone context inline', async () => {
    const created = { ...inbox, id: 'context-new', name: 'Research' };
    const client = createClient({
      getActiveSession: vi.fn().mockResolvedValue(requiredSession),
      listContexts: vi.fn().mockResolvedValue([]),
      createStandaloneContext: vi.fn().mockResolvedValue(created),
      selectContext: vi.fn().mockResolvedValue(resolvedSession(created)),
    });
    render(CapturePopup, { client, dismiss: vi.fn() });

    await fireEvent.click(
      await screen.findByRole('button', { name: 'Choose context' }),
    );
    const nameInput = await screen.findByRole('textbox', {
      name: 'New context name',
    });
    await fireEvent.input(nameInput, { target: { value: 'Research' } });
    await fireEvent.click(
      screen.getByRole('button', { name: 'Create context' }),
    );

    await waitFor(() =>
      expect(client.createStandaloneContext).toHaveBeenCalledWith('Research'),
    );
    expect(client.selectContext).toHaveBeenCalledWith(
      'session-1',
      'context-new',
    );
    expect(
      screen.getByRole('button', { name: 'Context Research. Change context' }),
    ).toBeVisible();
  });

  it('closes the context chooser before Escape cancels the session', async () => {
    const client = createClient();
    const dismiss = vi.fn();
    render(CapturePopup, { client, dismiss });
    const input = screen.getByRole('textbox', { name: 'Capture text' });
    const contextButton = await screen.findByRole('button', {
      name: 'Context Inbox. Change context',
    });

    await fireEvent.click(contextButton);
    expect(
      screen.getByRole('region', { name: 'Choose context' }),
    ).toBeVisible();
    await fireEvent.keyDown(input, { key: 'Escape' });
    expect(
      screen.queryByRole('region', { name: 'Choose context' }),
    ).not.toBeInTheDocument();
    expect(client.cancel).not.toHaveBeenCalled();

    await fireEvent.keyDown(input, { key: 'Escape' });
    await waitFor(() =>
      expect(client.cancel).toHaveBeenCalledWith('session-1'),
    );
    expect(dismiss).toHaveBeenCalledOnce();
  });

  it('groups searchable live and saved contexts and selects the current window without changing the draft', async () => {
    const client = createClient({
      getActiveSession: vi.fn().mockResolvedValue(requiredSession),
      listContextSources: vi.fn().mockResolvedValue({
        liveSources: [liveSource],
        savedContexts: [inbox],
      }),
      selectLiveSource: vi
        .fn()
        .mockResolvedValue(resolvedSession(liveSource.context)),
    });
    const { container } = render(CapturePopup, { client, dismiss: vi.fn() });
    const draft = screen.getByRole('textbox', { name: 'Capture text' });
    await fireEvent.input(draft, { target: { value: 'Draft stays' } });
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Choose context' }),
    );

    expect(
      screen.getByRole('heading', { name: 'Live sessions' }),
    ).toBeVisible();
    expect(screen.getByText('Current window')).toBeVisible();
    expect(
      screen.getByRole('heading', { name: 'Saved contexts' }),
    ).toBeVisible();
    const search = screen.getByRole('searchbox', { name: 'Search contexts' });
    expect(search).toHaveFocus();
    await fireEvent.input(search, { target: { value: 'Lyn' } });
    expect(
      screen.queryByRole('button', { name: 'Use context Inbox' }),
    ).not.toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: /Lyn · main/ }));

    await waitFor(() =>
      expect(client.selectLiveSource).toHaveBeenCalledWith(
        'session-1',
        'source-1',
      ),
    );
    expect(draft).toHaveValue('Draft stays');
    expect((await axe.run(container)).violations).toEqual([]);
  });

  it('keeps the chooser and draft available when a live source becomes stale', async () => {
    const stale: AppError = {
      code: 'CONTEXT_SOURCE_STALE',
      message: 'The selected context source is stale',
      retryable: true,
      details: {},
    };
    const client = createClient({
      getActiveSession: vi.fn().mockResolvedValue(requiredSession),
      listContextSources: vi.fn().mockResolvedValue({
        liveSources: [liveSource],
        savedContexts: [inbox],
      }),
      selectLiveSource: vi
        .fn()
        .mockRejectedValue(new CaptureCommandError(stale)),
    });
    render(CapturePopup, { client, dismiss: vi.fn() });
    const draft = screen.getByRole('textbox', { name: 'Capture text' });
    await fireEvent.input(draft, { target: { value: 'Do not lose this' } });
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Choose context' }),
    );
    await fireEvent.click(screen.getByRole('button', { name: /Lyn · main/ }));

    expect(await screen.findByRole('alert')).toHaveTextContent('stale');
    expect(
      screen.getByRole('region', { name: 'Choose context' }),
    ).toBeVisible();
    expect(
      screen.getByRole('button', { name: /Context source stale/ }),
    ).toBeVisible();
    expect(draft).toHaveValue('Do not lose this');
  });
});
