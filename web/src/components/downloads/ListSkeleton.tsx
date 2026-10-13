/**
 * Loading rows with the footprint of the real list rows, inside the same
 * flat panel, so nothing jumps when the data arrives.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Skeleton} from '@astryxdesign/core/Skeleton';

const styles = stylex.create({
  row: {
    display: 'grid',
    gridTemplateColumns: 'auto minmax(0, 1fr)',
    columnGap: 16,
    alignItems: 'start',
    paddingBlock: 18,
    paddingInline: {default: 20, '@media (max-width: 559px)': 16},
    borderTopWidth: {default: 1, ':first-child': 0},
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
  lines: {
    display: 'flex',
    flexDirection: 'column',
    gap: 10,
    paddingTop: 2,
  },
});

const sizes = stylex.create({
  thumb: (width: number, height: number) => ({width, height}),
});

export function ListSkeleton({rows = 3, thumb = 'icon', label}: {rows?: number; thumb?: 'icon' | 'frame'; label: string}) {
  const width = thumb === 'icon' ? 40 : 72;
  const height = thumb === 'icon' ? 40 : 48;
  return (
    <div role="status" aria-busy="true" aria-label={label}>
      {Array.from({length: rows}, (_, index) => (
        <div key={index} {...stylex.props(styles.row)} aria-hidden="true">
          <span {...stylex.props(sizes.thumb(width, height))}>
            <Skeleton width={width} height={height} radius={2} index={index} />
          </span>
          <span {...stylex.props(styles.lines)}>
            <Skeleton width={`${62 - index * 9}%`} height={14} radius={1} index={index} />
            <Skeleton width="34%" height={12} radius={1} index={index} />
            {thumb === 'icon' ? <Skeleton width="100%" height={6} radius={1} index={index} /> : null}
          </span>
        </div>
      ))}
    </div>
  );
}
