/**
 * What a file is, in metadata style: a short summary everyone sees (video,
 * audio, subtitles, size), and the file's location and technical details
 * only when someone opens "Show file details". Family members never see a
 * path unless they ask for it.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Button} from '@astryxdesign/core/Button';
import {MetadataList, MetadataListItem} from '@astryxdesign/core/MetadataList';
import {Text} from '@astryxdesign/core/Text';
import {ChevronDown, ChevronUp} from 'lucide-react';
import {useId, useState} from 'react';
import type {ItemDetail} from '../../api/types';
import {formatBytes, formatDateTime, formatRelative, formatTakenDateTime, formatUtcOffset} from '../../lib/format';
import {
  audioSummary,
  audioTrackDetail,
  audioTrackLabel,
  bitrateName,
  containerName,
  hdrName,
  subtitleLabel,
  subtitleSummary,
  videoCodecName,
  videoSummary,
} from './libraryFormat';

const styles = stylex.create({
  wrap: {
    display: 'flex',
    flexDirection: 'column',
    gap: 16,
    minWidth: 0,
  },
  // Facts read as columns: MetadataList spaces items 16px apart, and this
  // trailing room doubles that between columns without widening wrapped rows.
  fact: {
    paddingInlineEnd: 16,
  },
  toggle: {
    alignSelf: 'flex-start',
    marginInlineStart: -12,
  },
  details: {
    paddingTop: 4,
    paddingBottom: 4,
    borderTopWidth: 1,
    borderTopStyle: 'solid',
    borderTopColor: colorVars['--color-border'],
  },
  // File paths use the metadata style (DESIGN.md) and wrap anywhere.
  path: {
    fontSize: 13,
    lineHeight: 1.5,
    overflowWrap: 'anywhere',
  },
  lines: {
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
  },
});

/**
 * When it was taken, on the camera's clock as written, with the UTC offset the
 * camera wrote beside it when it wrote one: "Jun 14, 2024, 8:18 PM (UTC+01:00)".
 */
function takenLine(item: ItemDetail): string {
  const offset = formatUtcOffset(item.taken_offset);
  const when = formatTakenDateTime(item.taken_local);
  return offset ? `${when} (${offset})` : when;
}

function videoDetail(item: ItemDetail): string | null {
  const video = item.media?.video;
  if (!video) return null;
  const parts = [
    [videoCodecName(video.codec), video.profile].filter(Boolean).join(' '),
    `${video.width} × ${video.height}`,
    video.fps ? `${Math.round(video.fps * 1000) / 1000} fps` : null,
    hdrName(video.hdr),
    video.rotation ? `rotated ${video.rotation}°` : null,
  ];
  return parts.filter(Boolean).join(', ');
}

export function ItemFacts({item}: {item: ItemDetail}) {
  const [isOpen, setIsOpen] = useState(false);
  const detailsId = useId();
  const media = item.media;
  const isPhoto = item.kind === 'photo';

  const summary: {label: string; value: string}[] = [];
  const video = videoSummary(media?.video ?? null);
  if (video) summary.push({label: 'Video', value: video});
  if (isPhoto && item.width && item.height) {
    summary.push({label: 'Dimensions', value: `${item.width} × ${item.height}`});
  }
  const audio = media ? audioSummary(media.audio) : null;
  if (audio) summary.push({label: 'Audio', value: audio});
  const subtitles = media ? subtitleSummary(media.subtitles) : null;
  if (subtitles) summary.push({label: 'Subtitles', value: subtitles});
  if (item.camera) summary.push({label: 'Camera', value: item.camera});
  summary.push({label: 'File size', value: formatBytes(item.size)});

  const tagCamera = media?.tags ? [media.tags.make, media.tags.model].filter(Boolean).join(' ') : '';
  // A document's size alone is already in the line under its title.
  const hasSummary = summary.length > 1 || item.kind !== 'other';

  return (
    <div {...stylex.props(styles.wrap)}>
      {hasSummary ? (
        <MetadataList orientation="horizontal" columns="multi">
          {summary.map(entry => (
            <MetadataListItem key={entry.label} label={entry.label} xstyle={styles.fact}>
              {entry.value}
            </MetadataListItem>
          ))}
        </MetadataList>
      ) : null}

      <Button
        label={isOpen ? 'Hide file details' : 'Show file details'}
        variant="ghost"
        icon={
          isOpen ? (
            <ChevronUp size={16} strokeWidth={1.75} aria-hidden="true" />
          ) : (
            <ChevronDown size={16} strokeWidth={1.75} aria-hidden="true" />
          )
        }
        aria-expanded={isOpen}
        aria-controls={detailsId}
        onClick={() => setIsOpen(open => !open)}
        xstyle={styles.toggle}
      />

      <div id={detailsId} hidden={!isOpen} {...stylex.props(styles.details)}>
        {isOpen ? (
          <MetadataList label={{position: 'start', width: 140}}>
            <MetadataListItem label="Location">
              <span {...stylex.props(styles.lines)}>
                <span {...stylex.props(styles.path)}>{item.rel_path}</span>
                <Text type="supporting" color="secondary">
                  Inside your PopcornStash folder
                </Text>
              </span>
            </MetadataListItem>
            <MetadataListItem label="File type">
              {[containerName(media, item.rel_path), item.mime].filter(Boolean).join(', ')}
            </MetadataListItem>
            <MetadataListItem label="File size">{`${formatBytes(item.size)} (${new Intl.NumberFormat().format(item.size)} bytes)`}</MetadataListItem>
            {videoDetail(item) ? <MetadataListItem label="Video">{videoDetail(item)}</MetadataListItem> : null}
            {isPhoto && item.width && item.height ? (
              <MetadataListItem label="Dimensions">{`${item.width} × ${item.height} pixels`}</MetadataListItem>
            ) : null}
            {media && media.audio.length > 0 ? (
              <MetadataListItem label={media.audio.length > 1 ? 'Audio tracks' : 'Audio'}>
                <span {...stylex.props(styles.lines)}>
                  {media.audio.map((track, index) => (
                    <span key={track.index}>{`${audioTrackLabel(track, index)}, ${audioTrackDetail(track)}`}</span>
                  ))}
                </span>
              </MetadataListItem>
            ) : null}
            {media && media.subtitles.length > 0 ? (
              <MetadataListItem label="Subtitles">
                <span {...stylex.props(styles.lines)}>
                  {media.subtitles.map(track => (
                    <span key={track.index}>{subtitleLabel(track)}</span>
                  ))}
                </span>
              </MetadataListItem>
            ) : null}
            {bitrateName(media?.bitrate) ? <MetadataListItem label="Bit rate">{bitrateName(media?.bitrate)}</MetadataListItem> : null}
            {item.taken_local ? (
              <MetadataListItem label={item.kind === 'photo' ? 'Taken' : 'Recorded'}>{takenLine(item)}</MetadataListItem>
            ) : null}
            {tagCamera && tagCamera !== item.camera ? <MetadataListItem label="Device">{tagCamera}</MetadataListItem> : null}
            <MetadataListItem label="Added">{`${formatDateTime(item.added_at)} (${formatRelative(item.added_at).toLocaleLowerCase()})`}</MetadataListItem>
          </MetadataList>
        ) : null}
      </div>
    </div>
  );
}
