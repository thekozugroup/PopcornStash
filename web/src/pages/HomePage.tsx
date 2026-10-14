/**
 * Home (`GET /api/home`): anything that needs a look (Inbox review,
 * downloads in progress), Continue watching, then one shelf per library the
 * person can see, in the owner's order, each led by the library's own icon
 * and holding its newest items. A series library shows one tile per show
 * (up to 12, the most recently added first), pictured by the show's poster.
 * A library with nothing yet keeps its heading, so the order stays readable.
 * Short shelves sit side by side.
 *
 * An empty stash gets the first-run panel; a person who sees no library at
 * all is told so (and the owner is offered "Make a library").
 */
import * as stylex from '@stylexjs/stylex';
import {Button} from '@astryxdesign/core/Button';
import {FileText, Library as LibraryGlyph, type LucideIcon} from 'lucide-react';
import {Fragment, useEffect, useMemo, useState, type ReactNode} from 'react';
import {downloads as downloadsApi, home as homeApi} from '../api/client';
import {useStashEvents} from '../api/events';
import type {DownloadState, HomeLibrary, HomeResponse, Item} from '../api/types';
import {useLibraries} from '../app/LibrariesProvider';
import {useAuth} from '../auth/AuthProvider';
import {EmptyPanel} from '../components/EmptyPanel';
import {FirstRunPanel} from '../components/library/FirstRunPanel';
import {HomeNotices} from '../components/library/HomeNotices';
import {LoadError} from '../components/library/LoadError';
import {placeholderIcon} from '../components/library/placeholderIcon';
import {
  displayTitle,
  itemHref,
  itemMeta,
  posterSources,
  showTileMeta,
  thumbSources,
  timeLeft,
} from '../components/library/libraryFormat';
import {Shelf, ShelfSkeleton} from '../components/library/Shelf';
import {useLibraryEvents} from '../components/library/useLibraryEvents';
import {MediaTile, type MediaShape} from '../components/MediaTile';
import {PageHeader} from '../components/PageHeader';
import {formatCount, formatEpisodeCode, watchedFraction} from '../lib/format';
import {layoutShape, libraryCountLine, libraryHref, libraryShowHref} from '../lib/libraries';
import {libraryIcon} from '../lib/libraryIcons';

const styles = stylex.create({
  stack: {
    display: 'flex',
    flexDirection: 'column',
    gap: 48,
  },
  smallShelves: {
    display: 'flex',
    flexWrap: 'wrap',
    columnGap: 56,
    rowGap: 48,
  },
});

/** Shelves this short share a row with their neighbours instead of taking one each. */
const SMALL_SHELF = 2;

/** Download states that still lead to new files in a library. */
const IN_PROGRESS: ReadonlySet<DownloadState> = new Set(['queued', 'downloading', 'paused', 'completed', 'sorting']);

interface TileModel {
  key: string;
  href: string;
  title: string;
  meta: string;
  image: ReturnType<typeof thumbSources>;
  progress?: number | null;
  /** Placeholder for a tile without a picture. */
  icon?: LucideIcon;
}

interface ShelfModel {
  key: string;
  title: string;
  icon: LucideIcon;
  note: string;
  href: string;
  shape: MediaShape;
  tiles: TileModel[];
}

/**
 * One library's shelf. A series library has one tile per show (the server's `shows`,
 * the most recently added first), pictured by the show's poster in this library, so the
 * shelf agrees with the heading's counts and a show looks the same on Home, on its
 * library page and on its own page.
 */
function shelfFor(entry: HomeLibrary, versionOf: (id: string) => number | undefined): ShelfModel {
  const {library, recent, shows} = entry;
  const icon = libraryIcon(library.icon);
  const shape = layoutShape(library.layout);
  const base = {key: library.id, title: library.name, icon, note: libraryCountLine(library), href: libraryHref(library.id), shape};

  if (library.layout !== 'series') {
    return {
      ...base,
      tiles: recent.map(item => ({
        key: item.id,
        href: itemHref(item),
        title: displayTitle(item),
        meta: itemMeta(item),
        image: thumbSources(item, versionOf(item.id)),
        icon: placeholderIcon(item) ?? icon,
      })),
    };
  }

  return {
    ...base,
    tiles: shows.map(show => ({
      key: `show:${show.show_name}`,
      href: libraryShowHref(library.id, show.show_name),
      title: show.show_name,
      meta: showTileMeta(show),
      // A show without a poster yet: its newest episode's frame stands in.
      image: show.poster_item_id
        ? posterSources(show.poster_item_id, versionOf(show.poster_item_id))
        : thumbSources(show.latest, versionOf(show.latest.id)),
      icon,
    })),
  };
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
      placeholderIcon={tile.icon}
      isTextHidden={shape === 'square'}
    />
  ));
}

function LibraryShelf({shelf, isCompact}: {shelf: ShelfModel; isCompact: boolean}) {
  return (
    <Shelf
      title={shelf.title}
      icon={shelf.icon}
      note={shelf.note}
      href={shelf.href}
      headingLevel={2}
      shape={shelf.shape}
      isCompact={isCompact}
      emptyText={shelf.tiles.length === 0 ? 'Nothing here yet' : undefined}>
      {renderTiles(shelf.tiles, shelf.shape)}
    </Shelf>
  );
}

/**
 * Shelves in the owner's order. A run of short shelves (two tiles or fewer,
 * or nothing yet) shares one wrapping row, so the order still reads left to
 * right, top to bottom.
 */
function ShelfRuns({shelves}: {shelves: ShelfModel[]}) {
  const runs: {small: boolean; shelves: ShelfModel[]}[] = [];
  for (const shelf of shelves) {
    const small = shelf.tiles.length <= SMALL_SHELF;
    const last = runs.at(-1);
    if (last && last.small && small) last.shelves.push(shelf);
    else runs.push({small, shelves: [shelf]});
  }
  return (
    <>
      {runs.map(run =>
        // A short shelf with no short neighbour keeps the full width, so "See all" lines up.
        run.small && run.shelves.length > 1 ? (
          <div key={run.shelves[0]?.key} {...stylex.props(styles.smallShelves)}>
            {run.shelves.map(shelf => (
              <LibraryShelf key={shelf.key} shelf={shelf} isCompact />
            ))}
          </div>
        ) : (
          <Fragment key={run.shelves[0]?.key}>
            <LibraryShelf shelf={run.shelves[0] as ShelfModel} isCompact={false} />
          </Fragment>
        ),
      )}
    </>
  );
}

export default function HomePage() {
  const {isOwner} = useAuth();
  const {libraries} = useLibraries();
  const [nonce, setNonce] = useState(0);
  const [data, setData] = useState<HomeResponse | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [reviewCount, setReviewCount] = useState<number | null>(null);
  const [downloadStates, setDownloadStates] = useState<Record<string, DownloadState>>({});
  const [versions, setVersions] = useState<Record<string, number>>({});

  // A change to the person's libraries (one added, hidden, renamed, reordered)
  // comes through the shared list; Home follows it.
  useEffect(() => {
    const controller = new AbortController();
    homeApi.get(controller.signal).then(
      next => {
        setData(next);
        setError(null);
      },
      (failure: unknown) => {
        if (!controller.signal.aborted) setError(failure);
      },
    );
    return () => controller.abort();
  }, [nonce, libraries]);

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
    download: download =>
      setDownloadStates(states => {
        const next = {...states};
        if ('removed' in download) delete next[download.id];
        else next[download.id] = download.state;
        return next;
      }),
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

  const shelves = useMemo(
    () => (data ? data.libraries.map(entry => shelfFor(entry, id => versions[id])) : []),
    [data, versions],
  );

  const downloadsInProgress = Object.values(downloadStates).filter(state => IN_PROGRESS.has(state)).length;
  const needsReview = reviewCount ?? data?.inbox_count ?? 0;
  const notices = <HomeNotices needsReview={needsReview} downloadsInProgress={downloadsInProgress} />;

  if (!data) {
    return (
      <>
        <PageHeader title="Home" />
        {error ? (
          <LoadError title="Home could not be loaded" error={error} onRetry={() => setNonce(n => n + 1)} />
        ) : (
          <div {...stylex.props(styles.stack)} role="status" aria-busy="true" aria-label="Loading your libraries">
            <ShelfSkeleton shape="frame" count={5} />
            <ShelfSkeleton shape="poster" count={8} />
          </div>
        )}
      </>
    );
  }

  let body: ReactNode;
  if (data.libraries.length === 0) {
    body = isOwner ? (
      <EmptyPanel
        title="No libraries yet"
        description="A library is a folder of videos or photos with a name and an icon. Make one to see it here and in the sidebar."
        icon={<LibraryGlyph size={28} strokeWidth={1.5} />}
        actions={<Button label="Make a library" variant="primary" href="/settings/libraries/new" />}
      />
    ) : (
      <EmptyPanel
        title="Nothing has been shared with you yet"
        description="Libraries appear here once the owner shares them with you."
        icon={<LibraryGlyph size={28} strokeWidth={1.5} />}
      />
    );
  } else if (
    data.libraries.every(entry => entry.library.item_count === 0) &&
    data.continue_watching.length === 0 &&
    data.other_files_count === 0
  ) {
    body = <FirstRunPanel />;
  } else {
    body = (
      <>
        {data.continue_watching.length > 0 ? (
          <Shelf title="Continue watching" headingLevel={2} shape="frame">
            {renderTiles(
              data.continue_watching.map(item => continueTile(item, versions[item.id])),
              'frame',
            )}
          </Shelf>
        ) : null}
        <ShelfRuns shelves={shelves} />
        {data.other_files_count > 0 ? (
          <Shelf
            title="Other files"
            icon={FileText}
            note={formatCount(data.other_files_count, 'file')}
            href="/other"
            headingLevel={2}
            shape="frame"
            emptyText="Music, documents and anything else."
          />
        ) : null}
      </>
    );
  }

  return (
    <>
      <PageHeader title="Home" />
      <div {...stylex.props(styles.stack)}>
        {notices}
        {body}
      </div>
    </>
  );
}
