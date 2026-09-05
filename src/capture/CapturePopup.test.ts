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
  label: 'Lyn',
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
    discardStagedMedia: vi.fn().mockResolvedValue(resolvedSession()),
    saveImage: vi.fn().mockResolvedValue({
      captureId: 'capture-image-1',
      capturedAt: '2026-08-29T10:00:00Z',
      enrichmentScheduled: false,
    }),
    startAudioRecording: vi.fn().mockResolvedValue({
      state: 'recording',
      elapsedMs: 0,
    }),
    stopAudioRecording: vi.fn().mockResolvedValue({
      stagedMediaId: 'staged-audio-1',
      kind: 'audio',
      previewUri: 'lyn-media://staged/staged-audio-1',
      mimeType: 'audio/wav',
      byteSize: 3200,
      durationMs: 1250,
      widthPx: null,
      heightPx: null,
    }),
    playStagedAudio: vi.fn().mockResolvedValue({
      playing: true,
      durationMs: 1250,
    }),
    stopAudioPlayback: vi.fn().mockResolvedValue({
      playing: false,
      durationMs: null,
    }),
    saveAudio: vi.fn().mockResolvedValue({
      captureId: 'capture-audio-1',
      capturedAt: '2026-08-29T10:00:00Z',
      enrichmentScheduled: false,
    }),
    setPopupLayout: vi.fn().mockResolvedValue({ layout: 'compact' }),
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
  it('keeps required context and optional media actions visually quiet', async () => {
    const client = createClient({
      getActiveSession: vi.fn().mockResolvedValue(requiredSession),
    });
    const { container } = render(CapturePopup, {
      client,
      dismiss: vi.fn(),
    });

    const context = await screen.findByRole('button', {
      name: 'Choose context',
    });
    expect(context).toHaveClass('context-attention');
    expect(context).not.toHaveClass('context-required');
    expect(context.querySelector('.context-status-dot')).toBeInTheDocument();

    expect(screen.getByRole('button', { name: 'Screenshot' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Voice' })).toBeVisible();
    expect(container.querySelector('.lucide-image')).toBeInTheDocument();
    expect(container.querySelector('.lucide-mic')).toBeInTheDocument();

    const keys = Array.from(container.querySelectorAll('.keyboard-hint kbd'));
    expect(keys.map((key) => key.textContent)).toEqual(['⇧', 'Enter', 'Enter']);
    expect(screen.getByText('New line')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Save' })).toBeVisible();
    expect((await axe.run(container)).violations).toEqual([]);
  });

  it('adapts the native window between compact, chooser, image, and audio layouts', async () => {
    const client = createClient();
    render(CapturePopup, { client, dismiss: vi.fn() });

    await waitFor(() =>
      expect(client.setPopupLayout).toHaveBeenCalledWith('compact'),
    );

    await fireEvent.click(
      screen.getByRole('button', { name: 'Context Inbox. Change context' }),
    );
    await waitFor(() =>
      expect(client.setPopupLayout).toHaveBeenLastCalledWith('chooser'),
    );

    await fireEvent.click(screen.getByRole('button', { name: 'Back to note' }));
    await waitFor(() =>
      expect(client.setPopupLayout).toHaveBeenLastCalledWith('compact'),
    );

    await fireEvent.click(screen.getByRole('button', { name: 'Screenshot' }));
    await waitFor(() =>
      expect(client.setPopupLayout).toHaveBeenLastCalledWith('media'),
    );

    await fireEvent.click(screen.getByRole('button', { name: 'Remove image' }));
    await waitFor(() =>
      expect(client.setPopupLayout).toHaveBeenLastCalledWith('compact'),
    );

    await fireEvent.click(screen.getByRole('button', { name: 'Voice' }));
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Stop recording' }),
    );
    await waitFor(() =>
      expect(client.setPopupLayout).toHaveBeenLastCalledWith('audio'),
    );

    await fireEvent.click(
      screen.getByRole('button', { name: 'Remove recording' }),
    );
    await waitFor(() =>
      expect(client.setPopupLayout).toHaveBeenLastCalledWith('compact'),
    );
  });

  it('grows the compact window when inline failure feedback appears', async () => {
    const storageError: AppError = {
      code: 'STORAGE_WRITE_FAILED',
      message: 'The capture could not be saved',
      retryable: true,
      details: {},
    };
    const client = createClient({
      saveText: vi
        .fn()
        .mockRejectedValue(new CaptureCommandError(storageError)),
    });
    render(CapturePopup, { client, dismiss: vi.fn() });

    const input = screen.getByRole('textbox', { name: 'Capture text' });
    await fireEvent.input(input, { target: { value: 'Keep this note' } });
    await fireEvent.keyDown(input, { key: 'Enter' });

    expect(await screen.findByRole('alert')).toBeVisible();
    await waitFor(() =>
      expect(client.setPopupLayout).toHaveBeenLastCalledWith('error'),
    );
  });

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
    expect(
      screen.getByRole('textbox', { name: 'Screenshot caption (optional)' }),
    ).toHaveAttribute('placeholder', 'Add a note about this screenshot…');
    expect(screen.getByText('Screenshot preview')).toBeVisible();
    expect(
      screen.queryByRole('button', { name: 'Voice' }),
    ).not.toBeInTheDocument();
    expect(
      screen
        .getByRole('img', { name: 'Screenshot ready to save' })
        .compareDocumentPosition(input) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

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

  it('checks the native clipboard when WebKit omits pasted image items', async () => {
    const client = createClient();
    render(CapturePopup, { client, dismiss: vi.fn() });
    const input = screen.getByRole('textbox', { name: 'Capture text' });
    await waitFor(() => expect(client.getActiveSession).toHaveBeenCalled());

    await fireEvent.paste(input, {
      clipboardData: { items: [], types: [], files: [] },
    });

    expect(
      await screen.findByRole('img', { name: 'Screenshot ready to save' }),
    ).toBeVisible();
    expect(client.stageClipboardImage).toHaveBeenCalledWith('session-1');
  });

  it('pastes a new screenshot with visible feedback', async () => {
    const replacement = {
      stagedMediaId: 'staged-image-2',
      kind: 'image' as const,
      previewUri: 'lyn-media://staged/staged-image-2',
      mimeType: 'image/png' as const,
      byteSize: 256,
      durationMs: null,
      widthPx: 4,
      heightPx: 2,
    };
    const stageClipboardImage = vi
      .fn()
      .mockResolvedValueOnce({
        stagedMediaId: 'staged-image-1',
        kind: 'image',
        previewUri: 'lyn-media://staged/staged-image-1',
        mimeType: 'image/png',
        byteSize: 128,
        durationMs: null,
        widthPx: 2,
        heightPx: 1,
      })
      .mockResolvedValueOnce(replacement);
    const client = createClient({ stageClipboardImage });
    render(CapturePopup, { client, dismiss: vi.fn() });
    const input = screen.getByRole('textbox', { name: 'Capture text' });
    await fireEvent.input(input, { target: { value: 'Keep this caption' } });
    await fireEvent.paste(input, {
      clipboardData: { items: [{ type: 'image/png' }] },
    });
    await screen.findByRole('img', { name: 'Screenshot ready to save' });

    await fireEvent.click(
      screen.getByRole('button', { name: 'Paste new image' }),
    );

    await waitFor(() => expect(stageClipboardImage).toHaveBeenCalledTimes(2));
    expect(
      screen.getByRole('img', { name: 'Screenshot ready to save' }),
    ).toHaveAttribute('src', replacement.previewUri);
    expect(screen.getByRole('status')).toHaveTextContent('Screenshot replaced');
    expect(input).toHaveValue('Keep this caption');
  });

  it('removes a screenshot and keeps its caption as a text draft', async () => {
    const client = createClient();
    render(CapturePopup, { client, dismiss: vi.fn() });
    const input = screen.getByRole('textbox', { name: 'Capture text' });
    await fireEvent.input(input, { target: { value: 'Continue as text' } });
    await fireEvent.paste(input, {
      clipboardData: { items: [{ type: 'image/png' }] },
    });
    await screen.findByRole('img', { name: 'Screenshot ready to save' });

    await fireEvent.click(screen.getByRole('button', { name: 'Remove image' }));

    await waitFor(() =>
      expect(client.discardStagedMedia).toHaveBeenCalledWith(
        'session-1',
        'staged-image-1',
      ),
    );
    expect(screen.getByRole('textbox', { name: 'Capture text' })).toHaveValue(
      'Continue as text',
    );
    expect(screen.queryByRole('img')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Voice' })).toBeVisible();
  });

  it('silently preserves ordinary paste when the native clipboard has no image', async () => {
    const unsupported: AppError = {
      code: 'UNSUPPORTED_CLIPBOARD_CONTENT',
      message: 'The clipboard does not contain a supported image',
      retryable: true,
      details: {},
    };
    const client = createClient({
      stageClipboardImage: vi
        .fn()
        .mockRejectedValue(new CaptureCommandError(unsupported)),
    });
    render(CapturePopup, { client, dismiss: vi.fn() });
    const input = screen.getByRole('textbox', { name: 'Capture text' });
    await waitFor(() => expect(client.getActiveSession).toHaveBeenCalled());

    await fireEvent.paste(input, {
      clipboardData: { items: [], types: ['text/plain'], files: [] },
    });

    await waitFor(() =>
      expect(client.stageClipboardImage).toHaveBeenCalledWith('session-1'),
    );
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
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

  it('records, plays, stops, and saves a voice note with its exact caption', async () => {
    const client = createClient();
    const dismiss = vi.fn();
    render(CapturePopup, { client, dismiss });
    const input = screen.getByRole('textbox', { name: 'Capture text' });
    await fireEvent.input(input, { target: { value: '  Voice caption  ' } });

    await fireEvent.click(screen.getByRole('button', { name: 'Voice' }));
    expect(client.startAudioRecording).toHaveBeenCalledWith('session-1');
    expect(await screen.findByRole('status', { name: '' })).toHaveTextContent(
      'Recording voice note',
    );

    await fireEvent.click(
      screen.getByRole('button', { name: 'Stop recording' }),
    );
    expect(client.stopAudioRecording).toHaveBeenCalledWith('session-1');
    expect(await screen.findByText('0:01')).toBeVisible();

    await fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    expect(client.playStagedAudio).toHaveBeenCalledWith(
      'session-1',
      'staged-audio-1',
    );
    await fireEvent.click(
      screen.getByRole('button', { name: 'Stop playback' }),
    );
    expect(client.stopAudioPlayback).toHaveBeenCalledWith('staged-audio-1');

    await fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() =>
      expect(client.saveAudio).toHaveBeenCalledWith(
        'session-1',
        'staged-audio-1',
        '  Voice caption  ',
      ),
    );
    expect(client.saveText).not.toHaveBeenCalled();
    expect(dismiss).toHaveBeenCalledOnce();
  });

  it('removes a voice recording and keeps its caption as a text draft', async () => {
    const client = createClient();
    render(CapturePopup, { client, dismiss: vi.fn() });
    const input = screen.getByRole('textbox', { name: 'Capture text' });
    await fireEvent.input(input, { target: { value: 'Use as text instead' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Voice' }));
    await fireEvent.click(
      screen.getByRole('button', { name: 'Stop recording' }),
    );

    await fireEvent.click(
      screen.getByRole('button', { name: 'Remove recording' }),
    );

    await waitFor(() =>
      expect(client.discardStagedMedia).toHaveBeenCalledWith(
        'session-1',
        'staged-audio-1',
      ),
    );
    expect(screen.getByRole('textbox', { name: 'Capture text' })).toHaveValue(
      'Use as text instead',
    );
    expect(
      screen.queryByRole('button', { name: 'Play' }),
    ).not.toBeInTheDocument();
  });

  it('preserves a staged voice note and caption when audio save fails', async () => {
    const failure: AppError = {
      code: 'STORAGE_WRITE_FAILED',
      message: 'The voice note could not be saved',
      retryable: true,
      details: {},
    };
    const client = createClient({
      saveAudio: vi.fn().mockRejectedValue(new CaptureCommandError(failure)),
    });
    render(CapturePopup, { client, dismiss: vi.fn() });
    const input = screen.getByRole('textbox', { name: 'Capture text' });
    await fireEvent.input(input, { target: { value: 'Keep voice caption' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Voice' }));
    await fireEvent.click(
      screen.getByRole('button', { name: 'Stop recording' }),
    );

    await fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('could not');
    expect(screen.getByRole('button', { name: 'Play' })).toBeVisible();
    expect(input).toHaveValue('Keep voice caption');
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
    const contextButton = await screen.findByRole('button', {
      name: 'Context Inbox. Change context',
    });

    await fireEvent.click(contextButton);
    expect(
      screen.getByRole('region', { name: 'Choose context' }),
    ).toBeVisible();
    await fireEvent.keyDown(
      screen.getByRole('searchbox', { name: 'Search contexts' }),
      { key: 'Escape' },
    );
    expect(
      screen.queryByRole('region', { name: 'Choose context' }),
    ).not.toBeInTheDocument();
    expect(client.cancel).not.toHaveBeenCalled();

    await fireEvent.keyDown(
      screen.getByRole('textbox', { name: 'Capture text' }),
      { key: 'Escape' },
    );
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
    await fireEvent.click(
      screen.getByRole('button', {
        name: 'Use live context Lyn, branch main, current window',
      }),
    );

    await waitFor(() =>
      expect(client.selectLiveSource).toHaveBeenCalledWith(
        'session-1',
        'source-1',
      ),
    );
    expect(draft).toHaveValue('Draft stays');
    expect(container.textContent).not.toContain('source-1');
    expect(container.textContent).not.toContain('/home/');
    expect(container.innerHTML).not.toMatch(/correlation|pid=|process id/i);
    expect((await axe.run(container)).violations).toEqual([]);
  });

  it('uses a focused chooser mode and returns to the unchanged note', async () => {
    const client = createClient({
      getActiveSession: vi.fn().mockResolvedValue(requiredSession),
      listContextSources: vi.fn().mockResolvedValue({
        liveSources: [liveSource],
        savedContexts: [inbox],
      }),
    });
    const { container } = render(CapturePopup, {
      client,
      dismiss: vi.fn(),
    });
    const draft = screen.getByRole('textbox', { name: 'Capture text' });
    await fireEvent.input(draft, { target: { value: 'Keep this draft' } });

    await fireEvent.click(
      await screen.findByRole('button', { name: 'Choose context' }),
    );

    expect(container.querySelector('.capture-popup')).toHaveClass(
      'chooser-open',
    );
    expect(
      screen.queryByRole('textbox', { name: 'Capture text' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Save' }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Back to note' })).toBeVisible();

    await fireEvent.click(screen.getByRole('button', { name: 'Back to note' }));

    expect(container.querySelector('.capture-popup')).not.toHaveClass(
      'chooser-open',
    );
    expect(screen.getByRole('textbox', { name: 'Capture text' })).toHaveValue(
      'Keep this draft',
    );
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
    await fireEvent.click(
      screen.getByRole('button', {
        name: 'Use live context Lyn, branch main, current window',
      }),
    );

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
