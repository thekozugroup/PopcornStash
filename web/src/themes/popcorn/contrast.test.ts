/**
 * Action Blue pairings in the built Popcorn theme (popcorn.css, what ships),
 * measured with the WCAG 2 contrast formula in light and dark:
 *
 * - White on Action Blue, and on every hovered or pressed Action Blue fill,
 *   at 4.5:1 or more: button labels, checkbox marks, the switch's on knob.
 * - Thin Action Blue indicators at 3:1 or more against what they sit on:
 *   focus rings, the caret, progress fills against their track, checked
 *   boxes and on switches against the page and a panel.
 *
 * DESIGN.md (Colors, Action Blue) records the same numbers.
 */
import {describe, expect, it} from 'vitest';
import css from './popcorn.css?raw';

type Scheme = 'light' | 'dark';
/** Gamma-encoded sRGB channels, 0 to 1, with alpha. */
type Rgba = [number, number, number, number];
type Declarations = Map<string, string>;

/** Every innermost rule block of the stylesheet: its selector and its declarations. */
function parseRules(text: string): {selector: string; declarations: Declarations}[] {
  const rules: {selector: string; declarations: Declarations}[] = [];
  for (const match of text.replace(/\/\*[\s\S]*?\*\//g, '').matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const declarations: Declarations = new Map();
    for (const line of (match[2] ?? '').split(';')) {
      const colon = line.indexOf(':');
      if (colon > 0) declarations.set(line.slice(0, colon).trim(), line.slice(colon + 1).trim());
    }
    rules.push({selector: (match[1] ?? '').trim(), declarations});
  }
  return rules;
}

const rules = parseRules(css);
const tokens: Declarations = new Map(
  rules.filter(rule => rule.selector === ':scope').flatMap(rule => [...rule.declarations]),
);

/** The declarations of one component rule, by its exact selector. */
function rule(selector: string): Declarations {
  const found = rules.find(candidate => candidate.selector === selector);
  if (!found) throw new Error(`popcorn.css has no rule ${selector}`);
  return found.declarations;
}

/** Split on commas that are not inside parentheses. */
function splitArgs(text: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < text.length; i += 1) {
    if (text[i] === '(') depth += 1;
    else if (text[i] === ')') depth -= 1;
    else if (text[i] === ',' && depth === 0) {
      parts.push(text.slice(start, i).trim());
      start = i + 1;
    }
  }
  parts.push(text.slice(start).trim());
  return parts;
}

const encode = (v: number) => (v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055);
const decode = (v: number) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);

function fromOklch(l: number, c: number, h: number, a: number): Rgba {
  const rad = (h * Math.PI) / 180;
  const ca = c * Math.cos(rad);
  const cb = c * Math.sin(rad);
  const lc = (l + 0.3963377774 * ca + 0.2158037573 * cb) ** 3;
  const mc = (l - 0.1055613458 * ca - 0.0638541728 * cb) ** 3;
  const sc = (l - 0.0894841775 * ca - 1.291485548 * cb) ** 3;
  const linear = [
    4.0767416621 * lc - 3.3077115913 * mc + 0.2309699292 * sc,
    -1.2684380046 * lc + 2.6097574011 * mc - 0.3413193965 * sc,
    -0.0041960863 * lc - 0.7034186147 * mc + 1.707614701 * sc,
  ].map(v => encode(Math.min(1, Math.max(0, v))));
  return [linear[0] ?? 0, linear[1] ?? 0, linear[2] ?? 0, a];
}

/**
 * A colour value as the browser computes it inside `scopes` (outermost first):
 * var() looks from the innermost scope out, light-dark() takes the scheme's side.
 */
function color(value: string, scheme: Scheme, scopes: Declarations[] = [tokens]): Rgba {
  const text = value.trim();
  const call = /^([a-z-]+)\((.*)\)$/s.exec(text);
  if (call?.[1] === 'var') {
    const [name = '', fallback] = splitArgs(call[2] ?? '');
    for (let i = scopes.length - 1; i >= 0; i -= 1) {
      const found = scopes[i]?.get(name);
      if (found !== undefined) return color(found, scheme, scopes.slice(0, i + 1));
    }
    if (fallback !== undefined) return color(fallback, scheme, scopes);
    throw new Error(`${name} is not defined`);
  }
  if (call?.[1] === 'light-dark') {
    const [light = '', dark = ''] = splitArgs(call[2] ?? '');
    return color(scheme === 'light' ? light : dark, scheme, scopes);
  }
  if (call?.[1] === 'oklch') {
    const [channels = '', alphaPart] = (call[2] ?? '').split('/');
    const [l = '0', c = '0', h = '0'] = channels.trim().split(/\s+/);
    const lightness = l.endsWith('%') ? Number.parseFloat(l) / 100 : Number(l);
    const alphaValue = alphaPart === undefined ? 1 : Number.parseFloat(alphaPart);
    return fromOklch(lightness, Number(c), h === 'none' ? 0 : Number(h), alphaValue);
  }
  if (text === 'white') return [1, 1, 1, 1];
  if (text === 'black') return [0, 0, 0, 1];
  const hex = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})?$/i.exec(text);
  if (hex) {
    const [r, g, b, a] = hex.slice(1).map(part => (part === undefined ? 255 : Number.parseInt(part, 16)) / 255);
    return [r ?? 0, g ?? 0, b ?? 0, a ?? 1];
  }
  throw new Error(`cannot read the colour ${text}`);
}

/** `top` painted over an opaque `bottom` (browsers composite in gamma-encoded sRGB). */
function over(top: Rgba, bottom: Rgba): Rgba {
  const a = top[3];
  return [0, 1, 2].map(i => (top[i] ?? 0) * a + (bottom[i] ?? 0) * (1 - a)).concat(1) as Rgba;
}

/** color-mix(in srgb, a, b `share`), as Astryx tints a checked control on hover. */
function mix(a: Rgba, b: Rgba, share: number): Rgba {
  return [0, 1, 2].map(i => (a[i] ?? 0) * (1 - share) + (b[i] ?? 0) * share).concat(1) as Rgba;
}

function luminance(c: Rgba): number {
  return 0.2126 * decode(c[0]) + 0.7152 * decode(c[1]) + 0.0722 * decode(c[2]);
}

/** WCAG 2 contrast ratio of two opaque colours. */
function contrast(a: Rgba, b: Rgba): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return ((hi ?? 0) + 0.05) / ((lo ?? 0) + 0.05);
}

const token = (name: string, scheme: Scheme, scopes?: Declarations[]) => color(`var(${name})`, scheme, scopes);

describe.each(['light', 'dark'] as const)('Action Blue in %s', scheme => {
  const accent = token('--color-accent', scheme);
  const white = token('--color-on-accent', scheme);
  const page = token('--color-background-body', scheme);
  const panel = token('--color-background-surface', scheme);
  const popover = token('--color-background-popover', scheme);

  it('is Tailwind blue-600 with white on it, at 4.5:1 or more', () => {
    expect(accent.slice(0, 3).map(v => Math.round(v * 255))).toEqual([0x15, 0x5d, 0xfc]);
    expect(white.slice(0, 3).map(v => Math.round(v * 255))).toEqual([255, 255, 255]);
    expect(contrast(white, accent)).toBeGreaterThanOrEqual(4.5);
    // Badges and status fills on Action Blue, and the focus ring, are the same blue.
    expect(token('--astryx-theme-neutral-color-status-fill-accent', scheme)).toEqual(accent);
    expect(token('--focus-outline-color', scheme)).toEqual(accent);
  });

  it('keeps a primary button label at 4.5:1 or more on hover and press, going darker', () => {
    const primary = [tokens, rule('.astryx-button[data-variant="primary"]')];
    for (const state of ['--color-overlay-hover', '--color-overlay-pressed']) {
      const fill = over(token(state, scheme, primary), accent);
      expect(contrast(white, fill), state).toBeGreaterThanOrEqual(4.5);
      expect(luminance(fill), state).toBeLessThan(luminance(accent));
    }
  });

  it('keeps the white mark and knob at 4.5:1 or more on a hovered checked control', () => {
    const controls = [
      '.astryx-switch[data-checked="checked"]',
      '.astryx-checkbox-indicator[data-checked="checked"]',
      '.astryx-radio-indicator[data-checked="checked"]',
    ];
    for (const selector of controls) {
      // Astryx: color-mix(in srgb, accent, tint-hover 15%) while the control is hovered.
      const hovered = mix(accent, token('--color-tint-hover', scheme, [tokens, rule(selector)]), 0.15);
      expect(contrast(white, hovered), selector).toBeGreaterThanOrEqual(4.5);
      expect(luminance(hovered), selector).toBeLessThan(luminance(accent));
      // Still a control that stands out from the page and a panel while hovered.
      expect(contrast(hovered, page), selector).toBeGreaterThanOrEqual(3);
      expect(contrast(hovered, panel), selector).toBeGreaterThanOrEqual(3);
    }
    const knob = token('--color-background-surface', scheme, [
      tokens,
      rule('.astryx-switch-thumb[data-checked="checked"]'),
    ]);
    expect(contrast(knob, accent)).toBeGreaterThanOrEqual(4.5);
  });

  it('keeps thin Action Blue indicators at 3:1 or more against what they sit on', () => {
    // Focus rings, the caret, checked boxes and on switches: page, panel, dialog.
    for (const [name, surface] of Object.entries({page, panel, popover})) {
      expect(contrast(accent, surface), name).toBeGreaterThanOrEqual(3);
    }
    // The fill that holds the Inbox edit form, whose focus rings sit on it.
    expect(contrast(accent, token('--popcorn-control-well', scheme))).toBeGreaterThanOrEqual(3);
    // A download's progress fill against its track.
    const bar = [tokens, rule('.astryx-progress-bar[data-variant="accent"]')];
    const fill = token('--color-accent', scheme, bar);
    const track = over(token('--color-background-muted', scheme, bar), panel);
    expect(contrast(fill, track)).toBeGreaterThanOrEqual(3);
    // The watched hairline on a tile or an episode frame, on its Stage Black track.
    expect(contrast(accent, token('--popcorn-stage', scheme))).toBeGreaterThanOrEqual(3);
  });

  it('keeps selected text at 4.5:1 or more on the selection tint', () => {
    const tint = over(token('--color-accent-muted', scheme), panel);
    expect(contrast(token('--color-text-primary', scheme), tint)).toBeGreaterThanOrEqual(4.5);
  });
});

describe('Action Blue over media', () => {
  it('puts dark content on the light accent a dark picture gets, and light on the dark one', () => {
    for (const [media, scheme] of [['dark', 'dark'], ['light', 'light']] as const) {
      const surface = [tokens, rule(`[data-astryx-media="${media}"]`)];
      const fill = token('--color-accent', scheme, surface);
      expect(contrast(token('--color-on-accent', scheme, surface), fill), media).toBeGreaterThanOrEqual(4.5);
    }
  });
});
