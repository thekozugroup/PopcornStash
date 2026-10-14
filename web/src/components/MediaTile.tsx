/**
 * Media tile for posters (2:3), video frames (16:9) and photos (square crop).
 *
 * The whole tile is one link with one focus ring. Below the image: the title
 * (two lines, then truncated) and one metadata line. A thin accent hairline
 * on the image shows partial watch progress. Photos can hide the text and
 * keep the title as the link's accessible name.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars, fontWeightVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Skeleton} from '@astryxdesign/core/Skeleton';
import {Clapperboard, Film, ImageIcon, type LucideIcon} from 'lucide-react';
import {useState, type MouseEvent} from 'react';
import {Link} from 'react-router';
import {formatPercent} from '../lib/format';

export type MediaShape = 'poster' | 'frame' | 'square';

const RATIO: Record<MediaShape, string> = {
  poster: '2 / 3',
  frame: '16 / 9',
  square: '1 / 1',
};

/**
 * Rendered image width per shape for `srcset` selection. Thumbnails are
 * 16:9 video frames, so a 2:3 poster crop draws the image about 2.7 times
 * wider than the tile.
 */
const SIZES: Record<MediaShape, string> = {
  poster: '(max-width: 799px) 120vw, 480px',
  frame: '(max-width: 520px) 100vw, 360px',
  square: '(max-width: 520px) 34vw, 180px',
};

const PLACEHOLDER_ICON: Record<MediaShape, LucideIcon> = {
  poster: Film,
  frame: Clapperboard,
  square: ImageIcon,
};

const styles = stylex.create({
  tile: {
    display: 'flex',
    flexDirection: 'column',
    gap: 10,
    minWidth: 0,
    color: colorVars['--color-text-primary'],
    textDecoration: 'none',
    borderRadius: 'var(--popcorn-radius-media)',
    outline: 'none',
    WebkitTapHighlightColor: 'transparent',
  },
  focusRing: {
    outlineStyle: {default: 'none', ':focus-visible': 'solid'},
    outlineWidth: 2,
    outlineColor: colorVars['--color-accent'],
    outlineOffset: 3,
  },
  frame: {
    position: 'relative',
    display: 'block',
    width: '100%',
    overflow: 'hidden',
    borderRadius: 'var(--popcorn-radius-media)',
    backgroundColor: 'var(--popcorn-media-placeholder)',
  },
  image: {
    position: 'absolute',
    inset: 0,
    width: '100%',
    height: '100%',
    objectFit: 'cover',
    display: 'block',
  },
  placeholder: {
    position: 'absolute',
    inset: 0,
    display: 'grid',
    placeItems: 'center',
    color: colorVars['--color-icon-secondary'],
  },
  // A 1px inner hairline in place of a shadow, so light photos keep an edge
  // on a light page. It also carries the hover tint.
  hairline: {
    position: 'absolute',
    inset: 0,
    borderRadius: 'inherit',
    boxShadow: 'inset 0 0 0 1px var(--popcorn-media-hairline)',
    backgroundColor: {
      default: 'transparent',
      [stylex.when.ancestor(':hover')]: colorVars['--color-overlay-hover'],
    },
    transitionProperty: 'background-color',
    transitionDuration: {default: '160ms', '@media (prefers-reduced-motion: reduce)': '0s'},
    pointerEvents: 'none',
  },
  progressTrack: {
    position: 'absolute',
    insetInline: 0,
    bottom: 0,
    height: 4,
    // The black stage colour, opaque: the Action Blue fill keeps 4:1 against it on any
    // frame (a translucent track let a bright or mid-grey frame show through at 1.0-3.0:1).
    backgroundColor: 'var(--popcorn-stage)',
  },
  progressFill: {
    display: 'block',
    height: '100%',
    backgroundColor: colorVars['--color-accent'],
  },
  text: {
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
    minWidth: 0,
  },
  title: {
    display: '-webkit-box',
    WebkitBoxOrient: 'vertical',
    WebkitLineClamp: 2,
    overflow: 'hidden',
    fontSize: 14,
    lineHeight: 1.4286,
    fontWeight: fontWeightVars['--font-weight-medium'],
    overflowWrap: 'anywhere',
  },
  meta: {
    color: colorVars['--color-text-secondary'],
    fontSize: 13,
    lineHeight: 1.5,
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    fontVariantNumeric: 'tabular-nums',
  },
  visuallyHidden: {
    position: 'absolute',
    width: 1,
    height: 1,
    margin: -1,
    padding: 0,
    overflow: 'hidden',
    clipPath: 'inset(50%)',
    whiteSpace: 'nowrap',
    borderWidth: 0,
  },
  skeletonText: {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
  },
});

const ratioStyles = stylex.create({
  ratio: (ratio: string) => ({aspectRatio: ratio}),
  width: (fraction: string) => ({width: fraction}),
});

export interface MediaTileProps {
  href: string;
  title: string;
  /** One metadata line: year, duration or date. */
  meta?: string;
  /** Thumbnail URL; a quiet placeholder shows while missing or failed. */
  imageSrc?: string | null;
  /** Optional `srcset` (small and large thumbnails) for sharp tiles on dense screens. */
  imageSrcSet?: string | null;
  shape: MediaShape;
  /** Watched fraction 0..1, or null for none. */
  progress?: number | null;
  /** Hide title and metadata (photo grids). The title stays the accessible name. */
  isTextHidden?: boolean;
  /** Icon for the placeholder when there is no image (defaults by shape). */
  placeholderIcon?: LucideIcon;
  /**
   * Plain clicks (no modifier keys) go here instead of navigating, for
   * example to open a photo viewer. The link still works in a new tab.
   */
  onActivate?: () => void;
}

function isPlainClick(event: MouseEvent<HTMLAnchorElement>): boolean {
  return event.button === 0 && !event.metaKey && !event.ctrlKey && !event.shiftKey && !event.altKey;
}

export function MediaTile({
  href,
  title,
  meta,
  imageSrc,
  imageSrcSet,
  shape,
  progress,
  isTextHidden = false,
  placeholderIcon,
  onActivate,
}: MediaTileProps) {
  // Failure is remembered per source, so a refreshed thumbnail gets a new try.
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  const Placeholder = placeholderIcon ?? PLACEHOLDER_ICON[shape];
  const showImage = Boolean(imageSrc) && failedSrc !== imageSrc;
  const watched = progress !== null && progress !== undefined && progress > 0 ? Math.min(1, progress) : null;

  return (
    <Link
      to={href}
      aria-label={isTextHidden ? [title, meta].filter(Boolean).join(', ') : undefined}
      onClick={
        onActivate
          ? event => {
              if (!isPlainClick(event)) return;
              event.preventDefault();
              onActivate();
            }
          : undefined
      }
      {...stylex.props(stylex.defaultMarker(), styles.tile, styles.focusRing)}>
      <span {...stylex.props(styles.frame, ratioStyles.ratio(RATIO[shape]))}>
        {showImage ? (
          <img
            key={imageSrc ?? undefined}
            src={imageSrc ?? undefined}
            srcSet={imageSrcSet ?? undefined}
            sizes={imageSrcSet ? SIZES[shape] : undefined}
            alt=""
            loading="lazy"
            decoding="async"
            onError={() => setFailedSrc(imageSrc ?? null)}
            {...stylex.props(styles.image)}
          />
        ) : (
          <span {...stylex.props(styles.placeholder)} aria-hidden="true">
            <Placeholder size={shape === 'square' ? 22 : 28} strokeWidth={1.5} />
          </span>
        )}
        <span {...stylex.props(styles.hairline)} aria-hidden="true" />
        {watched !== null ? (
          <span aria-hidden="true" {...stylex.props(styles.progressTrack)}>
            <span {...stylex.props(styles.progressFill, ratioStyles.width(`${watched * 100}%`))} />
          </span>
        ) : null}
      </span>
      {isTextHidden ? null : (
        <span {...stylex.props(styles.text)}>
          <span {...stylex.props(styles.title)}>{title}</span>
          {meta ? <span {...stylex.props(styles.meta)}>{meta}</span> : null}
          {/* Read after the title, so the link is announced by its name first. */}
          {watched !== null ? <span {...stylex.props(styles.visuallyHidden)}>{`, ${formatPercent(watched)} watched`}</span> : null}
        </span>
      )}
    </Link>
  );
}

/** Loading placeholder with the same footprint as a tile. */
export function MediaTileSkeleton({shape, index = 0, isTextHidden = false}: {shape: MediaShape; index?: number; isTextHidden?: boolean}) {
  return (
    <span {...stylex.props(styles.tile)} aria-hidden="true">
      <span {...stylex.props(styles.frame, ratioStyles.ratio(RATIO[shape]))}>
        <Skeleton width="100%" height="100%" radius="none" index={index} />
      </span>
      {isTextHidden ? null : (
        <span {...stylex.props(styles.skeletonText)}>
          <Skeleton width="80%" height={14} radius={1} index={index} />
          <Skeleton width="45%" height={12} radius={1} index={index} />
        </span>
      )}
    </span>
  );
}
