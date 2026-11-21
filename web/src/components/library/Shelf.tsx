/**
 * A titled horizontal row of tiles (Home: continue watching, one shelf per
 * library). The row scrolls sideways with snap points and offers paging
 * buttons whenever cards extend beyond the row. Keyboard users tab through
 * the tiles and the row follows focus. A library shelf leads with the
 * library's own icon; one with nothing yet keeps its heading and says so.
 */
import * as stylex from '@stylexjs/stylex';
import {Heading} from '@astryxdesign/core/Heading';
import {IconButton} from '@astryxdesign/core/IconButton';
import {Button} from '@astryxdesign/core/Button';
import {Skeleton} from '@astryxdesign/core/Skeleton';
import {Text} from '@astryxdesign/core/Text';
import {colorVars, spacingVars, typeScaleVars} from '@astryxdesign/core/theme/tokens.stylex';
import {ChevronLeft, ChevronRight, type LucideIcon} from 'lucide-react';
import {Children, isValidElement, useId, useLayoutEffect, useState, type KeyboardEvent, type ReactNode} from 'react';
import {MediaTileSkeleton, type MediaShape} from '../MediaTile';
import {adjacentShelfTile} from './adjacentShelfTile';
import {shelfPageOffset} from './shelfPageOffset';

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
    gap: spacingVars['--spacing-3'],
    minWidth: 0,
  },
  header: {
    display: 'flex',
    alignItems: 'center',
    flexWrap: 'wrap',
    columnGap: spacingVars['--spacing-4'],
    rowGap: spacingVars['--spacing-1'],
    minWidth: 0,
  },
  titleGroup: {
    display: 'flex',
    alignItems: 'baseline',
    flexWrap: 'wrap',
    columnGap: 10,
    rowGap: 2,
    minWidth: 0,
    maxWidth: '100%',
    overflowWrap: 'anywhere',
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
    alignSelf: 'flex-start',
    marginBlockStart: spacingVars['--spacing-0-5'],
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
    justifyContent: 'flex-end',
    flexWrap: 'wrap',
    gap: spacingVars['--spacing-2'],
    maxWidth: '100%',
    marginInlineStart: 'auto',
  },
  destination: {
    color: colorVars['--color-text-accent'],
  },
  arrows: {
    display: 'flex',
    gap: spacingVars['--spacing-1'],
  },
  arrowIcon: {
    transform: {default: null, ':dir(rtl)': 'scaleX(-1)'},
  },
  row: {
    // Contain offscreen tiles' absolutely positioned screen-reader text.
    position: 'relative',
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
    // Let the 2px outline and its 3px offset paint outside the content gutter.
    // On a phone the scrollport extends all the way to the edge of the page.
    marginInline: {default: -6, '@media (max-width: 799px)': -20},
    paddingInline: {default: spacingVars['--spacing-1-5'], '@media (max-width: 799px)': spacingVars['--spacing-5']},
    scrollPaddingInline: {default: spacingVars['--spacing-1-5'], '@media (max-width: 799px)': spacingVars['--spacing-5']},
    paddingBlock: spacingVars['--spacing-1-5'],
    marginBlock: -6,
    scrollBehavior: {default: 'auto', '@media (prefers-reduced-motion: no-preference)': 'smooth'},
  },
  poster: {
    gridAutoColumns: {default: 168, '@media (max-width: 520px)': 132},
  },
  frame: {
    gridAutoColumns: 'min(100%, clamp(232px, 24vw, 308px))',
  },
  resume: {
    gridAutoColumns: 'min(100%, clamp(270px, 39vw, 440px))',
  },
  square: {
    gridAutoColumns: {default: 148, '@media (max-width: 520px)': 112},
  },
  // Short shelves can share a line on desktop. Their cards still page when
  // the available width is smaller than the group.
  compactSection: {
    flex: {default: '0 1 auto', '@media (max-width: 799px)': '1 1 100%'},
    maxWidth: '100%',
  },
  compactFrame: {
    // A percentage track cannot give a shrink-to-fit section its natural width.
    gridAutoColumns: 'clamp(232px, 24vw, 308px)',
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
  /** Wider landscape cards for the primary Continue watching shelf. */
  resume?: boolean;
  /** One or two tiles, sized to share a line with other short shelves. */
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
  resume = false,
  children,
}: ShelfProps) {
  const headingId = useId();
  const [row, setRow] = useState<HTMLUListElement | null>(null);
  const [edges, setEdges] = useState({start: true, end: true});

  const count = Children.count(children);
  useLayoutEffect(() => {
    if (!row) return;
    const measure = () => {
      const max = row.scrollWidth - row.clientWidth;
      const offset = Math.abs(row.scrollLeft);
      const start = offset <= 2;
      const end = offset >= max - 2;
      setEdges(previous => previous.start === start && previous.end === end ? previous : {start, end});
    };
    let frame = 0;
    const schedule = () => {
      if (frame) return;
      frame = requestAnimationFrame(() => { frame = 0; measure(); });
    };
    const observer = new ResizeObserver(schedule);
    observer.observe(row);
    // Decide whether the header needs a second line before the cards paint.
    // A feed refresh can also change tile count without resizing the row.
    measure();
    row.addEventListener('scroll', schedule, {passive: true});
    return () => {
      observer.disconnect();
      row.removeEventListener('scroll', schedule);
      cancelAnimationFrame(frame);
    };
  }, [row, count, shape, resume]);

  const moveFocus = (event: KeyboardEvent<HTMLUListElement>) => {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) return;
    const links = Array.from(event.currentTarget.querySelectorAll<HTMLAnchorElement>('li > a'));
    const index = links.indexOf(event.target as HTMLAnchorElement);
    if (index < 0) return;
    if (event.key === 'ArrowUp' || event.key === 'ArrowDown') {
      const nearest = adjacentShelfTile(event.currentTarget, links[index]!, event.key);
      if (nearest) { event.preventDefault(); nearest.focus(); }
      return;
    }
    const forwardKey = getComputedStyle(event.currentTarget).direction === 'rtl' ? 'ArrowLeft' : 'ArrowRight';
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? links.length - 1
      : Math.max(0, Math.min(links.length - 1, index + (event.key === forwardKey ? 1 : -1)));
    event.preventDefault();
    links[next]?.focus();
  };

  const page = (direction: 1 | -1) => {
    if (!row) return;
    const first = row.children[0];
    const second = row.children[1];
    if (!first || !second) return;
    const firstBox = first.getBoundingClientRect();
    const stride = second.getBoundingClientRect().left - firstBox.left;
    if (stride === 0) return;
    const style = getComputedStyle(row);
    const contentWidth = row.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
    const left = shelfPageOffset({scrollLeft: row.scrollLeft, contentWidth, cardWidth: firstBox.width, stride}, direction);
    row.scrollTo({left});
  };

  const hasOverflow = !emptyText && count > 1 && !(edges.start && edges.end);

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
        {href || hasOverflow ? (
          <div {...stylex.props(styles.trailing)}>
            {href ? (
              <Button label={`${linkLabel}, ${title}`} variant="ghost" href={href} xstyle={styles.destination}>
                {linkLabel}
              </Button>
            ) : null}
            {hasOverflow ? (
              <div {...stylex.props(styles.arrows)}>
                <IconButton
                  label={`Scroll ${title} back`}
                  icon={<ChevronLeft size={18} strokeWidth={1.75} aria-hidden="true" {...stylex.props(styles.arrowIcon)} />}
                  variant="ghost"
                  tooltip={edges.start ? 'Beginning of shelf' : 'Previous cards'}
                  isDisabled={edges.start}
                  tabIndex={edges.start ? -1 : undefined}
                  onClick={() => page(-1)}
                />
                <IconButton
                  label={`Scroll ${title} forward`}
                  icon={<ChevronRight size={18} strokeWidth={1.75} aria-hidden="true" {...stylex.props(styles.arrowIcon)} />}
                  variant="ghost"
                  tooltip={edges.end ? 'End of shelf' : 'Next cards'}
                  isDisabled={edges.end}
                  tabIndex={edges.end ? -1 : undefined}
                  onClick={() => page(1)}
                />
              </div>
            ) : null}
          </div>
        ) : null}
      </div>
      {emptyText ? (
        <Text as="p" type="supporting" color="secondary" xstyle={styles.nothing}>
          {emptyText}
        </Text>
      ) : (
        <ul data-media-shelf="" ref={setRow} aria-labelledby={headingId} onKeyDown={moveFocus} {...stylex.props(styles.row, styles[shape], isCompact && shape === 'frame' && styles.compactFrame, resume && styles.resume)}>
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
