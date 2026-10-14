/**
 * A photos library: a month-by-month timeline of square tiles, newest first,
 * loaded a page at a time as you scroll. A tile opens the photo viewer in
 * place; the tile is still a real link, so it also opens in a new tab. The
 * page header belongs to the library page.
 */
import * as stylex from '@stylexjs/stylex';
import {Button} from '@astryxdesign/core/Button';
import {Heading} from '@astryxdesign/core/Heading';
import {Skeleton} from '@astryxdesign/core/Skeleton';
import {Text} from '@astryxdesign/core/Text';
import type {LucideIcon} from 'lucide-react';
import {useCallback, useEffect, useMemo, useRef, useState, type ReactNode} from 'react';
import {items as itemsApi, libraries as librariesApi} from '../../api/client';
import type {PhotoMonth} from '../../api/types';
import {formatCount, formatMonth, formatTakenDate} from '../../lib/format';
import {MediaGrid, MediaGridSkeleton} from '../MediaGrid';
import {MediaTile} from '../MediaTile';
import {LoadError} from './LoadError';
import {itemHref, thumbSources} from './libraryFormat';
import {PhotoViewer} from './PhotoViewer';
import {useInfiniteScroll} from './useInfiniteScroll';
import {useLibraryEvents} from './useLibraryEvents';
import {pickItem} from './usePagedItems';

const styles = stylex.create({
  timeline: {
    display: 'flex',
    flexDirection: 'column',
    gap: 40,
  },
  month: {
    display: 'flex',
    flexDirection: 'column',
    gap: 14,
  },
  monthHeader: {
    display: 'flex',
    alignItems: 'baseline',
    flexWrap: 'wrap',
    columnGap: 10,
  },
  more: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    gap: 12,
    marginTop: 8,
  },
  sentinel: {
    height: 1,
  },
});

const PAGE_SIZE = 120;

interface TimelineState {
  nonce: number;
  months: PhotoMonth[];
  cursor: string | null;
  error: unknown;
  loaded: boolean;
  isLoadingMore: boolean;
  loadMoreError: unknown;
  pages: number;
}

/** Appends a page; a month split across two pages becomes one month. */
function mergeMonths(existing: PhotoMonth[], incoming: PhotoMonth[]): PhotoMonth[] {
  const result = existing.map(month => ({...month, items: [...month.items]}));
  for (const month of incoming) {
    const last = result.at(-1);
    if (last && last.month === month.month) {
      const seen = new Set(last.items.map(item => item.id));
      last.items.push(...month.items.filter(item => !seen.has(item.id)));
    } else {
      result.push({...month, items: [...month.items]});
    }
  }
  return result;
}

function MonthSkeleton({index}: {index: number}) {
  return (
    <div {...stylex.props(styles.month)} aria-hidden="true">
      <Skeleton width={140} height={22} radius={1} index={index} />
      <MediaGridSkeleton shape="square" count={index === 0 ? 14 : 9} />
    </div>
  );
}

export interface TimelineViewProps {
  libraryId: string;
  libraryName: string;
  /** The library's icon, for photos without a thumbnail yet. */
  icon: LucideIcon;
  /** What the library shows when it holds no photos. */
  empty: ReactNode;
}

export function TimelineView({libraryId, libraryName, icon, empty}: TimelineViewProps) {
  const [nonce, setNonce] = useState(0);
  const [state, setState] = useState<TimelineState>({
    nonce: -1,
    months: [],
    cursor: null,
    error: null,
    loaded: false,
    isLoadingMore: false,
    loadMoreError: null,
    pages: 0,
  });
  const [versions, setVersions] = useState<Record<string, number>>({});
  const [viewerIndex, setViewerIndex] = useState<number | null>(null);
  const loadingMore = useRef(false);
  const stateRef = useRef(state);

  useEffect(() => {
    stateRef.current = state;
  });

  useEffect(() => {
    const controller = new AbortController();
    loadingMore.current = false;
    librariesApi.timeline(libraryId, {limit: PAGE_SIZE}, controller.signal).then(
      page =>
        setState({
          nonce,
          months: page.months,
          cursor: page.next_cursor,
          error: null,
          loaded: true,
          isLoadingMore: false,
          loadMoreError: null,
          pages: 1,
        }),
      (error: unknown) => {
        if (!controller.signal.aborted) setState(s => ({...s, nonce, error, loaded: s.loaded}));
      },
    );
    return () => controller.abort();
  }, [libraryId, nonce]);

  const loadMore = useCallback(() => {
    const current = stateRef.current;
    if (!current.cursor || loadingMore.current) return;
    loadingMore.current = true;
    setState(s => ({...s, isLoadingMore: true, loadMoreError: null}));
    const requestNonce = current.nonce;
    librariesApi
      .timeline(libraryId, {limit: PAGE_SIZE, cursor: current.cursor})
      .then(
        page =>
          setState(s =>
            s.nonce !== requestNonce
              ? s
              : {...s, months: mergeMonths(s.months, page.months), cursor: page.next_cursor, isLoadingMore: false, pages: s.pages + 1},
          ),
        (error: unknown) => setState(s => (s.nonce !== requestNonce ? s : {...s, isLoadingMore: false, loadMoreError: error})),
      )
      .finally(() => {
        loadingMore.current = false;
      });
  }, [libraryId]);

  useLibraryEvents({
    onChanged: ids => {
      const shown = new Set(stateRef.current.months.flatMap(month => month.items.map(item => item.id)));
      for (const id of ids.filter(candidate => shown.has(candidate)).slice(0, 24)) {
        itemsApi.get(id).then(
          detail => {
            const fresh = pickItem(detail);
            // A photo organized into another library leaves this timeline.
            const stays = fresh.status === 'ready' && fresh.library_id === libraryId;
            setState(s => ({
              ...s,
              months: s.months
                .map(month => {
                  const items = stays
                    ? month.items.map(item => (item.id === id ? fresh : item))
                    : month.items.filter(item => item.id !== id);
                  return {...month, items, count: month.count - (month.items.length - items.length)};
                })
                .filter(month => month.items.length > 0),
            }));
            setVersions(v => ({...v, [id]: Date.now()}));
          },
          () => undefined,
        );
      }
    },
    onRemoved: id =>
      setState(s => ({
        ...s,
        months: s.months
          .map(month => {
            const items = month.items.filter(item => item.id !== id);
            return {...month, items, count: month.count - (month.items.length - items.length)};
          })
          .filter(month => month.items.length > 0),
      })),
    onAdded: () => {
      // New photos land at the top; reload only while the first page is all that is shown.
      if (stateRef.current.pages <= 1 && viewerIndex === null) setNonce(n => n + 1);
    },
    // A file accepted from Inbox as a photo is an update to an item not shown yet.
    onUpdated: ids => {
      const shown = new Set(stateRef.current.months.flatMap(month => month.items.map(item => item.id)));
      if (ids.some(id => !shown.has(id)) && stateRef.current.pages <= 1 && viewerIndex === null) {
        setNonce(n => n + 1);
      }
    },
  });

  const flat = useMemo(() => state.months.flatMap(month => month.items), [state.months]);
  const offsets = useMemo(() => {
    const result = new Map<string, number>();
    let offset = 0;
    for (const month of state.months) {
      result.set(month.month, offset);
      offset += month.items.length;
    }
    return result;
  }, [state.months]);

  const sentinel = useInfiniteScroll(loadMore, {
    enabled: Boolean(state.cursor) && !state.isLoadingMore && !state.loadMoreError,
    resetKey: flat.length,
  });

  const openViewer = (index: number | null) => {
    setViewerIndex(index);
    // Keep the viewer fed: fetch the next page as the end comes near.
    if (index !== null && index >= flat.length - 4) loadMore();
  };

  if (!state.loaded) {
    return (
      <>
        {state.error ? (
          <LoadError title={`${libraryName} could not be loaded`} error={state.error} onRetry={() => setNonce(n => n + 1)} />
        ) : (
          <div {...stylex.props(styles.timeline)} role="status" aria-busy="true" aria-label="Loading photos">
            <MonthSkeleton index={0} />
            <MonthSkeleton index={1} />
          </div>
        )}
      </>
    );
  }

  if (flat.length === 0) return <>{empty}</>;

  return (
    <>
      <div {...stylex.props(styles.timeline)}>
        {state.months.map(month => {
          const offset = offsets.get(month.month) ?? 0;
          const headingId = `month-${month.month}`;
          return (
            <section key={month.month} aria-labelledby={headingId} {...stylex.props(styles.month)}>
              <div {...stylex.props(styles.monthHeader)}>
                <Heading level={2} id={headingId}>
                  {formatMonth(month.month)}
                </Heading>
                <Text type="supporting" color="secondary">
                  {formatCount(month.count, 'photo')}
                </Text>
              </div>
              <MediaGrid shape="square" label={formatMonth(month.month)}>
                {month.items.map((photo, index) => {
                  const thumb = thumbSources(photo, versions[photo.id]);
                  return (
                    <MediaTile
                      key={photo.id}
                      href={itemHref(photo)}
                      title={photo.taken_local ? `Photo from ${formatTakenDate(photo.taken_local)}` : photo.title}
                      meta={photo.camera ?? undefined}
                      imageSrc={thumb?.src}
                      imageSrcSet={thumb?.srcSet}
                      shape="square"
                      placeholderIcon={icon}
                      isTextHidden
                      onActivate={() => openViewer(offset + index)}
                    />
                  );
                })}
              </MediaGrid>
            </section>
          );
        })}
      </div>
      <div ref={sentinel} aria-hidden="true" {...stylex.props(styles.sentinel)} />
      {state.isLoadingMore ? <MonthSkeleton index={2} /> : null}
      {state.loadMoreError ? (
        <div {...stylex.props(styles.more)}>
          <Text as="p" type="supporting" color="secondary">
            Older photos could not be loaded.
          </Text>
          <Button label="Try again" variant="secondary" onClick={loadMore} />
        </div>
      ) : null}
      {state.error && state.loaded ? (
        <LoadError title={`${libraryName} could not be refreshed`} error={state.error} onRetry={() => setNonce(n => n + 1)} />
      ) : null}
      <PhotoViewer photos={flat} index={viewerIndex} onIndexChange={openViewer} />
    </>
  );
}
