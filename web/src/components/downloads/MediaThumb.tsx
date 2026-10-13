/**
 * Small media thumbnail for list rows (finished downloads, Inbox). The
 * library's own frames and posters carry the colour; a quiet icon tile
 * stands in while a thumbnail is missing or fails to load.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import type {LucideIcon} from 'lucide-react';
import {useState} from 'react';

const styles = stylex.create({
  frame: {
    position: 'relative',
    display: 'grid',
    placeItems: 'center',
    flexShrink: 0,
    overflow: 'hidden',
    borderRadius: 8,
    backgroundColor: 'var(--popcorn-media-placeholder)',
    color: colorVars['--color-icon-secondary'],
  },
  image: {
    position: 'absolute',
    inset: 0,
    width: '100%',
    height: '100%',
    objectFit: 'cover',
    display: 'block',
  },
  hairline: {
    position: 'absolute',
    inset: 0,
    borderRadius: 'inherit',
    // Local value: the theme's --popcorn-media-hairline is solid white in dark mode.
    boxShadow: 'inset 0 0 0 1px light-dark(oklch(14.1% 0.005 285.823 / 0.08), oklch(100% 0 0 / 0.1))',
    pointerEvents: 'none',
  },
});

const sizeStyles = stylex.create({
  box: (width: string, height: string) => ({width, height}),
});

export interface MediaThumbProps {
  src: string | null | undefined;
  icon: LucideIcon;
  /** CSS width, for example "72px" or "clamp(88px, 22vw, 144px)". */
  width: string;
  /** CSS height or aspect ratio driven height. */
  height: string;
  iconSize?: number;
}

export function MediaThumb({src, icon: Icon, width, height, iconSize = 20}: MediaThumbProps) {
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  const showImage = Boolean(src) && failedSrc !== src;
  return (
    <span {...stylex.props(styles.frame, sizeStyles.box(width, height))} aria-hidden="true">
      {showImage ? (
        <>
          <img
            src={src ?? undefined}
            alt=""
            loading="lazy"
            decoding="async"
            onError={() => setFailedSrc(src ?? null)}
            {...stylex.props(styles.image)}
          />
          <span {...stylex.props(styles.hairline)} />
        </>
      ) : (
        <Icon size={iconSize} strokeWidth={1.75} />
      )}
    </span>
  );
}
