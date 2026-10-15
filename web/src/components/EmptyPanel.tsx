/**
 * Astryx EmptyState inside a flat Popcorn panel (surface, thin border, 16px
 * radius). Use it for empty pages and sections; the description should teach
 * how the space fills ("Add a link or drop files to start").
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {EmptyState} from '@astryxdesign/core/EmptyState';
import type {ReactNode} from 'react';

const styles = stylex.create({
  panel: {
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: colorVars['--color-border'],
    borderRadius: 'var(--radius-container)',
    backgroundColor: colorVars['--color-background-surface'],
    paddingBlock: {default: 44, '@media (max-width: 520px)': 32},
    paddingInline: {default: 24, '@media (max-width: 520px)': 20},
  },
  plain: {
    paddingBlock: 32,
  },
});

export interface EmptyPanelProps {
  title: string;
  description?: string;
  icon?: ReactNode;
  actions?: ReactNode;
  /** Render without the panel frame (inside an existing panel). */
  isPlain?: boolean;
  headingLevel?: 2 | 3 | 4;
}

export function EmptyPanel({title, description, icon, actions, isPlain = false, headingLevel = 2}: EmptyPanelProps) {
  return (
    <section {...stylex.props(isPlain ? styles.plain : styles.panel)}>
      <EmptyState
        title={title}
        description={description}
        icon={icon}
        actions={actions}
        headingLevel={headingLevel}
      />
    </section>
  );
}
