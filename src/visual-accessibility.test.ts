import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';

import { render, screen, waitFor } from '@testing-library/svelte';
import axe from 'axe-core';
import { describe, expect, it, vi } from 'vitest';

import LibraryPage from './library/LibraryPage.svelte';
import type { LibraryClient } from './library/library-client';
import type { SettingsClient } from './settings/settings-client';

const styles = readFileSync(resolve(import.meta.dirname, 'styles.css'), 'utf8');

function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return /\.(svelte|css)$/.test(entry.name) ? [path] : [];
  });
}

const sources = sourceFiles(import.meta.dirname).map((path) => ({
  path,
  text: readFileSync(path, 'utf8'),
}));

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

type Tokens = Record<string, string>;

// Read the palette from the shipped stylesheet so the checks below cover the
// colors users actually see, not a hand-maintained copy.
function block(source: string, selector: string): string {
  const start = source.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`missing theme block ${selector}`);
  const open = source.indexOf('{', start);
  return source.slice(open + 1, source.indexOf('}', open));
}

function tokens(body: string): Tokens {
  return Object.fromEntries(
    Array.from(body.matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/g), (match) => [
      match[1],
      match[2].replace(/\s+/g, ' ').trim(),
    ]),
  );
}

function mediaBlock(source: string, query: string): string {
  const start = source.indexOf(`@media ${query} {`);
  if (start < 0) throw new Error(`missing media block ${query}`);
  let depth = 0;
  for (let index = source.indexOf('{', start); index < source.length; index++) {
    if (source[index] === '{') depth++;
    if (source[index] === '}' && --depth === 0)
      return source.slice(source.indexOf('{', start) + 1, index);
  }
  throw new Error(`unterminated media block ${query}`);
}

const systemDarkMedia = mediaBlock(styles, '(prefers-color-scheme: dark)');
const light = tokens(block(styles, ':root'));
const explicitDark = tokens(block(styles, ":root[data-theme='dark']"));
const systemDark = tokens(
  block(systemDarkMedia, ":root:not([data-theme='light'])"),
);

// Tokens whose value must differ per theme; layout tokens stay shared.
const themedTokens = Object.keys(light).filter((name) =>
  /^--(color-|shadow-|media-)/.test(name),
);

describe('visual accessibility verification', () => {
  it('keeps the explicit and system dark palettes identical and complete', () => {
    expect(themedTokens.length).toBeGreaterThan(10);
    expect(explicitDark).toEqual(systemDark);
    expect(
      themedTokens.filter((name) => !(name in explicitDark)),
      'light tokens without a dark value',
    ).toEqual([]);
    expect(
      Object.keys(explicitDark).filter((name) => !(name in light)),
      'dark tokens without a light default',
    ).toEqual([]);
  });

  it('keeps theme-specific styling in tokens, not system-only rules', () => {
    // A rule here would apply for the OS preference but not for an explicit
    // Settings choice; express it as a token in all three theme blocks.
    const rest = systemDarkMedia
      .replace(/:root:not\(\[data-theme='light'\]\)\s*{[^}]*}/, '')
      .trim();
    expect(rest).toBe('');
  });

  it('keeps semantic text and status token pairs above WCAG AA contrast', () => {
    for (const [theme, palette] of [
      ['light', light],
      ['dark', explicitDark],
    ] as const) {
      const color = (name: string) => {
        const value = palette[`--color-${name}`];
        expect(value, `${theme} --color-${name}`).toMatch(/^#[0-9a-f]{6}$/i);
        return value;
      };
      const pairs: Array<[string, string, string, number]> = [
        ['text', 'canvas', 'text/canvas', 4.5],
        ['text', 'surface', 'text/surface', 4.5],
        ['text-muted', 'canvas', 'muted/canvas', 4.5],
        ['text-muted', 'surface', 'muted/surface', 4.5],
        ['text-faint', 'canvas', 'faint/canvas', 4.5],
        ['text-faint', 'surface', 'faint/surface', 4.5],
        ['on-accent', 'accent', 'on-accent/accent', 4.5],
        ['error', 'surface', 'error/surface', 4.5],
        ['warning', 'surface', 'warning/surface', 4.5],
        ['text', 'accent-soft', 'text/accent-soft', 4.5],
        ['text', 'error-soft', 'text/error-soft', 4.5],
        ['accent', 'surface-subtle', 'accent/subtle', 3],
        ['info', 'surface', 'info/surface', 3],
      ].map(([fg, bg, label, minimum]) => [
        label as string,
        color(fg as string),
        color(bg as string),
        minimum as number,
      ]);
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

  it('defines every design token referenced by component styles', () => {
    const defined = new Set(
      sources.flatMap(({ text }) =>
        Array.from(text.matchAll(/(--[a-z0-9-]+)\s*:/g), (match) => match[1]),
      ),
    );
    const missing = sources.flatMap(({ path, text }) =>
      Array.from(text.matchAll(/var\((--[a-z0-9-]+)\s*\)/g))
        .map((match) => match[1])
        .filter((token) => !defined.has(token))
        .map((token) => `${token} in ${path}`),
    );
    expect(missing).toEqual([]);
  });

  it('keeps no stylesheet class that markup no longer uses', () => {
    const markup = sources
      .filter(({ path }) => path.endsWith('.svelte'))
      .map(({ text }) => text)
      .join('\n');
    const css = styles.replace(/\/\*[\s\S]*?\*\//g, '');
    const classes = new Set(
      Array.from(css.matchAll(/\.([a-z][\w-]*)/gi), (match) => match[1]),
    );
    const unused = Array.from(classes).filter(
      (name) => !new RegExp(`(?<![\\w-])${name}(?![\\w-])`).test(markup),
    );
    expect(unused).toEqual([]);
  });

  it('keeps enabled provider reorder icons at full strength when idle', () => {
    const rule = block(styles, '.provider-actions button');
    // Translucent idle icons fell to ~2.45:1 on the light surface.
    expect(rule).not.toMatch(/opacity\s*:/);
    const token = rule.match(/color:\s*var\((--color-[a-z-]+)\)/)?.[1];
    expect(token).toBeDefined();
    for (const [theme, palette] of [
      ['light', light],
      ['dark', explicitDark],
    ] as const) {
      expect(
        contrast(palette[token!], palette['--color-surface']),
        `${theme} idle reorder icon`,
      ).toBeGreaterThanOrEqual(3);
    }
    const disabled = block(styles, 'button:disabled');
    expect(disabled).toMatch(/opacity:\s*0\.48/);
  });

  it('styles the classes used by provider reordering controls', () => {
    const markup = readFileSync(
      resolve(import.meta.dirname, 'settings/SettingsPanel.svelte'),
      'utf8',
    );
    const actionClass = markup.match(/<div class="(provider-[a-z-]+)">/)?.[1];
    expect(actionClass).toBeDefined();
    expect(styles).toContain(`.${actionClass} button`);
  });

  it('sets an explicit base text size and per-theme native control scheme', () => {
    expect(styles).toMatch(/body\s*{[^}]*font-size:\s*13px/);
    expect(styles).toMatch(
      /:root\[data-theme='dark'\]\s*{\s*color-scheme:\s*dark;/,
    );
    expect(styles).toMatch(
      /:root\[data-theme='light'\]\s*{\s*color-scheme:\s*light;/,
    );
  });

  it('uses the bundled Inter face and type-scale tokens for every text size', () => {
    const entry = readFileSync(resolve(import.meta.dirname, 'main.ts'), 'utf8');
    expect(entry).toContain("import '@fontsource-variable/inter/wght.css';");
    expect(styles).toMatch(/font-family:\s*'Inter Variable'/);
    const rawSizes = Array.from(
      styles.matchAll(/font-size:\s*(\d+(?:\.\d+)?px)/g),
      (match) => match[1],
    );
    // Only the body root size is literal; everything else uses --text-*.
    expect(rawSizes).toEqual(['13px']);
  });

  it('treats the Library brand mark as decorative beside the visible Lyn name', async () => {
    const library: LibraryClient = {
      updateVoiceCaption: vi.fn(),
      retryVoiceTranscription: vi.fn(),
      listContexts: vi.fn().mockResolvedValue([]),
      listCaptures: vi.fn().mockResolvedValue({ items: [], nextCursor: null }),
      getCapture: vi.fn(),
      deleteCapture: vi.fn(),
      deleteProject: vi.fn(),
      searchCaptures: vi.fn(),
      playMedia: vi.fn(),
      openMedia: vi.fn(),
      stopPlayback: vi.fn(),
    };
    const settings: SettingsClient = {
      get: vi.fn().mockResolvedValue({
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
      }),
      update: vi.fn(),
      version: vi.fn().mockResolvedValue({ version: '0.6.4' }),
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
