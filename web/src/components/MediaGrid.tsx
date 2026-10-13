/**
 * Responsive media grid. Columns come from `repeat(auto-fill, minmax(...))`,
 * never fixed counts: posters and frames keep readable widths, photos pack
 * densely with thin gutters.
 */
import * as stylex from '@stylexjs/stylex';
import {Children, isValidElement, type ReactNode} from 'react';
import {MediaTileSkeleton, type MediaShape} from './MediaTile';

const styles = stylex.create({
  grid: {
    display: 'grid',
    margin: 0,
    padding: 0,
    listStyle: 'none',
    minWidth: 0,
  },
  poster: {
    gridTemplateColumns: 'repeat(auto-fill, minmax(min(100%, 148px), 1fr))',
    columnGap: {default: 20, '@media (max-width: 520px)': 14},
    rowGap: 28,
  },
  frame: {
    gridTemplateColumns: {
      default: 'repeat(auto-fill, minmax(min(100%, 240px), 1fr))',
      '@media (max-width: 520px)': 'repeat(auto-fill, minmax(min(100%, 150px), 1fr))',
    },
    columnGap: {default: 20, '@media (max-width: 520px)': 14},
    rowGap: {default: 28, '@media (max-width: 520px)': 22},
  },
  square: {
    gridTemplateColumns: 'repeat(auto-fill, minmax(min(100%, 112px), 1fr))',
    gap: 4,
  },
  item: {
    minWidth: 0,
  },
  // Older results stay visible while a new search or sort loads.
  busy: {
    opacity: 0.55,
    transitionProperty: 'opacity',
    transitionDuration: {default: '160ms', '@media (prefers-reduced-motion: reduce)': '0s'},
  },
});

export interface MediaGridProps {
  shape: MediaShape;
  /** Accessible name for the list, for example "Movies". */
  label: string;
  /** MediaTile elements (each needs a `key`). */
  children: ReactNode;
  /** The tiles are about to be replaced (a new search is loading). */
  isBusy?: boolean;
}

export function MediaGrid({shape, label, children, isBusy = false}: MediaGridProps) {
  return (
    <ul aria-label={label} aria-busy={isBusy || undefined} {...stylex.props(styles.grid, styles[shape], isBusy && styles.busy)}>
      {Children.map(children, child =>
        isValidElement(child) ? (
          <li {...stylex.props(styles.item)}>
            {child}
          </li>
        ) : null,
      )}
    </ul>
  );
}

/** Loading state with the grid's own footprint. */
export function MediaGridSkeleton({shape, count = 12}: {shape: MediaShape; count?: number}) {
  return (
    <div aria-busy="true" aria-label="Loading" role="status" {...stylex.props(styles.grid, styles[shape])}>
      {Array.from({length: count}, (_, index) => (
        <MediaTileSkeleton key={index} shape={shape} index={index} isTextHidden={shape === 'square'} />
      ))}
    </div>
  );
}
