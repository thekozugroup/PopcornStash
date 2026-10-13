/**
 * A cursor-paged list of library items (`GET /api/items`) that stays live:
 * tiles refresh when their thumbnail or details change, removed items drop
 * out, and new arrivals appear at the top when the list is sorted by date
 * added. Previous results stay on screen while a new search loads.
 */
import {useCallback, useEffect, useRef, useState} from 'react';
import {items as itemsApi} from '../../api/client';
import type {Item, ItemCategory, ItemSort, SortOrder} from '../../api/types';
import {useLibraryEvents} from './useLibraryEvents';

export interface PagedItemsQuery {
  category: ItemCategory;
  q?: string;
  sort: ItemSort;
  order: SortOrder;
  favorite?: boolean;
}

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
  const {category, q = '', sort, order, favorite = false} = query;
  const [nonce, setNonce] = useState(0);
  const key = JSON.stringify([category, q, sort, order, favorite, nonce]);
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
      .list({category, q: q || undefined, sort, order, favorite: favorite || undefined, limit: PAGE_SIZE}, controller.signal)
      .then(
        page => setState({...EMPTY, key, items: page.items, cursor: page.next_cursor}),
        (error: unknown) => {
          if (controller.signal.aborted) return;
          setState({...EMPTY, key, error});
        },
      );
    return () => controller.abort();
  }, [key, category, q, sort, order, favorite]);

  const isCurrent = state.key === key;

  const loadMore = useCallback(() => {
    const current = stateRef.current;
    if (current.key !== key || !current.cursor || loadingMore.current) return;
    loadingMore.current = true;
    const requestKey = key;
    setState(s => ({...s, isLoadingMore: true, loadMoreError: null}));
    itemsApi
      .list({
        category,
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
  }, [key, category, q, sort, order, favorite]);

  const retry = useCallback(() => setNonce(n => n + 1), []);

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
                .filter(item => item.status === 'ready' && item.category === category),
            }));
            setVersions(v => ({...v, [id]: Date.now()}));
          },
          () => undefined,
        );
      }
    },
    onRemoved: id => setState(s => ({...s, items: s.items.filter(item => item.id !== id)})),
    onAdded: () => {
      // Only a list sorted newest-first has an obvious place for new arrivals.
      if (sort !== 'added' || order !== 'desc' || q || favorite) return;
      const requestKey = key;
      itemsApi.list({category, sort, order, limit: 20}).then(
        page => setState(s => (s.key !== requestKey ? s : {...s, items: mergeUnique(page.items, s.items)})),
        () => undefined,
      );
    },
  });

  return {
    items: state.items,
    isLoading: !isCurrent && state.key === '',
    isStale: !isCurrent && state.key !== '',
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
  };
}
