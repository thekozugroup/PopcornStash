/**
 * Popcorn theme for Astryx.
 *
 * Maps the PopcornStash DESIGN.md tokens (shared with Popcorn SR) onto
 * Astryx's themeable surface: Tailwind zinc neutrals, Tailwind blue-500 as
 * the single action and focus accent with zinc-950 text on blue, the platform
 * system sans, flat panels, thin dividers, 44px controls, 8px control radius,
 * 16px panel radius and 10px media radius. Light and dark follow the system
 * preference through Astryx's [light, dark] token tuples.
 *
 * It extends the maintained neutral theme so every token and component rule
 * Popcorn does not mention keeps a coherent, tested default.
 *
 * Build after editing:
 *   npm run astryx -- theme build src/themes/popcorn/popcornTheme.ts
 */
import {defineTheme, type TokenValue} from '@astryxdesign/core/theme';
import {neutralTheme} from '@astryxdesign/theme-neutral';
import {popcornIconRegistry} from './icons';

// ---------------------------------------------------------------------------
// Palette (Tailwind v4 values, as named in DESIGN.md)
// ---------------------------------------------------------------------------
const zinc = {
  50: 'oklch(98.5% 0 0)', // zinc-paper
  100: 'oklch(96.7% 0.001 286.375)', // zinc-subtle
  200: 'oklch(92% 0.004 286.32)', // zinc-divider
  300: 'oklch(87.1% 0.006 286.286)',
  400: 'oklch(70.5% 0.015 286.067)', // zinc-dark-muted
  500: 'oklch(55.2% 0.016 285.938)', // zinc-status
  600: 'oklch(44.2% 0.017 285.786)', // zinc-muted
  700: 'oklch(37% 0.013 285.805)',
  800: 'oklch(27.4% 0.006 286.033)', // zinc-dark-divider
  900: 'oklch(21% 0.006 285.885)', // zinc-ink
  950: 'oklch(14.1% 0.005 285.823)', // zinc-black
} as const;

// Written in oklch so alpha() can derive translucent variants from it.
const white = 'oklch(100% 0 0)';

const blue = {
  400: 'oklch(70.7% 0.165 254.624)',
  500: 'oklch(62.3% 0.214 259.815)', // action blue
  600: 'oklch(54.6% 0.245 262.881)',
  700: 'oklch(48.8% 0.243 264.376)',
} as const;

const emerald = {
  400: 'oklch(76.5% 0.177 163.223)',
  500: 'oklch(69.6% 0.17 162.48)', // status-connected
  700: 'oklch(50.8% 0.118 165.612)',
} as const;

const amber = {
  400: 'oklch(82.8% 0.189 84.429)',
  500: 'oklch(76.9% 0.188 70.08)', // status-warning
  700: 'oklch(55.5% 0.163 48.998)',
} as const;

const red = {
  400: 'oklch(70.4% 0.191 22.216)',
  500: 'oklch(63.7% 0.237 25.331)', // status-error
  600: 'oklch(57.7% 0.245 27.325)',
  700: 'oklch(50.5% 0.213 27.518)',
} as const;

/** `oklch(L C H)` → `oklch(L C H / amount)`. Every palette value above is oklch. */
const alpha = (color: string, amount: number) => {
  if (!color.startsWith('oklch(') || !color.endsWith(')')) {
    throw new Error(`alpha() needs an oklch() colour, got ${color}`);
  }
  return color.replace(/\)$/, ` / ${amount})`);
};

const systemSans = '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';

// ---------------------------------------------------------------------------
// Theme-local tokens. The neutral base routes its solid status fills through
// these names, so overriding them recolours Badge, StatusDot and Banner fills
// without touching component rules.
// ---------------------------------------------------------------------------
const localTokens: Record<string, TokenValue> = {
  '--astryx-theme-neutral-color-status-fill-accent': blue[500],
  '--astryx-theme-neutral-color-status-fill-success': emerald[500],
  '--astryx-theme-neutral-color-status-fill-warning': amber[500],
  '--astryx-theme-neutral-color-status-fill-error': red[500],
  '--astryx-theme-neutral-color-status-muted-accent': [
    alpha(blue[500], 0.14),
    alpha(blue[500], 0.24),
  ],
  // Popcorn shape and media tokens that Astryx has no slot for.
  '--popcorn-radius-media': '10px',
  '--popcorn-radius-tag': '6px',
  '--popcorn-radius-progress': '4px',
  '--popcorn-media-hairline': [alpha(zinc[950], 0.08), alpha(white, 0.08)],
  '--popcorn-media-placeholder': [zinc[100], zinc[800]],
  '--popcorn-stage': 'oklch(0% 0 0)',
  // Controls laid over media (the photo viewer's arrows and close button): a
  // translucent zinc-950 disc that reads on light and dark pictures alike.
  '--popcorn-media-control': alpha(zinc[950], 0.62),
  '--popcorn-media-control-hover': alpha(zinc[950], 0.8),
  '--popcorn-status-connected': emerald[500],
  '--popcorn-status-warning': amber[500],
  '--popcorn-status-error': red[500],
  '--popcorn-status-neutral': zinc[500],
};

export const popcornTheme = defineTheme({
  name: 'popcorn',
  extends: neutralTheme,
  icons: popcornIconRegistry,
  localTokens,

  // System sans for every role. Sizes come from the Astryx scale (14px base,
  // 1.2 ratio) and the explicit type tokens below, which pin the DESIGN.md
  // ramp: headline 30px, title 20px, section 16px, body 16px, label 14px,
  // metadata 13px.
  typography: {
    scale: {base: 14, ratio: 1.2},
    body: {
      family: '-apple-system',
      fallbacks: 'BlinkMacSystemFont, "Segoe UI", sans-serif',
    },
    heading: {
      family: '-apple-system',
      fallbacks: 'BlinkMacSystemFont, "Segoe UI", sans-serif',
      weight: 'semibold',
    },
    code: {
      family: 'ui-monospace',
      fallbacks: '"SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
    },
  },

  // Product motion: short and state-driven. Reduced motion is honoured by
  // every Astryx component.
  motion: {fast: 160, medium: 200, slow: 600, ratio: 0.8},

  tokens: {
    // Surfaces
    '--color-background-body': [zinc[50], zinc[950]],
    '--color-background-surface': [white, zinc[900]],
    '--color-background-card': [white, zinc[900]],
    '--color-background-popover': [white, zinc[900]],
    '--color-background-muted': [zinc[100], zinc[800]],
    '--color-background-inverted': [zinc[900], zinc[50]],
    '--color-neutral': [alpha(zinc[900], 0.06), alpha(zinc[50], 0.1)],
    '--color-overlay': [alpha(zinc[950], 0.45), alpha(zinc[950], 0.72)],
    '--color-overlay-hover': [alpha(zinc[950], 0.05), alpha(white, 0.06)],
    '--color-overlay-pressed': [alpha(zinc[950], 0.1), alpha(white, 0.12)],

    // Action and focus: Tailwind blue-500 with zinc-950 text in both themes.
    '--color-accent': blue[500],
    '--color-accent-muted': [alpha(blue[500], 0.16), alpha(blue[500], 0.26)],
    '--color-on-accent': zinc[950],
    // Accent used as text (links) needs AA contrast on its surface, so it
    // steps one shade toward the surface's opposite.
    '--color-text-accent': [blue[600], blue[400]],
    '--color-icon-accent': [blue[600], blue[400]],

    // Text
    '--color-text-primary': [zinc[900], zinc[50]],
    '--color-text-secondary': [zinc[600], zinc[400]],
    '--color-text-disabled': [zinc[400], zinc[600]],
    '--color-on-dark': zinc[50],
    '--color-on-light': zinc[950],
    '--color-icon-primary': [zinc[900], zinc[50]],
    '--color-icon-secondary': [zinc[600], zinc[400]],
    '--color-icon-disabled': [zinc[400], zinc[600]],

    // Status (always paired with words in the UI). Error text steps to red-700
    // in light mode so it keeps AA contrast on the muted error fill.
    '--color-success': [emerald[700], emerald[400]],
    '--color-success-muted': [alpha(emerald[500], 0.16), alpha(emerald[500], 0.22)],
    '--color-on-success': zinc[950],
    '--color-warning': [amber[700], amber[400]],
    '--color-warning-muted': [alpha(amber[500], 0.18), alpha(amber[500], 0.22)],
    '--color-on-warning': zinc[950],
    '--color-error': [red[700], red[400]],
    '--color-error-muted': [alpha(red[500], 0.14), alpha(red[500], 0.22)],
    '--color-on-error': [white, zinc[950]],

    // Lines
    '--color-border': [zinc[200], zinc[800]],
    '--color-border-emphasized': [zinc[300], zinc[700]],
    '--color-skeleton': [zinc[200], zinc[800]],
    '--color-track': [zinc[100], zinc[800]],
    '--color-shadow': [alpha(zinc[950], 0.1), alpha(zinc[950], 0.5)],

    // Shapes: controls 8px, panels 16px, tags 6px.
    '--radius-inner': '6px',
    '--radius-element': '8px',
    '--radius-container': '16px',
    '--radius-page': '16px',

    // Controls: 44px targets.
    '--size-element-sm': '36px',
    '--size-element-md': '44px',
    '--size-element-lg': '48px',

    // Focus: a 2px accent ring with clear offset.
    '--focus-outline-width': '2px',
    '--focus-outline-style': 'solid',
    '--focus-outline-color': blue[500],
    '--focus-outline-offset': '3px',

    // Type ramp from DESIGN.md
    '--font-family-body': systemSans,
    '--font-family-heading': systemSans,
    '--text-heading-1-size': '1.875rem',
    '--text-heading-1-weight': '600',
    '--text-heading-1-leading': '1.2',
    '--text-heading-2-size': '1.25rem',
    '--text-heading-2-weight': '600',
    '--text-heading-2-leading': '1.4',
    '--text-heading-3-size': '1rem',
    '--text-heading-3-weight': '600',
    '--text-heading-3-leading': '1.5',
    '--text-heading-4-size': '0.875rem',
    '--text-heading-4-weight': '600',
    '--text-heading-4-leading': '1.4286',
    '--text-body-size': '1rem',
    '--text-body-leading': '1.5',
    '--text-large-size': '1.25rem',
    '--text-large-leading': '1.4',
    '--text-label-size': '0.875rem',
    '--text-label-weight': '550',
    '--text-label-leading': '1.4286',
    '--text-supporting-size': '0.8125rem',
    '--text-supporting-leading': '1.5',
    '--text-display-3-size': '1.875rem',
    '--text-display-3-weight': '600',
    '--text-display-3-leading': '1.2',

    // Flat workspace: only overlays (dialogs, menus, toasts) keep elevation.
    '--shadow-low': '0 1px 2px light-dark(oklch(0% 0 0 / 6%), oklch(0% 0 0 / 40%))',
  },

  components: {
    heading: {
      'level:1': {letterSpacing: '-0.025em'},
      'level:2': {letterSpacing: '-0.025em'},
    },

    // Secondary buttons: surface fill with a thin divider border (Popcorn SR).
    button: {
      base: {fontWeight: '600'},
      'variant:secondary': {
        backgroundColor: 'var(--color-background-surface)',
        borderWidth: '1px',
        borderStyle: 'solid',
        borderColor: 'var(--color-border)',
      },
      'variant:ghost': {color: 'var(--color-text-secondary)'},
    },

    // Navigation stays neutral: muted labels, ink + subtle fill when current.
    'side-nav-item': {
      base: {
        color: 'var(--color-text-secondary)',
        fontWeight: '500',
      },
      selected: {
        color: 'var(--color-text-primary)',
        fontWeight: '500',
      },
    },

    // Shell: the page canvas behind content, the surface on the sidebar and
    // compact top bar (AppShell paints those itself in auto height).
    'app-shell': {
      'variant:section': {backgroundColor: 'var(--color-background-body)'},
    },

    // Tabs are navigation: the selected marker is ink, not the action blue.
    'tab-indicator': {
      selected: {backgroundColor: 'var(--color-text-primary)'},
    },

    // Cards are flat panels: surface, thin border, panel radius, no shadow.
    card: {
      base: {
        borderRadius: 'var(--radius-container)',
        boxShadow: 'none',
      },
    },

    badge: {
      base: {borderRadius: 'var(--popcorn-radius-tag)'},
    },

    // Disclosure rows ("Use a WireGuard or OpenVPN file instead") are targets
    // too: the whole row keeps the 44px control height, not just its text line.
    'collapsible-trigger': {
      base: {minHeight: 'var(--size-element-md)'},
    },

    'status-dot': {
      'variant:neutral': {backgroundColor: 'var(--popcorn-status-neutral)'},
    },

    'progress-bar-track': {
      base: {borderRadius: 'var(--popcorn-radius-progress)'},
    },
    'progress-bar-fill': {
      base: {borderRadius: 'var(--popcorn-radius-progress)'},
    },
  },

  adaptations: {
    // Popcorn SR's compact-navigation breakpoint is 800px; AppShell's mobile
    // switch resolves through `md`.
    widthBreakpoints: {sm: 640, md: 800, lg: 1024, xl: 1280, '2xl': 1536},
    rules: [
      {
        // Touch: every control, even "small" ones, keeps a 44px target.
        when: {pointer: 'coarse'},
        value: {tokens: {'--size-element-sm': '44px'}},
      },
    ],
  },
});
