import { describe, expect, it, vi } from 'vitest';

import type { AppSettings } from '../lib/ipc-types';
import {
  applyTheme,
  createSettingsClient,
  SettingsCommandError,
} from './settings-client';

const settings: AppSettings = {
  globalShortcut: 'Control+Shift+Space',
  providerTieBreakOrder: ['vscode', 'shell', 'foreground_window'],
  theme: 'system',
  localSpeechEnabled: false,
};

describe('settings client', () => {
  it('uses the strict get and patch command envelopes', async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, data: settings })
      .mockResolvedValueOnce({
        ok: true,
        data: { ...settings, theme: 'dark' },
      });
    const client = createSettingsClient(invoke);

    await client.get();
    await client.update({
      globalShortcut: null,
      providerTieBreakOrder: null,
      theme: 'dark',
      localSpeechEnabled: null,
    });

    expect(invoke).toHaveBeenNthCalledWith(1, 'get_settings', { input: {} });
    expect(invoke).toHaveBeenNthCalledWith(2, 'update_settings', {
      input: {
        patch: {
          globalShortcut: null,
          providerTieBreakOrder: null,
          theme: 'dark',
          localSpeechEnabled: null,
        },
      },
    });
  });

  it('preserves typed command errors', async () => {
    const invoke = vi.fn().mockResolvedValue({
      ok: false,
      error: {
        code: 'SHORTCUT_CONFLICT',
        message: 'That shortcut is already in use',
        retryable: true,
        details: {},
      },
    });

    await expect(createSettingsClient(invoke).get()).rejects.toBeInstanceOf(
      SettingsCommandError,
    );
  });

  it('applies explicit themes and restores system preference deterministically', () => {
    applyTheme('dark');
    expect(document.documentElement.dataset.theme).toBe('dark');

    applyTheme('light');
    expect(document.documentElement.dataset.theme).toBe('light');

    applyTheme('system');
    expect(document.documentElement).not.toHaveAttribute('data-theme');
  });
});
