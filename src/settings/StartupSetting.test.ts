import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import StartupSetting from './StartupSetting.svelte';

afterEach(cleanup);

it('reads OS registration and verifies both enable and disable', async () => {
  const call = vi
    .fn()
    .mockResolvedValueOnce(false)
    .mockResolvedValueOnce(undefined)
    .mockResolvedValueOnce(true)
    .mockResolvedValueOnce(undefined)
    .mockResolvedValueOnce(false);
  render(StartupSetting, { call });
  const control = screen.getByRole('switch');
  await waitFor(() => expect(control).toBeEnabled());
  await fireEvent.click(control);
  await waitFor(() => expect(control).toBeChecked());
  await waitFor(() => expect(control).toBeEnabled());
  await fireEvent.click(control);
  await waitFor(() => expect(control).not.toBeChecked());
  expect(call.mock.calls.map(([command]) => command)).toEqual([
    'plugin:autostart|is_enabled',
    'plugin:autostart|enable',
    'plugin:autostart|is_enabled',
    'plugin:autostart|disable',
    'plugin:autostart|is_enabled',
  ]);
});

it('keeps unavailable registration disabled and offers a read-only retry', async () => {
  const call = vi
    .fn()
    .mockRejectedValueOnce(new Error('unavailable'))
    .mockResolvedValueOnce(true);
  render(StartupSetting, { call });
  await screen.findByRole('alert');
  expect(screen.getByRole('switch')).toBeDisabled();
  await fireEvent.click(screen.getByRole('button', { name: 'Check again' }));
  await waitFor(() => expect(screen.getByRole('switch')).toBeChecked());
  expect(
    call.mock.calls.every(
      ([command]) => command === 'plugin:autostart|is_enabled',
    ),
  ).toBe(true);
});

it('reports a silent failed write instead of displaying the requested state', async () => {
  const call = vi
    .fn()
    .mockResolvedValueOnce(false)
    .mockResolvedValueOnce(undefined)
    .mockResolvedValueOnce(false);
  render(StartupSetting, { call });
  await waitFor(() => expect(screen.getByRole('switch')).toBeEnabled());
  await fireEvent.click(screen.getByRole('switch'));
  await screen.findByRole('alert');
  expect(screen.getByRole('switch')).not.toBeChecked();
});
