/**
 * One show (`/shows/:name`): the show's poster (the server's choice, the same
 * picture as on Shows and Home), how much there is, and one
 * primary action that resumes the next unwatched episode. Seasons sit in
 * tabs; each season lists its episodes with code, title, runtime and
 * progress.
 */
import * as stylex from '@stylexjs/stylex';
import {colorVars} from '@astryxdesign/core/theme/tokens.stylex';
import {Button} from '@astryxdesign/core/Button';
import {Heading} from '@astryxdesign/core/Heading';
import {Skeleton} from '@astryxdesign/core/Skeleton';
import {Tab, TabList} from '@astryxdesign/core/TabList';
import {Text} from '@astryxdesign/core/Text';
import {ChevronLeft, Film, Play, RotateCcw} from 'lucide-react';
import {useEffect, useMemo, useState} from 'react';
import {useNavigate, useParams} from 'react-router';
import {ApiError, shows as showsApi} from '../api/client';
import type {Item, ShowDetail} from '../api/types';
import {EpisodeList} from '../components/library/EpisodeList';
import {LoadError} from '../components/library/LoadError';
import {episodeName, itemHref, largeThumb, resumePosition, showMeta, timeLeft} from '../components/library/libraryFormat';
import {useLibraryEvents} from '../components/library/useLibraryEvents';
import {PageHeader, useDocumentTitle} from '../components/PageHeader';
import {formatEpisodeCode} from '../lib/format';

const styles = stylex.create({
  back: {
    marginInlineStart: -12,
    marginBottom: 16,
  },
  hero: {
    display: 'grid',
    gridTemplateColumns: {default: '200px minmax(0, 1fr)', '@media (max-width: 640px)': '112px minmax(0, 1fr)'},
    alignItems: {default: 'end', '@media (max-width: 640px)': 'start'},
    columnGap: {default: 32, '@media (max-width: 640px)': 18},
    rowGap: 20,
    marginBottom: 40,
  },
  poster: {
    position: 'relative',
    aspectRatio: '2 / 3',
    overflow: 'hidden',
    borderRadius: 'var(--popcorn-radius-media)',
    backgroundColor: 'var(--popcorn-media-placeholder)',
  },
  posterImage: {
    position: 'absolute',
    inset: 0,
    width: '100%',
    height: '100%',
    objectFit: 'cover',
  },
  posterPlaceholder: {
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
  },
  intro: {
    display: 'flex',
    flexDirection: 'column',
    gap: 10,
    minWidth: 0,
  },
  next: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'flex-start',
    gap: 10,
    marginTop: 14,
    gridColumn: {default: 'auto', '@media (max-width: 640px)': '1 / -1'},
  },
  nextDesktop: {
    display: {default: 'flex', '@media (max-width: 640px)': 'none'},
  },
  nextMobile: {
    display: {default: 'none', '@media (max-width: 640px)': 'flex'},
    marginTop: 0,
  },
  upNext: {
    lineHeight: 1.5,
  },
  tabs: {
    marginBottom: 8,
  },
  skeletonText: {
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
  },
});

/** The episode to watch next: the first one not finished, in order. */
function nextEpisode(seasons: ShowDetail['seasons']): {episode: Item; isRewatch: boolean} | null {
  const all = seasons.flatMap(season => season.episodes);
  const first = all[0];
  if (!first) return null;
  const unfinished = all.find(episode => !episode.progress?.finished);
  return unfinished ? {episode: unfinished, isRewatch: false} : {episode: first, isRewatch: true};
}

function seasonLabel(season: number): string {
  return season === 0 ? 'Specials' : `Season ${season}`;
}

interface ShowState {
  name: string;
  nonce: number;
  show: ShowDetail | null;
  error: unknown;
}

function ShowPoster({episode}: {episode: Item | undefined}) {
  const [failed, setFailed] = useState<string | null>(null);
  const src = episode ? largeThumb(episode) : null;
  return (
    <div {...stylex.props(styles.poster)}>
      {src && failed !== src ? (
        <img src={src} alt="" decoding="async" onError={() => setFailed(src)} {...stylex.props(styles.posterImage)} />
      ) : (
        <span aria-hidden="true" {...stylex.props(styles.posterPlaceholder)}>
          <Film size={28} strokeWidth={1.5} />
        </span>
      )}
      <span aria-hidden="true" {...stylex.props(styles.hairline)} />
    </div>
  );
}

export default function ShowPage() {
  const {name = ''} = useParams();
  const navigate = useNavigate();
  const [nonce, setNonce] = useState(0);
  const [state, setState] = useState<ShowState>({name: '', nonce: -1, show: null, error: null});
  const [chosenSeason, setChosenSeason] = useState<{name: string; season: number} | null>(null);
  useDocumentTitle(name || 'Show');

  useEffect(() => {
    const controller = new AbortController();
    showsApi.get(name, controller.signal).then(
      show => setState({name, nonce, show, error: null}),
      (error: unknown) => {
        if (!controller.signal.aborted) setState(s => ({name, nonce, show: s.name === name ? s.show : null, error}));
      },
    );
    return () => controller.abort();
  }, [name, nonce]);

  const show = state.name === name ? state.show : null;
  const error = state.name === name ? state.error : null;

  useLibraryEvents({
    onChanged: ids => {
      const shown = new Set(show?.seasons.flatMap(season => season.episodes.map(episode => episode.id)) ?? []);
      if (ids.some(id => shown.has(id))) setNonce(n => n + 1);
    },
    onAdded: () => setNonce(n => n + 1),
    onRemoved: () => setNonce(n => n + 1),
  });

  const seasons = useMemo(
    () => (show ? [...show.seasons].filter(s => s.episodes.length > 0).sort((a, b) => (a.season || 1e9) - (b.season || 1e9)) : []),
    [show],
  );
  const up = useMemo(() => nextEpisode(seasons), [seasons]);
  const episodeCount = seasons.reduce((sum, season) => sum + season.episodes.length, 0);
  const defaultSeason = up?.episode.season ?? seasons[0]?.season ?? 1;
  const activeSeason =
    chosenSeason?.name === name && seasons.some(s => s.season === chosenSeason.season) ? chosenSeason.season : defaultSeason;
  const current = seasons.find(season => season.season === activeSeason) ?? seasons[0];

  const back = (
    <div {...stylex.props(styles.back)}>
      <Button label="Shows" variant="ghost" href="/shows" icon={<ChevronLeft size={18} strokeWidth={1.75} aria-hidden="true" />} />
    </div>
  );

  if (!show && error) {
    if (error instanceof ApiError && error.code === 'not_found') {
      // One way out: the action already leads back to Shows, so no back link as well.
      return (
        <PageHeader
          title="Show not found"
          subtitle={`There is no show called “${name}” in your library. It may have been renamed or removed.`}
          actions={<Button label="See all shows" variant="primary" href="/shows" />}
          isActionBelow
        />
      );
    }
    return (
      <>
        {back}
        <PageHeader title={name || 'Show'} />
        <LoadError title="This show could not be loaded" error={error} onRetry={() => setNonce(n => n + 1)} />
      </>
    );
  }

  if (!show) {
    return (
      <>
        {back}
        <div {...stylex.props(styles.hero)} aria-busy="true" role="status" aria-label="Loading">
          <div {...stylex.props(styles.poster)}>
            <Skeleton width="100%" height="100%" radius="none" />
          </div>
          <div {...stylex.props(styles.skeletonText)}>
            <Skeleton width="60%" height={30} radius={1} />
            <Skeleton width="35%" height={16} radius={1} />
            <Skeleton width={180} height={44} radius={2} />
          </div>
        </div>
      </>
    );
  }

  const upEpisode = up?.episode;
  // The server's poster for the show; the item carries whether its thumbnail is ready.
  const allEpisodes = seasons.flatMap(season => season.episodes);
  const posterEpisode = allEpisodes.find(episode => episode.id === show.poster_item_id) ?? allEpisodes[0];
  // "Resume" only when the player will pick up part-way (the same rule it uses),
  // not for an episode that was opened for a few seconds and starts over.
  const resumes = upEpisode ? resumePosition(upEpisode) > 0 : false;
  const code = upEpisode ? formatEpisodeCode(upEpisode.season, upEpisode.episode) : '';
  const actionLabel = !upEpisode
    ? ''
    : up?.isRewatch
      ? 'Watch again from the start'
      : resumes
        ? `Resume ${code}`
        : `Play ${code}`;
  const upLine = upEpisode
    ? [episodeName(upEpisode), resumes && !up?.isRewatch ? timeLeft(upEpisode) : null]
        .filter(Boolean)
        .join(' · ')
    : '';

  const nextBlock = (extra: stylex.StyleXStyles) =>
    upEpisode ? (
      <div {...stylex.props(styles.next, extra)}>
        <Button
          label={actionLabel}
          variant="primary"
          icon={
            up?.isRewatch ? (
              <RotateCcw size={16} strokeWidth={2} aria-hidden="true" />
            ) : (
              <Play size={16} strokeWidth={2} aria-hidden="true" />
            )
          }
          onClick={() => navigate(itemHref(upEpisode), {state: {autoplay: true}})}
        />
        <Text as="p" type="supporting" color="secondary" xstyle={styles.upNext}>
          {up?.isRewatch ? 'You have watched every episode.' : upLine}
        </Text>
      </div>
    ) : null;

  return (
    <>
      {back}
      <div {...stylex.props(styles.hero)}>
        <ShowPoster episode={posterEpisode} />
        <div {...stylex.props(styles.intro)}>
          <Heading level={1} textWrap="balance">
            {show.show_name}
          </Heading>
          <Text as="p" color="secondary">
            {showMeta({season_count: seasons.length, item_count: episodeCount})}
          </Text>
          {nextBlock(styles.nextDesktop)}
        </div>
        {nextBlock(styles.nextMobile)}
      </div>

      <section aria-label="Episodes">
        {seasons.length > 1 ? (
          <div {...stylex.props(styles.tabs)}>
            <TabList
              value={String(activeSeason)}
              onChange={value => setChosenSeason({name, season: Number(value)})}
              role="tablist"
              hasDivider>
              {seasons.map(season => (
                <Tab
                  key={season.season}
                  value={String(season.season)}
                  label={seasonLabel(season.season)}
                  panelId={`season-${season.season}`}
                />
              ))}
            </TabList>
          </div>
        ) : (
          <Heading level={2}>{seasonLabel(current?.season ?? 1)}</Heading>
        )}
        {current ? (
          <div
            id={`season-${current.season}`}
            role={seasons.length > 1 ? 'tabpanel' : undefined}
            aria-label={seasons.length > 1 ? seasonLabel(current.season) : undefined}>
            <EpisodeList episodes={current.episodes} nextId={up && !up.isRewatch ? up.episode.id : null} label={seasonLabel(current.season)} />
          </div>
        ) : null}
      </section>
    </>
  );
}
