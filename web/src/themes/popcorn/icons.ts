/**
 * Icon registry for the Popcorn theme.
 *
 * Maps Astryx's semantic icon names to Lucide glyphs (the same family the
 * neutral base theme ships), so every Astryx component draws one consistent
 * stroke icon set. App code that needs a glyph outside this list imports it
 * from `lucide-react` directly and passes it to `<Icon icon={...} />`.
 */
import {createElement, type ComponentType} from 'react';
import type {IconRegistry} from '@astryxdesign/core/Icon';
import {
  AlertTriangle,
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  Calendar,
  Check,
  CheckCheck,
  CheckCircle,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronsLeft,
  ChevronsRight,
  Clock,
  Columns,
  Copy,
  ExternalLink,
  EyeOff,
  Filter,
  Info,
  Menu,
  Mic,
  MoreHorizontal,
  Search,
  Square,
  Upload,
  Wrench,
  X,
  XCircle,
} from 'lucide-react';

const iconProps = {size: '1em', strokeWidth: 1.75, 'aria-hidden': true as const};

// createElement instead of JSX: the theme build loads this module outside the
// app's JSX transform.
const glyph = (Component: ComponentType<typeof iconProps>) =>
  createElement(Component, iconProps);

export const popcornIconRegistry: IconRegistry = {
  close: glyph(X),
  chevronDown: glyph(ChevronDown),
  chevronLeft: glyph(ChevronLeft),
  chevronRight: glyph(ChevronRight),
  chevronsLeft: glyph(ChevronsLeft),
  chevronsRight: glyph(ChevronsRight),
  check: glyph(Check),
  success: glyph(CheckCircle),
  error: glyph(XCircle),
  warning: glyph(AlertTriangle),
  info: glyph(Info),
  calendar: glyph(Calendar),
  clock: glyph(Clock),
  externalLink: glyph(ExternalLink),
  menu: glyph(Menu),
  moreHorizontal: glyph(MoreHorizontal),
  search: glyph(Search),
  upload: glyph(Upload),
  arrowUp: glyph(ArrowUp),
  arrowDown: glyph(ArrowDown),
  arrowsUpDown: glyph(ArrowUpDown),
  funnel: glyph(Filter),
  eyeSlash: glyph(EyeOff),
  viewColumns: glyph(Columns),
  copy: glyph(Copy),
  checkDouble: glyph(CheckCheck),
  wrench: glyph(Wrench),
  stop: glyph(Square),
  microphone: glyph(Mic),
};
