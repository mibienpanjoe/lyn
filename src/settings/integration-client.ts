import { invoke } from '@tauri-apps/api/core';

import type {
  AppError,
  CommandResult,
  InstallIntegrationInput,
  InstallIntegrationResult,
  IntegrationStatus,
  ManageGnomeActivationInput,
  ManageGnomeActivationResult,
} from '../lib/ipc-types';

type Invoke = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

export interface IntegrationClient {
  list(): Promise<IntegrationStatus[]>;
  install(input: InstallIntegrationInput): Promise<InstallIntegrationResult>;
  manage?(
    input: ManageGnomeActivationInput,
  ): Promise<ManageGnomeActivationResult>;
}

export class IntegrationCommandError extends Error {
  constructor(readonly appError: AppError) {
    super(appError.message);
    this.name = 'IntegrationCommandError';
  }
}

async function command<T>(call: Invoke, name: string, input: unknown) {
  const result = await call<CommandResult<T>>(name, { input });
  if (!result.ok) throw new IntegrationCommandError(result.error);
  return result.data;
}

export function createIntegrationClient(
  call: Invoke = invoke,
): IntegrationClient {
  return {
    list: () =>
      command<IntegrationStatus[]>(call, 'get_integration_statuses', {}),
    install: (input) =>
      command<InstallIntegrationResult>(call, 'install_integration', input),
    manage: (input) =>
      command<ManageGnomeActivationResult>(
        call,
        'manage_gnome_activation',
        input,
      ),
  };
}

export const integrationClient = createIntegrationClient();
