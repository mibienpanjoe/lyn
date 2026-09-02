import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import axe from 'axe-core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { AppSettings } from '../lib/ipc-types';
import SettingsPanel from './SettingsPanel.svelte';
import type { SpeechModelClient } from './model-client';
import { SettingsCommandError, type SettingsClient } from './settings-client';

const initial: AppSettings = {
  globalShortcut: 'Control+Shift+Space',
  providerTieBreakOrder: ['vscode', 'shell', 'foreground_window'],
  theme: 'system',
  localSpeechEnabled: false,
};

function client(overrides: Partial<SettingsClient> = {}): SettingsClient {
  return {
    get: vi.fn().mockResolvedValue(initial),
    update: vi.fn().mockImplementation((patch) =>
      Promise.resolve({
        ...initial,
        globalShortcut: patch.globalShortcut ?? initial.globalShortcut,
        providerTieBreakOrder:
          patch.providerTieBreakOrder ?? initial.providerTieBreakOrder,
        theme: patch.theme ?? initial.theme,
      }),
    ),
    ...overrides,
  };
}

const modelClient: SpeechModelClient = {
  status: vi.fn().mockResolvedValue({
    state: 'not_installed',
    modelId: null,
    label: 'Multilingual base',
    downloadedBytes: null,
    totalBytes: null,
    errorCode: null,
  }),
  install: vi.fn().mockResolvedValue({
    accepted: true,
    modelId: 'whisper-base-multilingual-v1',
  }),
  cancel: vi.fn().mockResolvedValue({ cancelled: true }),
  remove: vi.fn().mockResolvedValue({ removed: true }),
  subscribe: vi.fn().mockResolvedValue(() => {}),
};

afterEach(() => document.documentElement.removeAttribute('data-theme'));

describe('Settings', () => {
  it('updates shortcut, provider order, and deterministic theme accessibly', async () => {
    const settingsClient = client();
    const { container } = render(SettingsPanel, {
      client: settingsClient,
      modelClient,
    });
    const shortcut = await screen.findByRole('textbox', {
      name: 'Global shortcut',
    });
    expect(
      screen.getByRole('button', { name: 'Save settings' }),
    ).toBeDisabled();
    await fireEvent.input(shortcut, { target: { value: 'Control+Alt+L' } });
    expect(screen.getByText('Unsaved changes')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Save settings' })).toBeEnabled();
    await fireEvent.click(
      screen.getByRole('button', { name: 'Move Terminal earlier' }),
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Dark' }));
    expect(document.documentElement.dataset.theme).toBe('dark');
    await fireEvent.click(
      screen.getByRole('button', { name: 'Save settings' }),
    );

    await waitFor(() => expect(settingsClient.update).toHaveBeenCalledOnce());
    expect(settingsClient.update).toHaveBeenCalledWith(
      expect.objectContaining({
        globalShortcut: 'Control+Alt+L',
        providerTieBreakOrder: ['shell', 'vscode', 'foreground_window'],
        theme: 'dark',
        localSpeechEnabled: false,
      }),
    );
    expect(await screen.findByRole('status')).toHaveTextContent(
      'Settings saved',
    );
    expect((await axe.run(container)).violations).toEqual([]);
  });

  it('restores the last working settings after a shortcut conflict', async () => {
    const settingsClient = client({
      update: vi.fn().mockRejectedValue(
        new SettingsCommandError({
          code: 'SHORTCUT_CONFLICT',
          message: 'That shortcut is already in use',
          retryable: true,
          details: {},
        }),
      ),
    });
    render(SettingsPanel, { client: settingsClient, modelClient });
    const shortcut = await screen.findByRole('textbox', {
      name: 'Global shortcut',
    });
    await fireEvent.input(shortcut, { target: { value: 'Control+Alt+L' } });
    await fireEvent.click(
      screen.getByRole('button', { name: 'Save settings' }),
    );

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'That shortcut is already in use',
    );
    expect(shortcut).toHaveValue('Control+Shift+Space');
  });

  it('offers an explicit model install while leaving core capture independent', async () => {
    const settingsClient = client();
    render(SettingsPanel, { client: settingsClient, modelClient });
    expect(await screen.findByText('Model not installed')).toBeVisible();
    expect(screen.getByText('Multilingual base')).toBeVisible();
    expect(screen.getByText('Approximately 150 MB')).toBeVisible();
    expect(
      screen.getByText(
        'Generate searchable captions for voice captures entirely on this device.',
      ),
    ).toBeVisible();
    expect(screen.getByRole('button', { name: 'Install model' })).toBeEnabled();
    expect(
      screen.queryByRole('checkbox', { name: 'Automatic local transcription' }),
    ).not.toBeInTheDocument();
    await fireEvent.click(
      screen.getByRole('button', { name: 'Install model' }),
    );
    expect(settingsClient.update).not.toHaveBeenCalled();
  });

  it('exposes transcription as a saved preference only after installation', async () => {
    const installedModel: SpeechModelClient = {
      ...modelClient,
      status: vi.fn().mockResolvedValue({
        state: 'installed',
        modelId: 'whisper-base-multilingual-v1',
        label: 'Multilingual base',
        downloadedBytes: null,
        totalBytes: null,
        errorCode: null,
      }),
    };
    render(SettingsPanel, { client: client(), modelClient: installedModel });
    const toggle = await screen.findByRole('checkbox', {
      name: 'Automatic transcription',
    });
    await fireEvent.click(toggle);
    expect(screen.getByText('Unsaved changes')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Save settings' })).toBeEnabled();
  });
});
