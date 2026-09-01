import { render, screen } from '@testing-library/svelte';
import axe from 'axe-core';
import { describe, expect, it } from 'vitest';

import App from './App.svelte';

describe('application shell', () => {
  it('renders the Library as the primary window surface', () => {
    render(App);

    expect(screen.getByRole('heading', { name: 'Recent' })).toBeVisible();
  });

  it('has no automatically detectable accessibility violations', async () => {
    const { container } = render(App);

    const results = await axe.run(container);

    expect(results.violations).toEqual([]);
  });
});
