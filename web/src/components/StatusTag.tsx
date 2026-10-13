/**
 * A quiet status tag: a small status dot followed by words. Colour is never
 * the only signal; the words carry the meaning and the dot is decorative.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {StatusDot} from '@astryxdesign/core/StatusDot';
import type {ReactNode} from 'react';

export type StatusTone = 'connected' | 'warning' | 'error' | 'neutral';

const styles = stylex.create({
  tag: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 7,
    maxWidth: '100%',
    paddingBlock: 5,
    paddingInline: 9,
    borderRadius: 'var(--popcorn-radius-tag, 6px)',
    backgroundColor: colorVars['--color-background-muted'],
    color: colorVars['--color-text-secondary'],
    fontSize: 12,
    lineHeight: 1.4,
    whiteSpace: 'nowrap',
  },
  wrap: {
    whiteSpace: 'normal',
    alignItems: 'baseline',
  },
  label: {
    minWidth: 0,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
  },
  dot: {
    display: 'inline-flex',
    flexShrink: 0,
  },
  dotWrap: {
    transform: 'translateY(-1px)',
  },
});

const DOT_VARIANT = {
  connected: 'success',
  warning: 'warning',
  error: 'error',
  neutral: 'neutral',
} as const;

export interface StatusTagProps {
  tone: StatusTone;
  children: ReactNode;
  /** Allow the words to wrap onto more lines (a long reason in a narrow column). Otherwise one line, truncated. */
  wrap?: boolean;
  /** Announce changes politely (live status such as the VPN). */
  live?: boolean;
  title?: string;
}

export function StatusTag({tone, children, wrap = false, live = false, title}: StatusTagProps) {
  return (
    <span
      {...stylex.props(styles.tag, wrap && styles.wrap)}
      role={live ? 'status' : undefined}
      title={title}>
      <span aria-hidden="true" {...stylex.props(styles.dot, wrap && styles.dotWrap)}>
        <StatusDot variant={DOT_VARIANT[tone]} label={tone} />
      </span>
      <span {...stylex.props(!wrap && styles.label)}>{children}</span>
    </span>
  );
}
