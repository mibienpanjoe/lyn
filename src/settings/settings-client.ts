import { invoke } from '@tauri-apps/api/core';

import type {
  AppError,
  AppSettings,
  CommandResult,
  SettingsPatch,
} from '../lib/ipc-types';

type Invoke = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

export interface SettingsClient {
  get(): Promise<AppSettings>;
  update(patch: SettingsPatch): Promise<AppSettings>;
}

export class SettingsCommandError extends Error {
  constructor(readonly appError: AppError) {
    super(appError.message);
    this.name = 'SettingsCommandError';
  }
}

async function command<T>(call: Invoke, name: string, input: unknown) {
  const result = await call<CommandResult<T>>(name, { input });
  if (!result.ok) throw new SettingsCommandError(result.error);
  return result.data;
}

export function createSettingsClient(call: Invoke = invoke): SettingsClient {
  return {
    get: () => command<AppSettings>(call, 'get_settings', {}),
    update: (patch) => command<AppSettings>(call, 'update_settings', { patch }),
  };
}

export const settingsClient = createSettingsClient();

export function applyTheme(theme: AppSettings['theme']) {
  if (theme === 'system')
    document.documentElement.removeAttribute('data-theme');
  else document.documentElement.dataset.theme = theme;
}
