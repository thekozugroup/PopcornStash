/**
 * Popcorn series brand: a popcorn mark followed by "Popcorn" and a small product
 * name, matching Popcorn SR's sidebar brand.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Link} from 'react-router';
import {Popcorn} from 'lucide-react';

const styles = stylex.create({
  brand: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 10,
    minHeight: 44,
    color: colorVars['--color-text-primary'],
    textDecoration: 'none',
    whiteSpace: 'nowrap',
    borderRadius: 8,
    outlineOffset: 4,
  },
  mark: {
    display: 'block',
    flexShrink: 0,
    width: 32,
    height: 32,
  },
  markLarge: {
    width: 40,
    height: 40,
  },
  word: {
    fontSize: 19,
    fontWeight: 650,
    letterSpacing: '-0.6px',
    lineHeight: 1,
  },
  product: {
    marginInlineStart: 6,
    color: colorVars['--color-text-secondary'],
    fontSize: 12,
    fontWeight: 500,
    letterSpacing: '0.02em',
  },
});

export function BrandMark({size = 'md'}: {size?: 'md' | 'lg'}) {
  return (
    <Popcorn size={32} strokeWidth={1.75} aria-hidden="true" {...stylex.props(styles.mark, size === 'lg' && styles.markLarge)} />
  );
}

/** Brand lockup. Links home unless `href` is null. */
export function Brand({href = '/'}: {href?: string | null}) {
  const content = (
    <>
      <BrandMark />
      <span {...stylex.props(styles.word)}>
        Popcorn<span {...stylex.props(styles.product)}>Stash</span>
      </span>
    </>
  );
  if (href === null) {
    return <span {...stylex.props(styles.brand)}>{content}</span>;
  }
  return (
    <Link to={href} aria-label="PopcornStash home" {...stylex.props(styles.brand)}>
      {content}
    </Link>
  );
}
