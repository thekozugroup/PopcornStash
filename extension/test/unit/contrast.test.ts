/**
 * Action Blue in the extension's styles, measured with the WCAG 2 contrast
 * formula in light and dark (the same bar as the web app's theme test):
 * white on Action Blue and on its hovered fills at 4.5:1 or more, and thin
 * Action Blue indicators (focus rings, the progress fill, an on switch) at
 * 3:1 or more against what they sit on.
 */
import {readFileSync} from 'node:fs';
import {describe, expect, it} from 'vitest';

type Scheme = 'light' | 'dark';
/** Gamma-encoded sRGB channels, 0 to 1, with alpha. */
type Rgba = [number, number, number, number];

const read = (name: string) =>
  readFileSync(new URL(`../../src/styles/${name}`, import.meta.url), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
const base = read('base.css');
const options = read('options.css');

/** The custom properties of the first `:root { ... }` block in `text`. */
function rootTokens(text: string): Map<string, string> {
  const body = /:root\s*\{([^{}]*)\}/.exec(text)?.[1] ?? '';
  return new Map(
    [...body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)].map(match => [match[1] ?? '', (match[2] ?? '').trim()]),
  );
}

const light = rootTokens(base);
const darkBlock = /@media \(prefers-color-scheme: dark\)\s*\{([\s\S]*?\})\s*\}/.exec(base)?.[1] ?? '';
const dark = new Map([...light, ...rootTokens(darkBlock)]);

/** The declarations of the rule whose selector is exactly `selector`. */
function declarations(text: string, selector: string): string {
  for (const match of text.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    if ((match[1] ?? '').trim() === selector) return match[2] ?? '';
  }
  throw new Error(`no rule ${selector}`);
}

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
  const [r = 0, g = 0, b = 0] = [
    4.0767416621 * lc - 3.3077115913 * mc + 0.2309699292 * sc,
    -1.2684380046 * lc + 2.6097574011 * mc - 0.3413193965 * sc,
    -0.0041960863 * lc - 0.7034186147 * mc + 1.707614701 * sc,
  ].map(v => encode(Math.min(1, Math.max(0, v))));
  return [r, g, b, a];
}

/** `top` painted over an opaque `bottom` (browsers composite in gamma-encoded sRGB). */
function over(top: Rgba, bottom: Rgba): Rgba {
  const [r = 0, g = 0, b = 0] = [0, 1, 2].map(i => top[i]! * top[3] + bottom[i]! * (1 - top[3]));
  return [r, g, b, 1];
}

/** A colour value as the browser computes it in `scheme`. */
function color(value: string, scheme: Scheme): Rgba {
  const tokens = scheme === 'light' ? light : dark;
  const text = value.trim();
  const call = /^([a-z-]+)\((.*)\)$/s.exec(text);
  const args = call?.[2] ?? '';
  if (call?.[1] === 'var') {
    const found = tokens.get(args.trim());
    if (found === undefined) throw new Error(`${args} is not defined`);
    return color(found, scheme);
  }
  if (call?.[1] === 'oklch') {
    const [channels = '', alpha] = args.split('/');
    const [l = '0', c = '0', h = '0'] = channels.trim().split(/\s+/);
    const opacity = alpha === undefined ? 1 : Number.parseFloat(alpha) / (alpha.includes('%') ? 100 : 1);
    return fromOklch(Number.parseFloat(l) / (l.endsWith('%') ? 100 : 1), Number(c), Number(h), opacity);
  }
  if (call?.[1] === 'color-mix') {
    // color-mix(in srgb, A, B P%): P% of B, the rest A, in gamma-encoded sRGB.
    const [space = '', a = '', bWithShare = ''] = splitArgs(args);
    if (space !== 'in srgb') throw new Error(`cannot mix ${space}`);
    const share = /\s([\d.]+)%$/.exec(bWithShare);
    const p = share ? Number(share[1]) / 100 : 0.5;
    const first = color(a, scheme);
    const second = color(share ? bWithShare.slice(0, share.index) : bWithShare, scheme);
    const [r = 0, g = 0, b = 0] = [0, 1, 2].map(i => first[i]! * (1 - p) + second[i]! * p);
    return [r, g, b, 1];
  }
  throw new Error(`cannot read the colour ${text}`);
}

function luminance(c: Rgba): number {
  return 0.2126 * decode(c[0]) + 0.7152 * decode(c[1]) + 0.0722 * decode(c[2]);
}

function contrast(a: Rgba, b: Rgba): number {
  const [hi = 0, lo = 0] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

describe('the styles paint Action Blue with the tokens measured below', () => {
  it('uses the on-accent for what sits on blue, and the darker hovers', () => {
    expect(declarations(base, '.button-primary')).toMatch(/background: var\(--accent\);\s*color: var\(--on-accent\)/);
    expect(declarations(base, '.button-primary:hover:not(:disabled)')).toContain('var(--accent-hover)');
    expect(declarations(options, '.switch:checked')).toContain('var(--accent)');
    expect(declarations(options, '.switch:checked:hover')).toContain('var(--switch-on-hover)');
    expect(declarations(options, '.switch:checked::before')).toContain('var(--on-accent)');
    expect(declarations(base, '.progress')).toContain('var(--track)');
    expect(declarations(base, '.progress-fill')).toContain('var(--accent)');
    expect(declarations(base, '.notice')).toContain('var(--notice-fill)');
    expect(declarations(base, '.notice-error')).toContain('var(--error-fill)');
    expect(declarations(base, '.notice-text')).toContain('var(--text)');
  });
});

describe.each(['light', 'dark'] as const)('Action Blue in %s', scheme => {
  const token = (name: string) => color(`var(${name})`, scheme);
  const accent = token('--accent');
  const white = token('--on-accent');
  const page = token('--page');
  const surface = token('--surface');

  it('is blue-600 with white on it, at 4.5:1 or more, and darker on hover', () => {
    expect(accent.slice(0, 3).map(v => Math.round(v * 255))).toEqual([0x15, 0x5d, 0xfc]);
    expect(white.slice(0, 3).map(v => Math.round(v * 255))).toEqual([255, 255, 255]);
    for (const fill of ['--accent', '--accent-hover', '--switch-on-hover']) {
      expect(contrast(white, token(fill)), fill).toBeGreaterThanOrEqual(4.5);
      if (fill !== '--accent') expect(luminance(token(fill)), fill).toBeLessThan(luminance(accent));
    }
  });

  it('keeps thin Action Blue indicators at 3:1 or more against what they sit on', () => {
    // Focus rings: the page, a panel, and a notice that holds a button.
    for (const behind of ['--page', '--surface', '--notice-fill']) {
      expect(contrast(accent, token(behind)), behind).toBeGreaterThanOrEqual(3);
    }
    // An error notice holds a button too ("Try again", "Allow access to <host>"): its
    // translucent fill over the page or a panel.
    for (const [name, behind] of Object.entries({page, surface})) {
      const fill = over(token('--error-fill'), behind);
      expect(contrast(accent, fill), `error notice on the ${name}`).toBeGreaterThanOrEqual(3);
      expect(contrast(token('--text'), fill), `notice text on the ${name}`).toBeGreaterThanOrEqual(4.5);
    }
    // A download's progress fill against its track.
    expect(contrast(accent, token('--track'))).toBeGreaterThanOrEqual(3);
    // An on switch, at rest and hovered, against the page.
    expect(contrast(accent, page)).toBeGreaterThanOrEqual(3);
    expect(contrast(token('--switch-on-hover'), page)).toBeGreaterThanOrEqual(3);
  });

  it('keeps selected text at 4.5:1 or more on the selection tint', () => {
    for (const behind of [page, surface]) {
      expect(contrast(token('--text'), over(token('--selection'), behind))).toBeGreaterThanOrEqual(4.5);
    }
  });
});
