/**
 * Episodes of one season as rows: frame, title (its code above a real episode
 * title), runtime and where you are in it. Each row is one link with one
 * focus ring.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars, fontWeightVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Check, Clapperboard} from 'lucide-react';
import {useState} from 'react';
import {Link} from 'react-router';
import type {Item} from '../../api/types';
import {formatDuration, formatEpisodeCode, watchedFraction} from '../../lib/format';
import {episodeName, itemHref, thumbSources, timeLeft} from './libraryFormat';

const styles = stylex.create({
  list: {
    listStyle: 'none',
    margin: 0,
    padding: 0,
    display: 'flex',
    flexDirection: 'column',
  },
  item: {
    borderBottomWidth: 1,
    borderBottomStyle: 'solid',
    borderBottomColor: colorVars['--color-border'],
  },
  row: {
    display: 'grid',
    gridTemplateColumns: {default: '176px minmax(0, 1fr)', '@media (max-width: 520px)': '120px minmax(0, 1fr)'},
    alignItems: 'center',
    columnGap: {default: 20, '@media (max-width: 520px)': 14},
    paddingBlock: 14,
    paddingInline: 8,
    marginInline: -8,
    borderRadius: 12,
    color: colorVars['--color-text-primary'],
    textDecoration: 'none',
    backgroundColor: {default: 'transparent', ':hover': colorVars['--color-overlay-hover']},
    outlineStyle: {default: 'none', ':focus-visible': 'solid'},
    outlineWidth: 2,
    outlineColor: colorVars['--color-accent'],
    outlineOffset: 0,
    transitionProperty: 'background-color',
    transitionDuration: {default: '160ms', '@media (prefers-reduced-motion: reduce)': '0s'},
  },
  frame: {
    position: 'relative',
    display: 'block',
    aspectRatio: '16 / 9',
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
  },
  placeholder: {
    position: 'absolute',
    inset: 0,
    display: 'grid',
    placeItems: 'center',
    color: colorVars['--color-icon-secondary'],
  },
  hairline: {
    position: 'absolute',
    inset: 0,
    borderRadius: 'inherit',
    boxShadow: 'inset 0 0 0 1px var(--popcorn-media-hairline)',
    pointerEvents: 'none',
  },
  track: {
    position: 'absolute',
    insetInline: 0,
    bottom: 0,
    height: 4,
    backgroundColor: 'color-mix(in oklch, var(--popcorn-stage) 45%, transparent)',
  },
  fill: {
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
  code: {
    color: colorVars['--color-text-secondary'],
    fontSize: 13,
    lineHeight: 1.5,
    fontVariantNumeric: 'tabular-nums',
  },
  title: {
    display: '-webkit-box',
    WebkitBoxOrient: 'vertical',
    WebkitLineClamp: 2,
    overflow: 'hidden',
    fontSize: 15,
    lineHeight: 1.4,
    fontWeight: fontWeightVars['--font-weight-medium'],
    overflowWrap: 'anywhere',
  },
  status: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 6,
    color: colorVars['--color-text-secondary'],
    fontSize: 13,
    lineHeight: 1.5,
    fontVariantNumeric: 'tabular-nums',
  },
  next: {
    color: colorVars['--color-text-primary'],
    fontWeight: fontWeightVars['--font-weight-medium'],
  },
});

const fillWidth = stylex.create({
  width: (value: string) => ({width: value}),
});

function EpisodeFrame({episode}: {episode: Item}) {
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  const thumb = thumbSources(episode);
  const watched = watchedFraction(episode.progress);
  const showImage = thumb && failedSrc !== thumb.src;
  return (
    <span {...stylex.props(styles.frame)}>
      {showImage ? (
        <img
          src={thumb.src}
          srcSet={thumb.srcSet}
          sizes="(max-width: 520px) 120px, 176px"
          alt=""
          loading="lazy"
          decoding="async"
          onError={() => setFailedSrc(thumb.src)}
          {...stylex.props(styles.image)}
        />
      ) : (
        <span aria-hidden="true" {...stylex.props(styles.placeholder)}>
          <Clapperboard size={22} strokeWidth={1.5} />
        </span>
      )}
      <span aria-hidden="true" {...stylex.props(styles.hairline)} />
      {watched !== null ? (
        <span aria-hidden="true" {...stylex.props(styles.track)}>
          <span {...stylex.props(styles.fill, fillWidth.width(`${(watched * 100).toFixed(1)}%`))} />
        </span>
      ) : null}
    </span>
  );
}

function episodeStatus(episode: Item, isNext: boolean) {
  if (episode.progress?.finished) {
    return (
      <span {...stylex.props(styles.status)}>
        <Check size={14} strokeWidth={2} aria-hidden="true" />
        Watched
      </span>
    );
  }
  const left = watchedFraction(episode.progress) !== null ? timeLeft(episode) : '';
  const duration = left || formatDuration(episode.duration_s);
  return (
    <span {...stylex.props(styles.status)}>
      {isNext ? (
        <>
          <span {...stylex.props(styles.next)}>Up next</span>
          {duration ? <span aria-hidden="true">·</span> : null}
        </>
      ) : null}
      {duration}
    </span>
  );
}

export function EpisodeList({episodes, nextId, label}: {episodes: Item[]; nextId: string | null; label: string}) {
  return (
    <ol aria-label={label} {...stylex.props(styles.list)}>
      {episodes.map(episode => {
        // The code earns its line only above a real episode title; otherwise the
        // title is "Episode 3" and the season is the tab it sits under.
        const code = episode.episode_title ? formatEpisodeCode(episode.season, episode.episode) : '';
        return (
          <li key={episode.id} {...stylex.props(styles.item)}>
            <Link to={itemHref(episode)} {...stylex.props(styles.row)}>
              <EpisodeFrame episode={episode} />
              <span {...stylex.props(styles.text)}>
                {code ? <span {...stylex.props(styles.code)}>{code}</span> : null}
                <span {...stylex.props(styles.title)}>{episodeName(episode)}</span>
                {episodeStatus(episode, episode.id === nextId)}
              </span>
            </Link>
          </li>
        );
      })}
    </ol>
  );
}
