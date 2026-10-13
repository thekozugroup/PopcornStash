/**
 * Stages for items that are not videos: a photo on the black stage (opens
 * the full-screen viewer with zoom), and a quiet panel for music and other
 * files with the native audio player or a download.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Button} from '@astryxdesign/core/Button';
import {IconButton} from '@astryxdesign/core/IconButton';
import {Text} from '@astryxdesign/core/Text';
import {MediaTheme} from '@astryxdesign/core/theme';
import {Download, FileArchive, FileText, Maximize2, Music} from 'lucide-react';
import {useState} from 'react';
import {items as itemsApi} from '../../api/client';
import type {ItemDetail} from '../../api/types';
import {formatDateTime} from '../../lib/format';
import {fileType} from './libraryFormat';
import {PhotoViewer} from './PhotoViewer';

const styles = stylex.create({
  stage: {
    position: 'relative',
    display: 'grid',
    placeItems: 'center',
    marginInline: {default: 0, '@media (max-width: 799px)': -20},
    paddingBlock: {default: 24, '@media (max-width: 799px)': 0},
    paddingInline: {default: 24, '@media (max-width: 799px)': 0},
    borderRadius: {default: 16, '@media (max-width: 799px)': 0},
    backgroundColor: 'var(--popcorn-stage)',
    colorScheme: 'dark',
    overflow: 'hidden',
  },
  photoButton: {
    display: 'block',
    padding: 0,
    borderWidth: 0,
    backgroundColor: 'transparent',
    cursor: 'zoom-in',
    maxWidth: '100%',
    borderRadius: 4,
    outlineStyle: {default: 'none', ':focus-visible': 'solid'},
    outlineWidth: 2,
    outlineColor: colorVars['--color-accent'],
    outlineOffset: 4,
  },
  photo: {
    display: 'block',
    maxWidth: '100%',
    maxHeight: {default: 'calc(100dvh - 240px)', '@media (max-width: 799px)': '70dvh'},
    minHeight: 120,
    width: 'auto',
    height: 'auto',
    objectFit: 'contain',
  },
  expand: {
    position: 'absolute',
    top: 12,
    insetInlineEnd: 12,
    borderRadius: 8,
    backgroundColor: 'color-mix(in oklch, var(--popcorn-stage) 55%, transparent)',
    color: colorVars['--color-on-dark'],
  },
  panel: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'flex-start',
    gap: 18,
    paddingBlock: 28,
    paddingInline: {default: 28, '@media (max-width: 520px)': 20},
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: colorVars['--color-border'],
    borderRadius: 16,
    backgroundColor: colorVars['--color-background-surface'],
  },
  fileRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 16,
  },
  fileIcon: {
    display: 'grid',
    placeItems: 'center',
    width: 56,
    height: 56,
    borderRadius: 12,
    flexShrink: 0,
    backgroundColor: colorVars['--color-background-muted'],
    color: colorVars['--color-icon-secondary'],
  },
  audioRow: {
    width: '100%',
  },
  audio: {
    flex: '1 1 auto',
    minWidth: 0,
    maxWidth: 640,
  },
});

const sizeStyles = stylex.create({
  ratio: (ratio: string) => ({aspectRatio: ratio}),
});

export function PhotoStage({item}: {item: ItemDetail}) {
  const [index, setIndex] = useState<number | null>(null);
  const ratio = item.width && item.height ? `${item.width} / ${item.height}` : null;
  const alt = item.taken_at ? `Photo taken ${formatDateTime(item.taken_at)}` : item.title;
  return (
    <div {...stylex.props(styles.stage)}>
      <button
        type="button"
        onClick={() => setIndex(0)}
        aria-label={`View ${alt} full screen`}
        {...stylex.props(styles.photoButton)}>
        <img
          src={itemsApi.viewUrl(item.id)}
          alt={alt}
          decoding="async"
          {...stylex.props(styles.photo, ratio ? sizeStyles.ratio(ratio) : null)}
        />
      </button>
      <span {...stylex.props(styles.expand)}>
        <MediaTheme mode="dark">
          <IconButton
            label="View full screen"
            icon={<Maximize2 size={18} strokeWidth={1.75} aria-hidden="true" />}
            variant="ghost"
            onClick={() => setIndex(0)}
          />
        </MediaTheme>
      </span>
      <PhotoViewer photos={[item]} index={index} onIndexChange={setIndex} hasDetailsLink={false} hasZoom />
    </div>
  );
}

const ARCHIVES = new Set(['ZIP', 'RAR', '7Z', 'TAR', 'GZ', 'ISO']);

export function FileStage({item}: {item: ItemDetail}) {
  const type = fileType(item.rel_path);
  const isAudio = item.kind === 'audio';
  const Icon = isAudio ? Music : ARCHIVES.has(type ?? '') ? FileArchive : FileText;
  const icon = (
    <span aria-hidden="true" {...stylex.props(styles.fileIcon)}>
      <Icon size={26} strokeWidth={1.5} />
    </span>
  );
  if (isAudio) {
    return (
      <div {...stylex.props(styles.panel)}>
        <div {...stylex.props(styles.fileRow, styles.audioRow)}>
          {icon}
          <audio
            controls
            preload="metadata"
            src={itemsApi.fileUrl(item.id)}
            aria-label={item.title}
            {...stylex.props(styles.audio)}
          />
        </div>
      </div>
    );
  }
  return (
    <div {...stylex.props(styles.panel)}>
      <div {...stylex.props(styles.fileRow)}>
        {icon}
        <Text as="p" color="secondary">
          {`PopcornStash does not preview ${type ? `${type} files` : 'this kind of file'}. Download it to open it on this device.`}
        </Text>
      </div>
      <Button
        label="Download"
        variant="primary"
        href={itemsApi.fileUrl(item.id, true)}
        icon={<Download size={16} strokeWidth={1.75} aria-hidden="true" />}
      />
    </div>
  );
}
