import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import axe from 'axe-core';
import { describe, expect, it, vi } from 'vitest';

import type { AppError, CaptureSession, ContextRef } from '../lib/ipc-types';
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
    createStandaloneContext: vi.fn().mockResolvedValue(inbox),
    selectContext: vi.fn().mockResolvedValue(resolvedSession()),
    saveText: vi.fn().mockResolvedValue({
      captureId: 'capture-1',
      capturedAt: '2026-08-29T10:00:00Z',
      enrichmentScheduled: false,
    }),
    cancel: vi.fn().mockResolvedValue({ cancelled: true }),
    dismissPopup: vi.fn().mockResolvedValue({
      dismissed: true,
      focusRestored: true,
    }),
    onSessionReady: vi.fn().mockResolvedValue(vi.fn()),
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
});
