/**
 * A short numbered how-to ("Sign in to Nord Account", "Choose ...") in the
 * supporting text style.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Children, type ReactNode} from 'react';

const styles = stylex.create({
  list: {
    margin: 0,
    paddingInlineStart: 22,
    listStyleType: 'decimal',
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
    color: colorVars['--color-text-secondary'],
    fontSize: '0.875rem',
    lineHeight: 1.55,
  },
  item: {
    paddingInlineStart: 4,
  },
});

export function Steps({children, label}: {children: ReactNode; label?: string}) {
  return (
    <ol aria-label={label} {...stylex.props(styles.list)}>
      {Children.map(children, child => (child ? <li {...stylex.props(styles.item)}>{child}</li> : null))}
    </ol>
  );
}
