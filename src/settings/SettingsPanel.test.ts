import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/svelte';
import axe from 'axe-core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type {
  AppSettings,
  IntegrationStatus,
  GnomeActivationPhase,
  GnomeManagementAction,
  ManageGnomeActivationResult,
} from '../lib/ipc-types';
import SettingsPanel from './SettingsPanel.svelte';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => false) }));
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

function nativeStatus(
  phase: GnomeActivationPhase,
  revision: string | null,
  actions: GnomeManagementAction[],
): IntegrationStatus {
  return {
    id: 'gnome',
    name: 'GNOME Terminal',
    description: 'Native context',
    detected: true,
    installed: false,
    details: null,
    native: {
      files:
        phase === 'configured'
          ? 'owned'
          : phase === 'absent'
            ? 'absent'
            : 'pending',
      connection: 'disconnected',
      canInstall: false,
      activation: { phase, revision, actions },
    },
  };
}

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
    label: 'Parakeet v3 · Q8',
    downloadedBytes: null,
    totalBytes: null,
    errorCode: null,
  }),
  install: vi.fn().mockResolvedValue({
    accepted: true,
    modelId: 'parakeet-tdt-0.6b-v3-q8-v1',
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
    expect(screen.getByText(/Alt\+L/)).toBeVisible();
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
    expect(screen.getByText('Parakeet v3 · Q8')).toBeVisible();
    expect(screen.getByText('Approximately 942 MB')).toBeVisible();
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
        modelId: 'parakeet-tdt-0.6b-v3-q8-v1',
        label: 'Parakeet v3 · Q8',
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
        modelId: 'parakeet-tdt-0.6b-v3-q8-v1',
        label: 'Parakeet v3 · Q8',
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
          details:
            'Extension files installed. Live connection and capture are not verified; reload VS Code, then invoke Lyn from an editor.',
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

  it('distinguishes GNOME connection from activation files without offering unavailable installation', async () => {
    const gnome: IntegrationStatus = {
      id: 'gnome',
      name: 'GNOME Terminal',
      description: 'Native terminal context',
      detected: true,
      installed: false,
      details: null,
      native: { files: 'owned', connection: 'connected', canInstall: false },
    };
    const intClient: IntegrationClient = {
      list: vi
        .fn()
        .mockResolvedValueOnce([gnome])
        .mockResolvedValueOnce([
          {
            ...gnome,
            native: { files: 'pending', connection: 'busy', canInstall: false },
          },
        ]),
      install: vi.fn(),
    };
    const { container } = render(SettingsPanel, {
      client: client(),
      modelClient,
      intClient,
    });
    await screen.findByText('GNOME Terminal');
    expect(screen.getByText('Connected')).toBeVisible();
    expect(screen.getByText(/Activation files verified\./)).toBeVisible();
    expect(
      screen.getByText(/Installation is not available in this build\./),
    ).toBeVisible();
    expect(screen.queryByText('Installed', { exact: true })).toBeNull();
    const gnomeCard = screen
      .getByText('GNOME Terminal')
      .closest('.integration-card') as HTMLElement;
    expect(within(gnomeCard).queryByRole('button')).toBeNull();
    await fireEvent.click(
      screen.getByRole('button', { name: 'Refresh integration statuses' }),
    );
    expect(await screen.findByText('Connection busy')).toBeVisible();
    expect(screen.getByText(/Installation needs attention\./)).toBeVisible();
    expect(intClient.install).not.toHaveBeenCalled();
    expect((await axe.run(container)).violations).toEqual([]);
  });

  it('refreshes after a failed native first installation and retires its old offer', async () => {
    const offered = nativeStatus('absent', null, []);
    offered.native!.canInstall = true;
    const list = vi
      .fn()
      .mockResolvedValueOnce([offered])
      .mockRejectedValueOnce(new Error('read failed'));
    const install = vi
      .fn()
      .mockRejectedValue(new Error('raw private installation path'));
    render(SettingsPanel, {
      client: client(),
      modelClient,
      intClient: { list, install },
    });
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Install' }),
    );
    await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
    expect(install).toHaveBeenCalledExactlyOnceWith({ id: 'gnome' });
    expect(screen.queryByText(/raw private/)).toBeNull();
    expect(screen.queryByRole('button', { name: 'Install' })).toBeNull();
  });

  it.each(['a', 'b'])(
    'accepts first preparation only when its locked receipt matches refresh (%s)',
    async (revision) => {
      const offered = nativeStatus('absent', null, []);
      offered.native!.canInstall = true;
      const install = vi.fn().mockResolvedValue({
        id: 'gnome',
        success: true,
        installed: false,
        message: 'raw backend message',
        configurationRevision: 'a'.repeat(64),
      });
      const list = vi
        .fn()
        .mockResolvedValueOnce([offered])
        .mockResolvedValueOnce([
          nativeStatus('configured', revision.repeat(64), [
            'synchronize',
            'uninstall',
          ]),
        ]);
      render(SettingsPanel, {
        client: client(),
        modelClient,
        intClient: { list, install, manage: vi.fn() },
      });
      await fireEvent.click(
        await screen.findByRole('button', { name: 'Install' }),
      );
      await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
      const receipt =
        'Installation prepared. Apply the configuration before the next GNOME Terminal start.';
      if (revision === 'a')
        expect(await screen.findByText(receipt)).toBeVisible();
      else await waitFor(() => expect(screen.queryByText(receipt)).toBeNull());
      expect(screen.queryByText('raw backend message')).toBeNull();
      expect(install).toHaveBeenCalledExactlyOnceWith({ id: 'gnome' });
      expect(
        await screen.findByRole('button', {
          name: 'Apply configuration',
        }),
      ).toBeVisible();
    },
  );

  it('uses one captured revision, blocks duplicate actions and refreshes after completion', async () => {
    let finish!: (result: ManageGnomeActivationResult) => void;
    const manage = vi.fn(
      () =>
        new Promise<ManageGnomeActivationResult>((resolve) => {
          finish = resolve;
        }),
    );
    const list = vi
      .fn()
      .mockResolvedValueOnce([
        nativeStatus('restoring_update', 'a'.repeat(64), ['restore_update']),
      ])
      .mockResolvedValueOnce([
        nativeStatus('configured', 'b'.repeat(64), ['uninstall']),
      ]);
    const intClient: IntegrationClient = { list, manage, install: vi.fn() };
    render(SettingsPanel, { client: client(), modelClient, intClient });
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Restore configuration' }),
    );
    expect(manage).toHaveBeenCalledWith({
      action: 'restore_update',
      expectedRevision: 'a'.repeat(64),
    });
    const busy = screen.getByRole('button', { name: 'Working…' });
    expect(busy).toBeDisabled();
    await fireEvent.click(busy);
    expect(manage).toHaveBeenCalledTimes(1);
    finish({
      action: 'restore_update',
      activation: {
        phase: 'configured',
        revision: 'b'.repeat(64),
        actions: ['uninstall'],
      },
    });
    expect(
      await screen.findByText(/Previous configuration restored/),
    ).toBeVisible();
    expect(
      await screen.findByRole('button', { name: 'Remove integration' }),
    ).toBeEnabled();
    expect(list).toHaveBeenCalledTimes(2);
    expect(intClient.install).not.toHaveBeenCalled();
  });

  it('removes an integration with one compound request and no client-side continuation', async () => {
    const manage = vi.fn().mockResolvedValue({
      action: 'uninstall',
      activation: { phase: 'absent', revision: null },
    });
    const list = vi
      .fn()
      .mockResolvedValueOnce([
        nativeStatus('configured', 'a'.repeat(64), ['uninstall']),
      ])
      .mockResolvedValueOnce([nativeStatus('absent', null, [])]);
    const install = vi.fn();
    render(SettingsPanel, {
      client: client(),
      modelClient,
      intClient: { list, manage, install },
    });
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Remove integration' }),
    );
    expect(await screen.findByText(/Integration removed/)).toBeVisible();
    expect(manage).toHaveBeenCalledTimes(1);
    expect(manage).toHaveBeenCalledWith({
      action: 'uninstall',
      expectedRevision: 'a'.repeat(64),
    });
    expect(install).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: 'Finish removal' })).toBeNull();
  });

  it('retires stale offers even when refresh fails and never replays the mutation', async () => {
    const stale = new IntegrationCommandError({
      code: 'PERMISSION_DENIED',
      message: 'Do not display backend text',
      retryable: false,
      details: { state: 'stale_revision' },
    });
    const manage = vi
      .fn()
      .mockRejectedValueOnce(stale)
      .mockResolvedValueOnce({
        action: 'restore_update',
        activation: { phase: 'configured', revision: 'c'.repeat(64) },
      });
    const list = vi
      .fn()
      .mockResolvedValueOnce([
        nativeStatus('restoring_update', 'a'.repeat(64), ['restore_update']),
      ])
      .mockRejectedValueOnce(new Error('read failed'))
      .mockResolvedValueOnce([
        nativeStatus('restoring_update', 'b'.repeat(64), ['restore_update']),
      ])
      .mockResolvedValueOnce([nativeStatus('configured', 'c'.repeat(64), [])]);
    render(SettingsPanel, {
      client: client(),
      modelClient,
      intClient: { list, manage, install: vi.fn() },
    });
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Restore configuration' }),
    );
    await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
    expect(
      screen.queryByRole('button', { name: 'Restore configuration' }),
    ).toBeNull();
    expect(manage).toHaveBeenCalledTimes(1);
    expect(screen.queryByText('Do not display backend text')).toBeNull();
    await fireEvent.click(
      screen.getByRole('button', { name: 'Refresh integration statuses' }),
    );
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Restore configuration' }),
    );
    expect(manage).toHaveBeenNthCalledWith(2, {
      action: 'restore_update',
      expectedRevision: 'b'.repeat(64),
    });
    expect(
      await screen.findByText(/Previous configuration restored/),
    ).toBeVisible();
  });

  it('does not claim success when the post-action observation changed', async () => {
    const manage = vi.fn().mockResolvedValue({
      action: 'restore_update',
      activation: { phase: 'changed', revision: null },
    });
    const list = vi
      .fn()
      .mockResolvedValueOnce([
        nativeStatus('restoring_update', 'a'.repeat(64), ['restore_update']),
      ])
      .mockResolvedValueOnce([nativeStatus('changed', null, [])]);
    render(SettingsPanel, {
      client: client(),
      modelClient,
      intClient: { list, manage, install: vi.fn() },
    });
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Restore configuration' }),
    );
    expect(await screen.findByText(/configuration changed/)).toBeVisible();
    expect(screen.queryByText(/Previous configuration restored/)).toBeNull();
    expect(manage).toHaveBeenCalledTimes(1);
  });

  it.each([
    nativeStatus('changed', null, []),
    nativeStatus('configured', 'c'.repeat(64), ['uninstall']),
  ])(
    'retires a successful receipt when refresh observes another authority',
    async (after) => {
      const manage = vi.fn().mockResolvedValue({
        action: 'restore_update',
        activation: { phase: 'configured', revision: 'b'.repeat(64) },
      });
      const list = vi
        .fn()
        .mockResolvedValueOnce([
          nativeStatus('restoring_update', 'a'.repeat(64), ['restore_update']),
        ])
        .mockResolvedValueOnce([after]);
      render(SettingsPanel, {
        client: client(),
        modelClient,
        intClient: { list, manage, install: vi.fn() },
      });
      await fireEvent.click(
        await screen.findByRole('button', { name: 'Restore configuration' }),
      );
      expect(await screen.findByText(/configuration changed/)).toBeVisible();
      expect(screen.queryByText(/Previous configuration restored/)).toBeNull();
      expect(manage).toHaveBeenCalledTimes(1);
    },
  );

  it('localizes a failed post-action refresh in French instead of displaying backend text', async () => {
    const manage = vi.fn().mockResolvedValue({
      action: 'restore_update',
      activation: { phase: 'configured', revision: 'b'.repeat(64) },
    });
    const list = vi
      .fn()
      .mockResolvedValueOnce([
        nativeStatus('restoring_update', 'a'.repeat(64), ['restore_update']),
      ])
      .mockRejectedValueOnce(
        new IntegrationCommandError({
          code: 'INTERNAL_ERROR',
          message: 'backend-only refresh failure',
          retryable: false,
          details: {},
        }),
      );
    render(SettingsPanel, {
      client: client({
        get: vi.fn().mockResolvedValue({ ...initial, language: 'french' }),
      }),
      modelClient,
      intClient: { list, manage, install: vi.fn() },
    });
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Restaurer la configuration' }),
    );
    expect(
      await screen.findByText(/L’état n’a pas pu être actualisé/),
    ).toBeVisible();
    expect(screen.queryByText('backend-only refresh failure')).toBeNull();
    expect(manage).toHaveBeenCalledTimes(1);
  });

  it('localizes native management errors in French and hides unsupported or invalid offers', async () => {
    const manage = vi.fn().mockRejectedValue(
      new IntegrationCommandError({
        code: 'PERMISSION_DENIED',
        message: 'backend-only text',
        retryable: false,
        details: { state: 'stale_revision' },
      }),
    );
    const list = vi
      .fn()
      .mockResolvedValueOnce([
        nativeStatus('restoring_update', 'a'.repeat(64), ['restore_update']),
      ])
      .mockResolvedValueOnce([
        nativeStatus('configured', 'A'.repeat(64), ['uninstall', 'remove']),
      ]);
    const { container } = render(SettingsPanel, {
      client: client({
        get: vi.fn().mockResolvedValue({ ...initial, language: 'french' }),
      }),
      modelClient,
      intClient: { list, manage, install: vi.fn() },
    });
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Restaurer la configuration' }),
    );
    expect(await screen.findByText(/La configuration a changé/)).toBeVisible();
    expect(screen.queryByText('backend-only text')).toBeNull();
    const card = screen
      .getByText('GNOME Terminal')
      .closest('.integration-card') as HTMLElement;
    expect(within(card).queryByRole('button')).toBeNull();
    expect((await axe.run(container)).violations).toEqual([]);
  });

  it('shows unknown GNOME evidence safely in French without an installation action', async () => {
    const intClient: IntegrationClient = {
      list: vi.fn().mockResolvedValue([
        {
          id: 'gnome',
          name: 'GNOME Terminal',
          description: 'Native terminal context',
          detected: true,
          installed: false,
          details: null,
        },
      ]),
      install: vi.fn(),
    };
    render(SettingsPanel, {
      client: client({
        get: vi.fn().mockResolvedValue({ ...initial, language: 'french' }),
      }),
      modelClient,
      intClient,
    });
    await screen.findByText('Connexion indisponible');
    expect(
      screen.getByText(/État de l’installation indisponible\./),
    ).toBeVisible();
    expect(
      screen.getByText(
        /L’installation n’est pas disponible dans cette version\./,
      ),
    ).toBeVisible();
    const card = screen
      .getByText('GNOME Terminal')
      .closest('.integration-card') as HTMLElement;
    expect(within(card).queryByRole('button')).toBeNull();
  });

  it.each([
    {
      language: 'english' as const,
      label: 'Previous configuration needs to be restored.',
    },
    {
      language: 'french' as const,
      label: 'Configuration précédente à restaurer.',
    },
  ])(
    'shows the precise GNOME recovery phase in $language without exposing its revision',
    async ({ language, label }) => {
      const revision = 'a'.repeat(64);
      const intClient: IntegrationClient = {
        list: vi.fn().mockResolvedValue([
          {
            id: 'gnome',
            name: 'GNOME Terminal',
            description: 'Native context',
            detected: true,
            installed: false,
            details: null,
            native: {
              files: 'pending',
              connection: 'disconnected',
              canInstall: false,
              activation: { phase: 'restoring_update', revision },
            },
          },
        ]),
        install: vi.fn(),
      };
      render(SettingsPanel, {
        client: client({
          get: vi.fn().mockResolvedValue({ ...initial, language }),
        }),
        modelClient,
        intClient,
      });
      expect(
        await screen.findByText(new RegExp(label.replace('.', '\\.'))),
      ).toBeVisible();
      const card = screen
        .getByText('GNOME Terminal')
        .closest('.integration-card') as HTMLElement;
      expect(card.textContent).not.toContain(revision);
      expect(within(card).queryByRole('button')).toBeNull();
      expect(intClient.install).not.toHaveBeenCalled();
    },
  );

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
