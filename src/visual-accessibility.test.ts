import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { render, screen, waitFor } from '@testing-library/svelte';
import axe from 'axe-core';
import { describe, expect, it, vi } from 'vitest';

import LibraryPage from './library/LibraryPage.svelte';
import type { LibraryClient } from './library/library-client';
import type { SettingsClient } from './settings/settings-client';

const styles = readFileSync(resolve(import.meta.dirname, 'styles.css'), 'utf8');

function channel(hex: string): [number, number, number] {
  const value = Number.parseInt(hex.slice(1), 16);
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255];
}

function luminance(hex: string): number {
  const linear = channel(hex).map((sample) => {
    const value = sample / 255;
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2];
}

function contrast(foreground: string, background: string): number {
  const lighter = Math.max(luminance(foreground), luminance(background));
  const darker = Math.min(luminance(foreground), luminance(background));
  return (lighter + 0.05) / (darker + 0.05);
}

const light = {
  canvas: '#f6f7f3',
  surface: '#ffffff',
  subtle: '#eef1eb',
  text: '#171a17',
  muted: '#5f675f',
  faint: '#6b746a',
  accent: '#286b59',
  onAccent: '#ffffff',
  soft: '#e3f0ea',
  error: '#b4232c',
  errorSoft: '#fbe7e8',
  warning: '#9a5b13',
} as const;

const dark = {
  canvas: '#101310',
  surface: '#171b17',
  subtle: '#202620',
  text: '#f1f4ef',
  muted: '#a7b0a5',
  faint: '#8e988c',
  accent: '#79c4a9',
  onAccent: '#102018',
  soft: '#18372d',
  error: '#ff858b',
  errorSoft: '#3c191c',
  warning: '#e5a44f',
} as const;

describe('visual accessibility verification', () => {
  it('keeps semantic text and status token pairs above WCAG AA contrast', () => {
    for (const [theme, tokens] of [
      ['light', light],
      ['dark', dark],
    ] as const) {
      const pairs: Array<[string, string, string, number]> = [
        ['text/canvas', tokens.text, tokens.canvas, 4.5],
        ['text/surface', tokens.text, tokens.surface, 4.5],
        ['muted/canvas', tokens.muted, tokens.canvas, 4.5],
        ['muted/surface', tokens.muted, tokens.surface, 4.5],
        ['faint/canvas', tokens.faint, tokens.canvas, 4.5],
        ['faint/surface', tokens.faint, tokens.surface, 4.5],
        ['on-accent/accent', tokens.onAccent, tokens.accent, 4.5],
        ['error/surface', tokens.error, tokens.surface, 4.5],
        ['warning/surface', tokens.warning, tokens.surface, 4.5],
        ['text/accent-soft', tokens.text, tokens.soft, 4.5],
        ['text/error-soft', tokens.text, tokens.errorSoft, 4.5],
        ['accent/subtle', tokens.accent, tokens.subtle, 3],
      ];
      for (const [label, foreground, background, minimum] of pairs) {
        expect(
          contrast(foreground, background),
          `${theme} ${label}`,
        ).toBeGreaterThanOrEqual(minimum);
      }
    }
  });

  it('avoids prohibited motion, font, and remote-asset patterns in shared styles', () => {
    expect(styles).not.toMatch(/transition\s*:\s*all\b/i);
    expect(styles).not.toMatch(/will-change\s*:/i);
    expect(styles).not.toMatch(/fonts\.googleapis|fonts\.gstatic|typekit/i);
    expect(styles).not.toMatch(/@import\s+url\(/i);
    expect(styles).toMatch(/prefers-reduced-motion:\s*reduce/);
    expect(styles).toMatch(/:focus-visible/);
    expect(styles).toMatch(/min-width:\s*320px/);
    expect(styles).toMatch(/width:\s*min\(100%,\s*620px\)/);
  });

  it('treats the Library brand mark as decorative beside the visible Lyn name', async () => {
    const library: LibraryClient = {
      listContexts: vi.fn().mockResolvedValue([]),
      listCaptures: vi.fn().mockResolvedValue({ items: [], nextCursor: null }),
      getCapture: vi.fn(),
      searchCaptures: vi.fn(),
      playMedia: vi.fn(),
      openMedia: vi.fn(),
      stopPlayback: vi.fn(),
    };
    const settings: SettingsClient = {
      get: vi.fn().mockResolvedValue({
        globalShortcut: 'Control+Shift+Space',
        providerTieBreakOrder: ['vscode', 'shell', 'foreground_window'],
        theme: 'system',
        localSpeechEnabled: false,
      }),
      update: vi.fn(),
    };

    const { container } = render(LibraryPage, {
      client: library,
      settings,
    });

    await waitFor(() =>
      expect(screen.getByRole('heading', { name: 'Recent' })).toBeVisible(),
    );
    const brand = container.querySelector('.library-brand');
    expect(brand?.textContent).toContain('Lyn');
    const logo = brand?.querySelector('img');
    expect(logo).toHaveAttribute('alt', '');
    expect((await axe.run(container)).violations).toEqual([]);
  });
});
