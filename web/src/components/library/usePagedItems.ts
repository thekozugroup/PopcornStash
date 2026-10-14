/**
 * A cursor-paged list of library items (`GET /api/items`) that stays live:
 * tiles refresh when their thumbnail or details change, removed items drop
 * out, and new arrivals appear at the top when the list is sorted by date
 * added. Previous results stay on screen while a new search loads.
 */
import {useCallback, useEffect, useRef, useState} from 'react';
import {items as itemsApi} from '../../api/client';
import type {Item, ItemPlace, ItemSort, SortOrder} from '../../api/types';
import {useLibraryEvents} from './useLibraryEvents';

/** One library's items, or every item in one kind of place (Other files). */
export type PagedItemsScope = {library_id: string; place?: undefined} | {place: ItemPlace; library_id?: undefined};

export type PagedItemsQuery = PagedItemsScope & {
  q?: string;
  sort: ItemSort;
  order: SortOrder;
  favorite?: boolean;
};

interface PagedState {
  key: string;
  items: Item[];
  cursor: string | null;
  error: unknown;
  isLoadingMore: boolean;
  loadMoreError: unknown;
}

const PAGE_SIZE = 60;
const EMPTY: PagedState = {key: '', items: [], cursor: null, error: null, isLoadingMore: false, loadMoreError: null};

function mergeUnique(first: Item[], second: Item[]): Item[] {
  const seen = new Set(first.map(item => item.id));
  return [...first, ...second.filter(item => !seen.has(item.id))];
}

export interface PagedItems {
  items: Item[];
  /** First page of the current query is loading and nothing is shown yet. */
  isLoading: boolean;
  /** Older results are shown while the current query loads. */
  isStale: boolean;
  error: unknown;
  hasMore: boolean;
  isLoadingMore: boolean;
  loadMoreError: unknown;
  loadMore: () => void;
  retry: () => void;
  /** Cache-busting version for an item's thumbnail after a live update. */
  versionOf: (id: string) => number | undefined;
}

export function usePagedItems(query: PagedItemsQuery): PagedItems {
  const {library_id: libraryId, place, q = '', sort, order, favorite = false} = query;
  const [nonce, setNonce] = useState(0);
  const key = JSON.stringify([libraryId, place, q, sort, order, favorite, nonce]);
  /** Whether an item still belongs on this page after a live update. */
  const belongs = (item: Item) =>
    item.status === 'ready' && (libraryId ? item.library_id === libraryId : item.place === place);
  const [state, setState] = useState<PagedState>(EMPTY);
  const [versions, setVersions] = useState<Record<string, number>>({});
  const loadingMore = useRef(false);
  const stateRef = useRef(state);

  useEffect(() => {
    stateRef.current = state;
  });

  useEffect(() => {
    const controller = new AbortController();
    loadingMore.current = false;
    itemsApi
      .list(
        {library_id: libraryId, place, q: q || undefined, sort, order, favorite: favorite || undefined, limit: PAGE_SIZE},
        controller.signal,
      )
      .then(
        page => setState({...EMPTY, key, items: page.items, cursor: page.next_cursor}),
        (error: unknown) => {
          if (controller.signal.aborted) return;
          setState({...EMPTY, key, error});
        },
      );
    return () => controller.abort();
  }, [key, libraryId, place, q, sort, order, favorite]);

  const isCurrent = state.key === key;

  const loadMore = useCallback(() => {
    const current = stateRef.current;
    if (current.key !== key || !current.cursor || loadingMore.current) return;
    loadingMore.current = true;
    const requestKey = key;
    setState(s => ({...s, isLoadingMore: true, loadMoreError: null}));
    itemsApi
      .list({
        library_id: libraryId,
        place,
        q: q || undefined,
        sort,
        order,
        favorite: favorite || undefined,
        limit: PAGE_SIZE,
        cursor: current.cursor,
      })
      .then(
        page =>
          setState(s =>
            s.key !== requestKey
              ? s
              : {...s, items: mergeUnique(s.items, page.items), cursor: page.next_cursor, isLoadingMore: false},
          ),
        (error: unknown) =>
          setState(s => (s.key !== requestKey ? s : {...s, isLoadingMore: false, loadMoreError: error})),
      )
      .finally(() => {
        loadingMore.current = false;
      });
  }, [key, libraryId, place, q, sort, order, favorite]);

  const retry = useCallback(() => setNonce(n => n + 1), []);

  /** Brings in new arrivals at the top of a newest-first list. */
  const mergeNewest = () => {
    // Only a list sorted newest-first (added or filmed) has an obvious place for new arrivals.
    if ((sort !== 'added' && sort !== 'taken') || order !== 'desc' || q || favorite) return;
    const requestKey = key;
    const dateOf = (item: Item) => (sort === 'taken' ? (item.taken_at ?? item.added_at) : item.added_at);
    itemsApi.list({library_id: libraryId, place, sort, order, limit: 20}).then(
      page =>
        setState(s =>
          s.key !== requestKey
            ? s
            : {...s, items: mergeUnique(page.items, s.items).sort((a, b) => dateOf(b) - dateOf(a))},
        ),
      () => undefined,
    );
  };

  useLibraryEvents({
    onChanged: ids => {
      const shown = new Set(stateRef.current.items.map(item => item.id));
      for (const id of ids.filter(candidate => shown.has(candidate)).slice(0, 24)) {
        itemsApi.get(id).then(
          detail => {
            setState(s => ({
              ...s,
              items: s.items
                .map(item => (item.id === id ? {...item, ...pickItem(detail)} : item))
                .filter(belongs),
            }));
            setVersions(v => ({...v, [id]: Date.now()}));
          },
          () => undefined,
        );
      }
    },
    onRemoved: id => setState(s => ({...s, items: s.items.filter(item => item.id !== id)})),
    onAdded: () => mergeNewest(),
    // An item accepted from Inbox or organized into this library arrives as an update
    // to an item this page has never shown.
    onUpdated: ids => {
      const shown = new Set(stateRef.current.items.map(item => item.id));
      if (ids.some(id => !shown.has(id))) mergeNewest();
    },
  });

  return {
    items: state.items,
    // Nothing on screen yet (first load, or a retry after an error): skeleton.
    isLoading: !isCurrent && state.items.length === 0,
    // Older results stay visible, dimmed, while the new query loads.
    isStale: !isCurrent && state.items.length > 0,
    error: isCurrent ? state.error : null,
    hasMore: isCurrent && Boolean(state.cursor),
    isLoadingMore: isCurrent && state.isLoadingMore,
    loadMoreError: isCurrent ? state.loadMoreError : null,
    loadMore,
    retry,
    versionOf: (id: string) => versions[id],
  };
}

/** The list fields of an ItemDetail (drops media, suggestion and links). */
export function pickItem(detail: Item): Item {
  return {
    id: detail.id,
    kind: detail.kind,
    category: detail.category,
    status: detail.status,
    title: detail.title,
    year: detail.year,
    show_name: detail.show_name,
    season: detail.season,
    episode: detail.episode,
    episode_title: detail.episode_title,
    rel_path: detail.rel_path,
    size: detail.size,
    duration_s: detail.duration_s,
    width: detail.width,
    height: detail.height,
    taken_at: detail.taken_at,
    camera: detail.camera,
    thumb: detail.thumb,
    favorite: detail.favorite,
    added_at: detail.added_at,
    progress: detail.progress,
    library_id: detail.library_id,
    place: detail.place,
  };
}
