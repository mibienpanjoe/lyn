import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import axe from 'axe-core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { AppSettings } from '../lib/ipc-types';
import SettingsPanel from './SettingsPanel.svelte';
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

afterEach(() => document.documentElement.removeAttribute('data-theme'));

describe('Settings', () => {
  it('updates shortcut, provider order, and deterministic theme accessibly', async () => {
    const settingsClient = client();
    const { container } = render(SettingsPanel, { client: settingsClient });
    const shortcut = await screen.findByRole('textbox', {
      name: 'Global shortcut',
    });
    await fireEvent.input(shortcut, { target: { value: 'Control+Alt+L' } });
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
    render(SettingsPanel, { client: settingsClient });
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

  it('labels local speech unavailable without disabling core capture', async () => {
    render(SettingsPanel, { client: client() });
    expect(
      await screen.findByText('Unavailable in this build.', { exact: false }),
    ).toBeVisible();
    expect(
      screen.getByRole('checkbox', { name: 'Automatic local transcription' }),
    ).toBeDisabled();
  });
});
