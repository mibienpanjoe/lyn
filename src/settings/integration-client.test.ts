import { describe, expect, it, vi } from 'vitest';

import type {
  InstallIntegrationResult,
  IntegrationStatus,
} from '../lib/ipc-types';
import {
  createIntegrationClient,
  IntegrationCommandError,
} from './integration-client';

const mockStatuses: IntegrationStatus[] = [
  {
    id: 'cursor',
    name: 'Cursor IDE',
    description: 'Reports focused Cursor workspace.',
    detected: true,
    installed: false,
    details: 'Detected',
  },
];

const mockInstallResult: InstallIntegrationResult = {
  id: 'cursor',
  success: true,
  message: 'Installed successfully',
  installed: true,
};

describe('integration client', () => {
  it('calls get_integration_statuses and install_integration correctly', async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, data: mockStatuses })
      .mockResolvedValueOnce({ ok: true, data: mockInstallResult });

    const client = createIntegrationClient(invoke);

    const statuses = await client.list();
    expect(statuses).toEqual(mockStatuses);
    expect(invoke).toHaveBeenNthCalledWith(1, 'get_integration_statuses', {
      input: {},
    });

    const result = await client.install({ id: 'cursor' });
    expect(result).toEqual(mockInstallResult);
    expect(invoke).toHaveBeenNthCalledWith(2, 'install_integration', {
      input: { id: 'cursor' },
    });
  });

  it('preserves typed command errors', async () => {
    const invoke = vi.fn().mockResolvedValue({
      ok: false,
      error: {
        code: 'VALIDATION_ERROR',
        message: 'Invalid integration request',
        retryable: false,
        details: {},
      },
    });

    const client = createIntegrationClient(invoke);
    await expect(client.install({ id: 'cursor' })).rejects.toBeInstanceOf(
      IntegrationCommandError,
    );
  });
});
