import { render, screen } from '@testing-library/svelte';
import axe from 'axe-core';
import { describe, expect, it } from 'vitest';

import App from './App.svelte';

describe('application shell', () => {
  it('identifies Lyn with a semantic page heading', () => {
    render(App);

    expect(
      screen.getByRole('heading', { level: 1, name: 'Lyn' }),
    ).toBeVisible();
  });

  it('has no automatically detectable accessibility violations', async () => {
    const { container } = render(App);

    const results = await axe.run(container);

    expect(results.violations).toEqual([]);
  });
});
