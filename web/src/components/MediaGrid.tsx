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
    gridTemplateColumns: 'repeat(auto-fill, minmax(min(100%, 240px), 1fr))',
    columnGap: 20,
    rowGap: 28,
  },
  square: {
    gridTemplateColumns: 'repeat(auto-fill, minmax(min(100%, 112px), 1fr))',
    gap: 4,
  },
  item: {
    minWidth: 0,
  },
});

export interface MediaGridProps {
  shape: MediaShape;
  /** Accessible name for the list, for example "Movies". */
  label: string;
  /** MediaTile elements (each needs a `key`). */
  children: ReactNode;
}

export function MediaGrid({shape, label, children}: MediaGridProps) {
  return (
    <ul aria-label={label} {...stylex.props(styles.grid, styles[shape])}>
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
