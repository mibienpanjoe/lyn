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
        localSpeechEnabled:
          patch.localSpeechEnabled ?? initial.localSpeechEnabled,
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
  it('presents the global shortcut as keycaps until editing is requested', async () => {
    render(SettingsPanel, { client: client(), modelClient });

    await screen.findByRole('heading', { name: 'Quick capture' });
    expect(screen.getByText('Ctrl', { selector: 'kbd' })).toBeVisible();
    expect(screen.getByText('Shift', { selector: 'kbd' })).toBeVisible();
    expect(screen.getByText('Space', { selector: 'kbd' })).toBeVisible();
    expect(
      screen.queryByRole('textbox', { name: 'Global shortcut' }),
    ).not.toBeInTheDocument();

    await fireEvent.click(
      screen.getByRole('button', { name: 'Change shortcut' }),
    );

    expect(
      screen.getByRole('textbox', { name: 'Global shortcut' }),
    ).toHaveFocus();
    expect(screen.getByRole('button', { name: 'Done' })).toBeVisible();
  });

  it('pairs each theme label with a distinct decorative icon', async () => {
    render(SettingsPanel, { client: client(), modelClient });
    await screen.findByRole('heading', { name: 'Appearance' });

    for (const name of ['System', 'Light', 'Dark']) {
      expect(
        screen.getByRole('button', { name }).querySelector('svg'),
      ).not.toBeNull();
    }
  });

  it('updates shortcut, provider order, and deterministic theme accessibly', async () => {
    const settingsClient = client();
    const { container } = render(SettingsPanel, {
      client: settingsClient,
      modelClient,
    });
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Change shortcut' }),
    );
    const shortcut = await screen.findByRole('textbox', {
      name: 'Global shortcut',
    });
    expect(
      screen.queryByRole('button', { name: 'Save settings' }),
    ).not.toBeInTheDocument();
    await fireEvent.input(shortcut, { target: { value: 'Control+Alt+L' } });
    expect(screen.getByText('Unsaved changes')).toBeVisible();
    expect(settingsClient.update).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByRole('button', { name: 'Done' }));
    await fireEvent.click(
      screen.getByRole('button', { name: 'Move Terminal earlier' }),
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Dark' }));
    expect(document.documentElement.dataset.theme).toBe('dark');

    await waitFor(() =>
      expect(settingsClient.update).toHaveBeenLastCalledWith(
        expect.objectContaining({
          globalShortcut: 'Control+Alt+L',
          providerTieBreakOrder: ['shell', 'vscode', 'foreground_window'],
          theme: 'dark',
          localSpeechEnabled: false,
        }),
      ),
    );
    expect(settingsClient.update).toHaveBeenLastCalledWith(
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

  it('serializes rapid changes and persists the newest settings snapshot', async () => {
    let resolveFirst!: (settings: AppSettings) => void;
    const update = vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise<AppSettings>((resolve) => {
            resolveFirst = resolve;
          }),
      )
      .mockImplementation((patch) =>
        Promise.resolve({ ...initial, theme: patch.theme ?? initial.theme }),
      );
    const settingsClient = client({ update });
    render(SettingsPanel, { client: settingsClient, modelClient });

    await fireEvent.click(await screen.findByRole('button', { name: 'Light' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Dark' }));
    expect(update).toHaveBeenCalledTimes(1);

    resolveFirst({ ...initial, theme: 'light' });

    await waitFor(() => expect(update).toHaveBeenCalledTimes(2));
    expect(update.mock.calls[1]?.[0]).toEqual(
      expect.objectContaining({ theme: 'dark' }),
    );
    expect(document.documentElement.dataset.theme).toBe('dark');
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
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Change shortcut' }),
    );
    const shortcut = await screen.findByRole('textbox', {
      name: 'Global shortcut',
    });
    await fireEvent.input(shortcut, { target: { value: 'Control+Alt+L' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Done' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'That shortcut is already in use',
    );
    expect(screen.getByText('Ctrl', { selector: 'kbd' })).toBeVisible();
    expect(screen.getByText('Shift', { selector: 'kbd' })).toBeVisible();
    expect(screen.getByText('Space', { selector: 'kbd' })).toBeVisible();
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

  it('shows a safe retry state when model installation fails', async () => {
    const failedModel: SpeechModelClient = {
      ...modelClient,
      status: vi.fn().mockResolvedValue({
        state: 'not_installed',
        modelId: 'whisper-base-multilingual-v1',
        label: 'Multilingual base',
        downloadedBytes: null,
        totalBytes: null,
        errorCode: 'MODEL_DOWNLOAD_FAILED',
      }),
    };

    render(SettingsPanel, { client: client(), modelClient: failedModel });

    expect(await screen.findByText('Installation failed')).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Retry installation' }),
    ).toBeEnabled();
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
    const settingsClient = client();
    render(SettingsPanel, {
      client: settingsClient,
      modelClient: installedModel,
    });
    const toggle = await screen.findByRole('checkbox', {
      name: 'Automatic transcription',
    });
    await fireEvent.click(toggle);
    await waitFor(() =>
      expect(settingsClient.update).toHaveBeenCalledWith(
        expect.objectContaining({ localSpeechEnabled: true }),
      ),
    );
    expect(await screen.findByRole('status')).toHaveTextContent(
      'Settings saved',
    );
  });
});
