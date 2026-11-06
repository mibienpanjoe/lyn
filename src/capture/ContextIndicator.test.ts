import { render, screen } from '@testing-library/svelte';
import axe from 'axe-core';
import { describe, expect, it, vi } from 'vitest';

import ContextIndicator from './ContextIndicator.svelte';
import type { ContextResolution } from '../lib/ipc-types';

const resolved: ContextResolution = {
  state: 'resolved',
  candidate: {
    context: { id: 'project-alpha', kind: 'project', name: 'alpha' },
    branchName: 'gnome-alpha',
    provider: 'vscode',
    requiresConfirmation: false,
  },
  selection: null,
};

describe('context indicator', () => {
  it('shows project, branch and provider chips while keeping the stable accessible name', async () => {
    const { container } = render(ContextIndicator, {
      resolution: resolved,
      open: false,
      onclick: vi.fn(),
    });

    // Desktop fixtures select this exact accessible name.
    const button = screen.getByRole('button', {
      name: 'Context alpha, branch gnome-alpha. Change context',
    });
    expect(button).not.toHaveClass('context-attention');
    expect(container.querySelector('.context-chip')).toHaveTextContent('alpha');
    expect(container.querySelector('.context-branch')).toHaveTextContent(
      'gnome-alpha',
    );
    expect(container.querySelector('.context-provider')).toHaveTextContent(
      'VS Code',
    );
    expect(container.textContent).not.toContain(', branch ');
    expect((await axe.run(container)).violations).toEqual([]);
  });

  it('omits the provider label for generic foreground-window resolution', () => {
    const { container } = render(ContextIndicator, {
      resolution: {
        ...resolved,
        candidate: {
          ...resolved.candidate!,
          branchName: null,
          provider: 'foreground_window',
        },
      } as ContextResolution,
      open: false,
      onclick: vi.fn(),
    });

    expect(
      screen.getByRole('button', { name: 'Context alpha. Change context' }),
    ).toBeVisible();
    expect(container.querySelector('.context-branch')).toBeNull();
    expect(container.querySelector('.context-provider')).toBeNull();
  });

  it('keeps the stale prompt name and shows a short visible prompt', () => {
    render(ContextIndicator, {
      resolution: resolved,
      open: false,
      stale: true,
      onclick: vi.fn(),
    });

    const button = screen.getByRole('button', {
      name: 'Context source stale. Choose another context',
    });
    expect(button).toHaveClass('context-attention');
    expect(button).toHaveTextContent('Source changed — choose another context');
  });
});
