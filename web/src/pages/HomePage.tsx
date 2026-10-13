/**
 * Home: what is in the library as quiet links, anything that needs a look
 * (Inbox review, downloads in progress), continue watching, and recently
 * added grouped by kind rather than one long wall. An empty library gets a
 * first-run panel that explains the three ways files come in.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars, fontWeightVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Heading} from '@astryxdesign/core/Heading';
import {Skeleton} from '@astryxdesign/core/Skeleton';
import {useEffect, useMemo, useState} from 'react';
import {Link} from 'react-router';
import {downloads as downloadsApi, library} from '../api/client';
import {useStashEvents} from '../api/events';
import type {DownloadState, Item, LibrarySummary} from '../api/types';
import {FirstRunPanel} from '../components/library/FirstRunPanel';
import {HomeNotices} from '../components/library/HomeNotices';
import {LoadError} from '../components/library/LoadError';
import {
  CATEGORY_PATH,
  displayTitle,
  itemHref,
  itemMeta,
  showHref,
  thumbSources,
  timeLeft,
} from '../components/library/libraryFormat';
import {Shelf, ShelfSkeleton} from '../components/library/Shelf';
import {useLibraryEvents} from '../components/library/useLibraryEvents';
import {MediaTile, type MediaShape} from '../components/MediaTile';
import {PageHeader} from '../components/PageHeader';
import {formatCount, formatEpisodeCode, watchedFraction} from '../lib/format';

const styles = stylex.create({
  counts: {
    display: 'flex',
    flexWrap: 'wrap',
    columnGap: 20,
    rowGap: 0,
    margin: 0,
    padding: 0,
    listStyle: 'none',
  },
  countLink: {
    display: 'inline-flex',
    alignItems: 'center',
    minHeight: 44,
    color: colorVars['--color-text-secondary'],
    fontSize: 15,
    textDecoration: 'none',
    textUnderlineOffset: '0.2em',
    textDecorationLine: {default: 'none', ':hover': 'underline'},
    borderRadius: 4,
    outlineStyle: {default: 'none', ':focus-visible': 'solid'},
    outlineWidth: 2,
    outlineColor: colorVars['--color-accent'],
    outlineOffset: 2,
  },
  countNumber: {
    color: colorVars['--color-text-primary'],
    fontWeight: fontWeightVars['--font-weight-semibold'],
    fontVariantNumeric: 'tabular-nums',
    marginInlineEnd: 5,
  },
  stack: {
    display: 'flex',
    flexDirection: 'column',
    gap: 48,
  },
  group: {
    display: 'flex',
    flexDirection: 'column',
    gap: 36,
  },
  smallGroups: {
    display: 'flex',
    flexWrap: 'wrap',
    columnGap: 56,
    rowGap: 36,
  },
  groupHeading: {
    paddingBottom: 12,
    borderBottomWidth: 1,
    borderBottomStyle: 'solid',
    borderBottomColor: colorVars['--color-border'],
  },
  countsSkeleton: {
    display: 'flex',
    gap: 20,
  },
});

/** Groups this small share a row with each other instead of taking one each. */
const SMALL_GROUP = 2;

/** Download states that still lead to new files in the library. */
const IN_PROGRESS: ReadonlySet<DownloadState> = new Set(['queued', 'downloading', 'paused', 'completed', 'sorting']);

interface TileModel {
  key: string;
  href: string;
  title: string;
  meta: string;
  image: ReturnType<typeof thumbSources>;
  progress?: number | null;
}

interface Group {
  key: string;
  title: string;
  note?: string;
  href: string;
  shape: MediaShape;
  newest: number;
  tiles: TileModel[];
}

function itemTile(item: Item, version?: number): TileModel {
  return {
    key: item.id,
    href: itemHref(item),
    title: displayTitle(item),
    meta: itemMeta(item),
    image: thumbSources(item, version),
  };
}

/** Recently added, grouped: episodes collapse into one tile per show. */
function groupRecent(items: Item[], versionOf: (id: string) => number | undefined): Group[] {
  const groups = new Map<string, Group>();
  const ensure = (key: string, init: Omit<Group, 'tiles' | 'newest'>) => {
    let group = groups.get(key);
    if (!group) {
      group = {...init, tiles: [], newest: 0};
      groups.set(key, group);
    }
    return group;
  };
  const showEpisodes = new Map<string, Item[]>();

  for (const item of items) {
    switch (item.category) {
      case 'movie': {
        const group = ensure('movie', {key: 'movie', title: 'Movies', href: CATEGORY_PATH.movie, shape: 'poster'});
        group.tiles.push(itemTile(item, versionOf(item.id)));
        group.newest = Math.max(group.newest, item.added_at);
        break;
      }
      case 'episode': {
        const name = item.show_name ?? item.title;
        showEpisodes.set(name, [...(showEpisodes.get(name) ?? []), item]);
        break;
      }
      case 'home_video': {
        const group = ensure('home_video', {key: 'home_video', title: 'Home Videos', href: CATEGORY_PATH.home_video, shape: 'frame'});
        group.tiles.push(itemTile(item, versionOf(item.id)));
        group.newest = Math.max(group.newest, item.added_at);
        break;
      }
      case 'photo': {
        const group = ensure('photo', {key: 'photo', title: 'Photos', href: CATEGORY_PATH.photo, shape: 'square'});
        group.tiles.push(itemTile(item, versionOf(item.id)));
        group.newest = Math.max(group.newest, item.added_at);
        break;
      }
      default: {
        const group = ensure('other', {key: 'other', title: 'Other', href: CATEGORY_PATH.other, shape: 'frame'});
        group.tiles.push(itemTile(item, versionOf(item.id)));
        group.newest = Math.max(group.newest, item.added_at);
      }
    }
  }

  if (showEpisodes.size > 0) {
    const group = ensure('episode', {key: 'episode', title: 'Shows', href: CATEGORY_PATH.episode, shape: 'poster'});
    for (const [name, episodes] of showEpisodes) {
      const sorted = [...episodes].sort((a, b) => (a.season ?? 0) - (b.season ?? 0) || (a.episode ?? 0) - (b.episode ?? 0));
      const newest = episodes.reduce((latest, episode) => (episode.added_at > latest.added_at ? episode : latest));
      const only = sorted.length === 1 ? sorted[0] : undefined;
      group.tiles.push({
        key: `show:${name}`,
        href: showHref(name),
        title: name,
        meta: only ? `New: ${formatEpisodeCode(only.season, only.episode)}` : formatCount(episodes.length, 'new episode'),
        image: thumbSources(newest, versionOf(newest.id)),
      });
      group.newest = Math.max(group.newest, newest.added_at);
    }
  }

  for (const group of groups.values()) {
    const count = group.key === 'episode' ? [...showEpisodes.values()].reduce((sum, list) => sum + list.length, 0) : group.tiles.length;
    group.note = group.key === 'episode' ? formatCount(count, 'new episode') : `${count} new`;
  }
  return [...groups.values()].sort((a, b) => b.newest - a.newest);
}

function continueTile(item: Item, version?: number): TileModel {
  const left = timeLeft(item);
  const isEpisode = item.category === 'episode';
  const code = isEpisode ? formatEpisodeCode(item.season, item.episode) : '';
  return {
    key: item.id,
    href: itemHref(item),
    title: isEpisode ? (item.show_name ?? displayTitle(item)) : displayTitle(item),
    meta: [code, left].filter(Boolean).join(' · '),
    image: thumbSources(item, version),
    progress: watchedFraction(item.progress),
  };
}

function CountLinks({summary}: {summary: LibrarySummary}) {
  const {counts} = summary;
  const links = [
    {href: '/movies', count: counts.movies, one: 'movie', many: 'movies'},
    {href: '/shows', count: counts.shows, one: 'show', many: 'shows'},
    {href: '/home-videos', count: counts.home_videos, one: 'home video', many: 'home videos'},
    {href: '/photos', count: counts.photos, one: 'photo', many: 'photos'},
    {href: '/other', count: counts.other, one: 'other file', many: 'other files'},
  ].filter(link => link.count > 0);
  if (links.length === 0) return null;
  return (
    <ul aria-label="Your library" {...stylex.props(styles.counts)}>
      {links.map(link => (
        <li key={link.href}>
          <Link to={link.href} {...stylex.props(styles.countLink)}>
            <span {...stylex.props(styles.countNumber)}>{new Intl.NumberFormat().format(link.count)}</span>
            {link.count === 1 ? link.one : link.many}
          </Link>
        </li>
      ))}
    </ul>
  );
}

function renderTiles(tiles: TileModel[], shape: MediaShape) {
  return tiles.map(tile => (
    <MediaTile
      key={tile.key}
      href={tile.href}
      title={tile.title}
      meta={tile.meta}
      imageSrc={tile.image?.src}
      imageSrcSet={tile.image?.srcSet}
      shape={shape}
      progress={tile.progress}
      isTextHidden={shape === 'square'}
    />
  ));
}

export default function HomePage() {
  const [nonce, setNonce] = useState(0);
  const [summary, setSummary] = useState<LibrarySummary | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [reviewCount, setReviewCount] = useState<number | null>(null);
  const [downloadStates, setDownloadStates] = useState<Record<string, DownloadState>>({});
  const [versions, setVersions] = useState<Record<string, number>>({});

  useEffect(() => {
    const controller = new AbortController();
    library.summary(controller.signal).then(
      next => {
        setSummary(next);
        setError(null);
      },
      (failure: unknown) => {
        if (!controller.signal.aborted) setError(failure);
      },
    );
    return () => controller.abort();
  }, [nonce]);

  useEffect(() => {
    const controller = new AbortController();
    downloadsApi.list('active', 100, controller.signal).then(
      result => setDownloadStates(Object.fromEntries(result.items.map(download => [download.id, download.state]))),
      () => undefined,
    );
    return () => controller.abort();
  }, []);

  useStashEvents({
    inbox: event => setReviewCount(event.count),
    download: download => setDownloadStates(states => ({...states, [download.id]: download.state})),
  });

  useLibraryEvents({
    onChanged: ids => {
      const now = Date.now();
      setVersions(v => ({...v, ...Object.fromEntries(ids.map(id => [id, now]))}));
      setNonce(n => n + 1);
    },
    onAdded: () => setNonce(n => n + 1),
    onRemoved: () => setNonce(n => n + 1),
  });

  const versionOf = (id: string) => versions[id];
  const groups = useMemo(
    () => (summary ? groupRecent(summary.recently_added, id => versions[id]) : []),
    [summary, versions],
  );

  const downloadsInProgress = Object.values(downloadStates).filter(state => IN_PROGRESS.has(state)).length;
  const needsReview = reviewCount ?? summary?.needs_review ?? 0;

  if (!summary) {
    return (
      <>
        <PageHeader title="Home">
          <div {...stylex.props(styles.countsSkeleton)} aria-hidden="true">
            <Skeleton width={96} height={18} radius={1} />
            <Skeleton width={84} height={18} radius={1} />
            <Skeleton width={110} height={18} radius={1} />
          </div>
        </PageHeader>
        {error ? (
          <LoadError title="Your library could not be loaded" error={error} onRetry={() => setNonce(n => n + 1)} />
        ) : (
          <div {...stylex.props(styles.stack)} role="status" aria-busy="true" aria-label="Loading your library">
            <ShelfSkeleton shape="frame" count={5} />
            <ShelfSkeleton shape="poster" count={8} />
          </div>
        )}
      </>
    );
  }

  const {counts} = summary;
  const isEmpty =
    counts.movies + counts.episodes + counts.home_videos + counts.photos + counts.other === 0 &&
    summary.recently_added.length === 0;

  if (isEmpty) {
    return (
      <>
        <PageHeader title="Home" subtitle="Your family library. Nothing is here yet." />
        <div {...stylex.props(styles.stack)}>
          <HomeNotices needsReview={needsReview} downloadsInProgress={downloadsInProgress} />
          <FirstRunPanel />
        </div>
      </>
    );
  }

  return (
    <>
      <PageHeader title="Home">
        <CountLinks summary={summary} />
      </PageHeader>
      <div {...stylex.props(styles.stack)}>
        <HomeNotices needsReview={needsReview} downloadsInProgress={downloadsInProgress} />

        {summary.continue_watching.length > 0 ? (
          <Shelf title="Continue watching" headingLevel={2} shape="frame">
            {renderTiles(
              summary.continue_watching.map(item => continueTile(item, versionOf(item.id))),
              'frame',
            )}
          </Shelf>
        ) : null}

        {groups.length > 0 ? (
          <section aria-labelledby="recent-heading" {...stylex.props(styles.group)}>
            <div {...stylex.props(styles.groupHeading)}>
              <Heading level={2} id="recent-heading">
                Recently added
              </Heading>
            </div>
            {groups
              .filter(group => group.tiles.length > SMALL_GROUP)
              .map(group => (
                <Shelf key={group.key} title={group.title} note={group.note} href={group.href} shape={group.shape}>
                  {renderTiles(group.tiles, group.shape)}
                </Shelf>
              ))}
            {groups.some(group => group.tiles.length <= SMALL_GROUP) ? (
              <div {...stylex.props(styles.smallGroups)}>
                {groups
                  .filter(group => group.tiles.length <= SMALL_GROUP)
                  .map(group => (
                    <Shelf
                      key={group.key}
                      title={group.title}
                      note={group.note}
                      href={group.href}
                      shape={group.shape}
                      isCompact>
                      {renderTiles(group.tiles, group.shape)}
                    </Shelf>
                  ))}
              </div>
            ) : null}
          </section>
        ) : null}
      </div>
    </>
  );
}
