import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import CaptureStream from './CaptureStream.svelte';
import type { CaptureSummary } from '../lib/ipc-types';

function capture(id: string, capturedAt: Date): CaptureSummary {
  return {
    id,
    kind: 'text',
    context: { id: 'project-1', kind: 'project', name: 'Lyn' },
    branchName: null,
    capturedAt: capturedAt.toISOString(),
    textExcerpt: `Note ${id}`,
    caption: null,
    captionSource: null,
    media: null,
  };
}

describe('capture stream day groups', () => {
  it('labels today and yesterday by name and older days by date', () => {
    const now = new Date();
    const yesterday = new Date(now);
    yesterday.setDate(now.getDate() - 1);
    const older = new Date(2026, 0, 15, 12);

    render(CaptureStream, {
      captures: [
        capture('a', now),
        capture('b', yesterday),
        capture('c', older),
      ],
      onselect: vi.fn(),
    });

    const headings = screen
      .getAllByRole('heading', { level: 2 })
      .map((heading) => heading.textContent);
    expect(headings).toEqual([
      'Today',
      'Yesterday',
      new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' }).format(older),
    ]);
  });

  it('localizes the relative day labels', () => {
    render(CaptureStream, {
      captures: [capture('a', new Date())],
      language: 'french',
      onselect: vi.fn(),
    });
    expect(
      screen.getByRole('heading', { level: 2, name: 'Aujourd’hui' }),
    ).toBeVisible();
  });
});
