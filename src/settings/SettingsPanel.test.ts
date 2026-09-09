import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import axe from 'axe-core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { AppSettings, IntegrationStatus } from '../lib/ipc-types';
import SettingsPanel from './SettingsPanel.svelte';
import type { SpeechModelClient } from './model-client';
import { SettingsCommandError, type SettingsClient } from './settings-client';
import {
  type IntegrationClient,
  IntegrationCommandError,
} from './integration-client';

const initial: AppSettings = {
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
        language: patch.language ?? initial.language,
      }),
    ),
    version: vi.fn().mockResolvedValue({ version: '0.6.4' }),
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

const defaultIntClient: IntegrationClient = {
  list: vi.fn().mockResolvedValue([]),
  install: vi.fn().mockResolvedValue({
    id: 'cursor',
    success: true,
    message: 'Extension installed',
    installed: true,
  }),
};

afterEach(() => document.documentElement.removeAttribute('data-theme'));

describe('Settings', () => {
  it('presents the global shortcut as keycaps until editing is requested', async () => {
    render(SettingsPanel, {
      client: client(),
      modelClient,
      intClient: defaultIntClient,
    });

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
    render(SettingsPanel, {
      client: client(),
      modelClient,
      intClient: defaultIntClient,
    });
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
      intClient: defaultIntClient,
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
          providerTieBreakOrder: [
            'vscode',
            'cursor',
            'shell',
            'browser',
            'foreground_window',
          ],
          theme: 'dark',
          localSpeechEnabled: false,
        }),
      ),
    );
    expect(settingsClient.update).toHaveBeenLastCalledWith(
      expect.objectContaining({
        globalShortcut: 'Control+Alt+L',
        providerTieBreakOrder: [
          'vscode',
          'cursor',
          'shell',
          'browser',
          'foreground_window',
        ],
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
    render(SettingsPanel, {
      client: settingsClient,
      modelClient,
      intClient: defaultIntClient,
    });

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
    render(SettingsPanel, {
      client: settingsClient,
      modelClient,
      intClient: defaultIntClient,
    });
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
    render(SettingsPanel, {
      client: settingsClient,
      modelClient,
      intClient: defaultIntClient,
    });
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

    render(SettingsPanel, {
      client: client(),
      modelClient: failedModel,
      intClient: defaultIntClient,
    });

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
      intClient: defaultIntClient,
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

  it('displays integrations with statuses and allows 1-click installation', async () => {
    const installMock = vi.fn().mockResolvedValue({
      id: 'cursor',
      success: true,
      message: 'Extension installed successfully!',
      installed: true,
    });
    const intClient: IntegrationClient = {
      list: vi.fn().mockResolvedValue([
        {
          id: 'cursor',
          name: 'Cursor IDE',
          description:
            'Reports the focused Cursor workspace folder to Lyn on capture.',
          detected: true,
          installed: false,
          details: 'Cursor detected on this system',
        },
        {
          id: 'vscode',
          name: 'Visual Studio Code',
          description:
            'Reports the focused VS Code workspace folder to Lyn on capture.',
          detected: true,
          installed: true,
          details: 'Extension active in ~/.vscode/extensions/',
        },
      ]),
      install: installMock,
    };

    render(SettingsPanel, {
      client: client(),
      modelClient,
      intClient,
    });

    await screen.findByRole('heading', {
      name: 'Integrations & Context Providers',
    });
    expect(screen.getByText('Cursor IDE')).toBeVisible();
    expect(screen.getByText('Visual Studio Code')).toBeVisible();

    const installButtons = screen.getAllByRole('button', {
      name: 'Install Extension',
    });
    expect(installButtons.length).toBeGreaterThan(0);

    await fireEvent.click(installButtons[0]);
    expect(installMock).toHaveBeenCalledWith({ id: 'cursor' });
    expect(
      await screen.findByText('Extension installed successfully!'),
    ).toBeVisible();
  });

  it('handles overlapping list requests and discards out-of-order stale responses', async () => {
    let resolveFirst!: (value: IntegrationStatus[]) => void;
    const firstPromise = new Promise<IntegrationStatus[]>((resolve) => {
      resolveFirst = resolve;
    });

    let resolveSecond!: (value: IntegrationStatus[]) => void;
    const secondPromise = new Promise<IntegrationStatus[]>((resolve) => {
      resolveSecond = resolve;
    });

    let callCount = 0;
    const listMock = vi.fn().mockImplementation(() => {
      callCount++;
      return callCount === 1 ? firstPromise : secondPromise;
    });

    const intClient: IntegrationClient = {
      list: listMock,
      install: vi.fn(),
    };

    render(SettingsPanel, {
      client: client(),
      modelClient,
      intClient,
      isLinux: true,
    });

    // Request 1 initiated on mount
    expect(listMock).toHaveBeenCalledTimes(1);

    // Trigger Request 2 via refresh button
    const refreshBtn = await screen.findByRole('button', {
      name: 'Refresh integration statuses',
    });
    await fireEvent.click(refreshBtn);
    expect(listMock).toHaveBeenCalledTimes(2);

    // Resolve second request first (out-of-order)
    const secondData: IntegrationStatus[] = [
      {
        id: 'browser',
        name: 'Fresh Browser Integration',
        description: 'Latest status',
        detected: true,
        installed: true,
        details: 'Active',
      },
    ];
    resolveSecond(secondData);

    expect(await screen.findByText('Fresh Browser Integration')).toBeVisible();

    // Now resolve first request later with stale data
    const firstData: IntegrationStatus[] = [
      {
        id: 'browser',
        name: 'Stale Browser Integration',
        description: 'Old status',
        detected: true,
        installed: false,
        details: 'Outdated',
      },
    ];
    resolveFirst(firstData);

    // Wait microtasks/timers
    await new Promise((r) => setTimeout(r, 25));

    // Stale data must NOT overwrite fresh data
    expect(screen.queryByText('Stale Browser Integration')).toBeNull();
    expect(screen.getByText('Fresh Browser Integration')).toBeVisible();
  });

  it('gates integrations section to Linux and prevents non-Linux builds from rendering it', async () => {
    const listMock = vi.fn().mockResolvedValue([]);
    const intClient: IntegrationClient = {
      list: listMock,
      install: vi.fn(),
    };

    render(SettingsPanel, {
      client: client(),
      modelClient,
      intClient,
      isLinux: false,
    });

    await screen.findByRole('heading', { name: 'Appearance' });
    expect(
      screen.queryByRole('heading', {
        name: 'Integrations & Context Providers',
      }),
    ).toBeNull();
    expect(listMock).not.toHaveBeenCalled();
  });

  it('renders an alert with a retry action when integration discovery fails and recovers on retry', async () => {
    const mockSuccessData: IntegrationStatus[] = [
      {
        id: 'cursor',
        name: 'Cursor IDE',
        description: 'Reports focused Cursor workspace.',
        detected: true,
        installed: false,
        details: 'Cursor detected',
      },
    ];

    const listMock = vi
      .fn()
      .mockRejectedValueOnce(new Error('Discovery service offline'))
      .mockResolvedValueOnce(mockSuccessData);

    const intClient: IntegrationClient = {
      list: listMock,
      install: vi.fn(),
    };

    render(SettingsPanel, {
      client: client(),
      modelClient,
      intClient,
      isLinux: true,
    });

    await screen.findByRole('heading', {
      name: 'Integrations & Context Providers',
    });

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Integration discovery failed.');
    expect(screen.queryByText('Cursor IDE')).toBeNull();

    const retryBtn = screen.getByRole('button', { name: 'Retry' });
    expect(retryBtn).toBeVisible();

    await fireEvent.click(retryBtn);
    expect(listMock).toHaveBeenCalledTimes(2);

    expect(await screen.findByText('Cursor IDE')).toBeVisible();
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('preserves typed IntegrationCommandError message when discovery fails', async () => {
    const listMock = vi.fn().mockRejectedValueOnce(
      new IntegrationCommandError({
        code: 'VALIDATION_ERROR',
        message: 'Custom backend validation error',
        retryable: false,
        details: {},
      }),
    );

    const intClient: IntegrationClient = {
      list: listMock,
      install: vi.fn(),
    };

    render(SettingsPanel, {
      client: client(),
      modelClient,
      intClient,
      isLinux: true,
    });

    await screen.findByRole('heading', {
      name: 'Integrations & Context Providers',
    });

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Custom backend validation error');
    expect(screen.getByRole('button', { name: 'Retry' })).toBeVisible();
  });

  it('allows toggling French mode in settings and adapts UI text while keeping English default', async () => {
    const updateMock = vi.fn().mockImplementation((patch) =>
      Promise.resolve({
        ...initial,
        ...patch,
      }),
    );
    const mockClient = client({ update: updateMock });

    render(SettingsPanel, {
      client: mockClient,
      modelClient,
      intClient: defaultIntClient,
      isLinux: true,
    });

    // Default is English
    expect(
      await screen.findByRole('heading', { name: 'Settings' }),
    ).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Appearance' })).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Language' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'English' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );

    // Switch to French
    const frenchBtn = screen.getByRole('button', { name: 'Français' });
    await fireEvent.click(frenchBtn);

    // Verify UI translated to French
    expect(
      await screen.findByRole('heading', { name: 'Paramètres' }),
    ).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Apparence' })).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Langue' })).toBeVisible();
    expect(frenchBtn).toHaveAttribute('aria-pressed', 'true');

    expect(updateMock).toHaveBeenCalledWith(
      expect.objectContaining({ language: 'french' }),
    );

    // Switch back to English
    const englishBtn = screen.getByRole('button', { name: 'English' });
    await fireEvent.click(englishBtn);
    expect(
      await screen.findByRole('heading', { name: 'Settings' }),
    ).toBeVisible();
  });

  it('renders platform icon wraps and parses titles with subtitle pills cleanly', async () => {
    const mockStatuses: IntegrationStatus[] = [
      {
        id: 'browser',
        name: 'Web Browser (Chrome, Brave, Edge, Firefox)',
        description: 'Correlates active localhost tabs.',
        detected: true,
        installed: false,
        details: 'Supported web browser detected',
      },
      {
        id: 'cursor',
        name: 'Cursor IDE',
        description: 'Reports focused Cursor workspace.',
        detected: true,
        installed: true,
        details: 'Extension active in ~/.cursor/extensions/',
      },
    ];

    const intClient: IntegrationClient = {
      list: vi.fn().mockResolvedValue(mockStatuses),
      install: vi.fn(),
    };

    const { container } = render(SettingsPanel, {
      client: client(),
      modelClient,
      intClient,
      isLinux: true,
    });

    await screen.findByRole('heading', {
      name: 'Integrations & Context Providers',
    });

    // Verify parsed title and subtitle pill
    expect(await screen.findByText('Web Browser')).toBeVisible();
    expect(screen.getByText('Chrome · Brave · Edge · Firefox')).toBeVisible();

    // Verify platform icon wraps are present
    expect(
      container.querySelector('[data-platform="browser"]'),
    ).toBeInTheDocument();
    expect(
      container.querySelector('[data-platform="cursor"]'),
    ).toBeInTheDocument();

    // Verify path pills in details
    expect(container.querySelector('.path-pill')).toHaveTextContent(
      '~/.cursor/extensions/',
    );
  });

  it('shows the installed app version', async () => {
    render(SettingsPanel, {
      client: client(),
      modelClient,
      intClient: defaultIntClient,
    });

    expect(await screen.findByRole('heading', { name: 'About' })).toBeVisible();
    expect(await screen.findByText('0.6.4')).toBeVisible();
  });
});
