/**
 * Full-screen photo viewer on a true black stage (Astryx Lightbox). Arrow
 * keys and the on-screen buttons move between photos, a horizontal swipe
 * does the same on touch screens, and the caption carries the date, the
 * camera and a download of the original file. HEIC, RAW and TIFF photos are
 * shown through `/view`, which serves a browser-friendly rendition.
 */
import * as stylex from '@stylexjs/stylex';
import {Button} from '@astryxdesign/core/Button';
import {Lightbox} from '@astryxdesign/core/Lightbox';
import {MediaTheme} from '@astryxdesign/core/theme';
import {Download, Info} from 'lucide-react';
import {useMemo, useRef, type TouchEvent} from 'react';
import {items as itemsApi} from '../../api/client';
import type {Item} from '../../api/types';
import {formatDateTime} from '../../lib/format';
import {fileName, itemHref} from './libraryFormat';

const styles = stylex.create({
  // True black stage in both themes, painted on the dialog itself as well as
  // its backdrop so the counter and caption always sit on black.
  stage: {
    backgroundColor: 'var(--popcorn-stage)',
    '::backdrop': {
      backgroundColor: 'var(--popcorn-stage)',
      backdropFilter: 'none',
    },
  },
  caption: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    gap: 10,
    paddingBottom: 'max(12px, env(safe-area-inset-bottom))',
  },
  facts: {
    display: 'flex',
    flexWrap: 'wrap',
    justifyContent: 'center',
    columnGap: 14,
    rowGap: 2,
    fontSize: 14,
    lineHeight: 1.5,
    fontVariantNumeric: 'tabular-nums',
  },
  muted: {
    opacity: 0.72,
  },
  actions: {
    display: 'flex',
    flexWrap: 'wrap',
    justifyContent: 'center',
    gap: 8,
  },
});

const SWIPE_MIN_PX = 48;

function photoAlt(photo: Item): string {
  const when = photo.taken_at ? formatDateTime(photo.taken_at) : null;
  return when ? `Photo taken ${when}` : `Photo ${photo.title}`;
}

function Caption({photo, hasDetailsLink}: {photo: Item; hasDetailsLink: boolean}) {
  const when = photo.taken_at ? formatDateTime(photo.taken_at) : null;
  return (
    <div {...stylex.props(styles.caption)}>
      <div {...stylex.props(styles.facts)}>
        {when ? <span>{when}</span> : null}
        {photo.camera ? <span {...stylex.props(styles.muted)}>{photo.camera}</span> : null}
      </div>
      <div {...stylex.props(styles.actions)}>
        <Button
          label="Download original"
          variant="secondary"
          href={itemsApi.fileUrl(photo.id, true)}
          icon={<Download size={16} strokeWidth={1.75} aria-hidden="true" />}
          tooltip={fileName(photo.rel_path)}
        />
        {hasDetailsLink ? (
          <Button
            label="Details"
            variant="ghost"
            href={itemHref(photo)}
            icon={<Info size={16} strokeWidth={1.75} aria-hidden="true" />}
          />
        ) : null}
      </div>
    </div>
  );
}

export interface PhotoViewerProps {
  photos: Item[];
  index: number | null;
  onIndexChange: (index: number | null) => void;
  /** Show a link to the photo's own page (off when already on it). */
  hasDetailsLink?: boolean;
  /** Allow zoom (double click, Enter, + and -). Off in galleries so swipes stay simple. */
  hasZoom?: boolean;
}

export function PhotoViewer({photos, index, onIndexChange, hasDetailsLink = true, hasZoom = false}: PhotoViewerProps) {
  const touchStart = useRef<{x: number; y: number} | null>(null);
  const media = useMemo(
    () =>
      photos.map(photo => ({
        src: itemsApi.viewUrl(photo.id),
        alt: photoAlt(photo),
        caption: <Caption photo={photo} hasDetailsLink={hasDetailsLink} />,
      })),
    [photos, hasDetailsLink],
  );

  if (photos.length === 0) return null;
  const current = Math.min(index ?? 0, photos.length - 1);

  const onTouchStart = (event: TouchEvent<HTMLDialogElement>) => {
    const touch = event.touches[0];
    touchStart.current = event.touches.length === 1 && touch ? {x: touch.clientX, y: touch.clientY} : null;
  };
  const onTouchEnd = (event: TouchEvent<HTMLDialogElement>) => {
    const start = touchStart.current;
    touchStart.current = null;
    const touch = event.changedTouches[0];
    if (!start || !touch || photos.length < 2) return;
    const dx = touch.clientX - start.x;
    const dy = touch.clientY - start.y;
    if (Math.abs(dx) < SWIPE_MIN_PX || Math.abs(dx) < Math.abs(dy) * 1.5) return;
    const next = dx < 0 ? current + 1 : current - 1;
    if (next >= 0 && next < photos.length) onIndexChange(next);
  };

  return (
    <MediaTheme mode="dark">
      <Lightbox
        isOpen={index !== null}
        onOpenChange={open => {
          if (!open) onIndexChange(null);
        }}
        media={media.length === 1 ? media[0]! : media}
        index={current}
        onIndexChange={onIndexChange}
        hasZoom={hasZoom}
        onTouchStart={onTouchStart}
        onTouchEnd={onTouchEnd}
        xstyle={styles.stage}
      />
    </MediaTheme>
  );
}
