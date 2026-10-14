/**
 * A titled horizontal row of tiles (Home: continue watching, one shelf per
 * library). The row scrolls sideways with snap points; on wide screens with
 * a mouse, quiet arrow buttons page through it. Keyboard users tab through
 * the tiles and the row follows focus. A library shelf leads with the
 * library's own icon; one with nothing yet keeps its heading and says so.
 */
import * as stylex from '@stylexjs/stylex';
import {Heading} from '@astryxdesign/core/Heading';
import {IconButton} from '@astryxdesign/core/IconButton';
import {Button} from '@astryxdesign/core/Button';
import {Skeleton} from '@astryxdesign/core/Skeleton';
import {Text} from '@astryxdesign/core/Text';
import {colorVars, typeScaleVars} from '@astryxdesign/core/theme/tokens.stylex';
import {ChevronLeft, ChevronRight, type LucideIcon} from 'lucide-react';
import {Children, isValidElement, useCallback, useEffect, useId, useState, type ReactNode} from 'react';
import {MediaTileSkeleton, type MediaShape} from '../MediaTile';

const styles = stylex.create({
  // Every shelf heading is Section type (the theme's third-level heading), whatever its
  // place in the outline: on Home the shelves are h2s (DESIGN.md, Library pages, On Home).
  sectionType: {
    fontSize: typeScaleVars['--text-heading-3-size'],
    fontWeight: typeScaleVars['--text-heading-3-weight'],
    lineHeight: typeScaleVars['--text-heading-3-leading'],
  },
  section: {
    display: 'flex',
    flexDirection: 'column',
    gap: 14,
    minWidth: 0,
  },
  header: {
    display: 'flex',
    alignItems: 'baseline',
    justifyContent: 'space-between',
    gap: 16,
    minWidth: 0,
  },
  titleGroup: {
    display: 'flex',
    alignItems: 'baseline',
    flexWrap: 'wrap',
    columnGap: 10,
    rowGap: 2,
    minWidth: 0,
  },
  // Icon, name and count share one baseline row; the icon sits on the name's line.
  iconTitle: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 8,
    minWidth: 0,
  },
  titleIcon: {
    flexShrink: 0,
    color: colorVars['--color-icon-secondary'],
    alignSelf: 'center',
  },
  nothing: {
    margin: 0,
    minHeight: 44,
    display: 'flex',
    alignItems: 'center',
  },
  trailing: {
    display: 'flex',
    alignItems: 'center',
    gap: 4,
    flexShrink: 0,
    // Optical alignment: the ghost button's label lines up with the row's edge.
    marginInlineEnd: -10,
  },
  arrows: {
    display: {default: 'none', '@media (hover: hover) and (pointer: fine) and (min-width: 800px)': 'flex'},
    gap: 4,
  },
  row: {
    display: 'grid',
    gridAutoFlow: 'column',
    gap: {default: 20, '@media (max-width: 520px)': 14},
    margin: 0,
    padding: 0,
    listStyle: 'none',
    overflowX: 'auto',
    overflowY: 'hidden',
    overscrollBehaviorX: 'contain',
    scrollSnapType: 'x proximity',
    scrollbarWidth: 'none',
    // Under 800px the row runs to the screen edge and keeps the page gutter.
    marginInline: {default: 0, '@media (max-width: 799px)': -20},
    paddingInline: {default: 0, '@media (max-width: 799px)': 20},
    scrollPaddingInline: {default: 0, '@media (max-width: 799px)': 20},
    // Room for the focus ring of the first and last tiles.
    paddingBlock: 4,
    marginBlock: -4,
    scrollBehavior: {default: 'smooth', '@media (prefers-reduced-motion: reduce)': 'auto'},
  },
  poster: {
    gridAutoColumns: {default: 168, '@media (max-width: 520px)': 132},
  },
  frame: {
    gridAutoColumns: {default: 288, '@media (max-width: 520px)': 232},
  },
  square: {
    gridAutoColumns: {default: 148, '@media (max-width: 520px)': 112},
  },
  // A short group (one or two tiles) sits beside other short groups instead
  // of taking a whole row; it never scrolls. On phones each group takes the
  // full width, so its "See all" lines up with the page edge like the others.
  compactSection: {
    flex: {default: '0 1 auto', '@media (max-width: 799px)': '1 1 100%'},
  },
  compactRow: {
    overflowX: 'visible',
    marginInline: 0,
    paddingInline: 0,
    scrollPaddingInline: 0,
  },
  cell: {
    scrollSnapAlign: 'start',
    minWidth: 0,
  },
});

export interface ShelfProps {
  title: string;
  /** A library's icon, drawn before its name. */
  icon?: LucideIcon;
  /** Nothing to show yet: the heading stays, with this sentence in place of the row. */
  emptyText?: string;
  /** Quiet text after the title ("3 new"). */
  note?: string;
  /**
   * The heading's place in the page outline. It always looks like a Section heading
   * (DESIGN.md, Library pages, On Home), whatever its level.
   */
  headingLevel?: 2 | 3;
  /** "See all" destination. */
  href?: string;
  linkLabel?: string;
  shape: MediaShape;
  /** One or two tiles: no scrolling, sized to its content. */
  isCompact?: boolean;
  children?: ReactNode;
}

export function Shelf({
  title,
  icon: Icon,
  emptyText,
  note,
  headingLevel = 3,
  href,
  linkLabel = 'See all',
  shape,
  isCompact = false,
  children,
}: ShelfProps) {
  const headingId = useId();
  const [row, setRow] = useState<HTMLUListElement | null>(null);
  const [edges, setEdges] = useState({start: true, end: true});

  const measure = useCallback(() => {
    if (!row) return;
    const max = row.scrollWidth - row.clientWidth;
    setEdges({start: row.scrollLeft <= 2, end: row.scrollLeft >= max - 2});
  }, [row]);

  useEffect(() => {
    if (!row) return;
    const observer = new ResizeObserver(() => measure());
    observer.observe(row);
    row.addEventListener('scroll', measure, {passive: true});
    return () => {
      observer.disconnect();
      row.removeEventListener('scroll', measure);
    };
  }, [row, measure]);

  const page = (direction: 1 | -1) => {
    if (!row) return;
    row.scrollBy({left: direction * row.clientWidth * 0.85});
  };

  const hasOverflow = !(edges.start && edges.end);

  return (
    <section {...stylex.props(styles.section, isCompact && styles.compactSection)} aria-labelledby={headingId}>
      <div {...stylex.props(styles.header)}>
        <div {...stylex.props(styles.titleGroup)}>
          {Icon ? (
            <span {...stylex.props(styles.iconTitle)}>
              <Icon size={18} strokeWidth={1.5} aria-hidden="true" {...stylex.props(styles.titleIcon)} />
              <Heading level={headingLevel} id={headingId} xstyle={styles.sectionType}>
                {title}
              </Heading>
            </span>
          ) : (
            <Heading level={headingLevel} id={headingId} xstyle={styles.sectionType}>
              {title}
            </Heading>
          )}
          {note ? (
            <Text type="supporting" color="secondary">
              {note}
            </Text>
          ) : null}
        </div>
        <div {...stylex.props(styles.trailing)}>
          {hasOverflow && !isCompact ? (
            <div {...stylex.props(styles.arrows)}>
              <IconButton
                label={`Scroll ${title} back`}
                icon={<ChevronLeft size={18} strokeWidth={1.75} aria-hidden="true" />}
                variant="ghost"
                size="sm"
                isDisabled={edges.start}
                onClick={() => page(-1)}
              />
              <IconButton
                label={`Scroll ${title} forward`}
                icon={<ChevronRight size={18} strokeWidth={1.75} aria-hidden="true" />}
                variant="ghost"
                size="sm"
                isDisabled={edges.end}
                onClick={() => page(1)}
              />
            </div>
          ) : null}
          {href ? <Button label={linkLabel} variant="ghost" href={href} /> : null}
        </div>
      </div>
      {emptyText ? (
        <Text as="p" type="supporting" color="secondary" xstyle={styles.nothing}>
          {emptyText}
        </Text>
      ) : (
        <ul ref={setRow} {...stylex.props(styles.row, styles[shape], isCompact && styles.compactRow)}>
          {Children.map(children, child =>
            isValidElement(child) ? <li {...stylex.props(styles.cell)}>{child}</li> : null,
          )}
        </ul>
      )}
    </section>
  );
}

export function ShelfSkeleton({shape, count = 6}: {shape: MediaShape; count?: number}) {
  return (
    <div {...stylex.props(styles.section)} aria-hidden="true">
      <Skeleton width={180} height={20} radius={1} />
      <div {...stylex.props(styles.row, styles[shape])}>
        {Array.from({length: count}, (_, index) => (
          <MediaTileSkeleton key={index} shape={shape} index={index} isTextHidden={shape === 'square'} />
        ))}
      </div>
    </div>
  );
}
