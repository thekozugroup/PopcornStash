/**
 * Shows: every series as its poster (the server's choice, the same picture
 * Home and the show's page use), most recently updated first. The list is
 * small enough to search and sort in the browser.
 */
import {Button} from '@astryxdesign/core/Button';
import {Tv} from 'lucide-react';
import {useCallback, useEffect, useMemo, useState, type ReactNode} from 'react';
import {useSearchParams} from 'react-router';
import {shows as showsApi} from '../api/client';
import type {ShowSummary} from '../api/types';
import {EmptyPanel} from '../components/EmptyPanel';
import {LibraryToolbar} from '../components/library/LibraryToolbar';
import {LoadError} from '../components/library/LoadError';
import {posterSources, showHref, showMeta} from '../components/library/libraryFormat';
import {useLibraryEvents} from '../components/library/useLibraryEvents';
import {MediaGrid, MediaGridSkeleton} from '../components/MediaGrid';
import {MediaTile} from '../components/MediaTile';
import {PageHeader} from '../components/PageHeader';

type ShowSort = 'added' | 'title';

const SORT_OPTIONS: {value: ShowSort; label: string}[] = [
  {value: 'added', label: 'Recently updated'},
  {value: 'title', label: 'Title'},
];

interface ShowsState {
  shows: ShowSummary[] | null;
  error: unknown;
}

function sortKey(name: string): string {
  return name.toLocaleLowerCase().replace(/^(the|a|an) /, '');
}

export default function ShowsPage() {
  const [params, setParams] = useSearchParams();
  const q = params.get('q') ?? '';
  const sort: ShowSort = params.get('sort') === 'title' ? 'title' : 'added';
  const [nonce, setNonce] = useState(0);
  const [state, setState] = useState<ShowsState>({shows: null, error: null});
  const [version, setVersion] = useState<number | undefined>(undefined);

  useEffect(() => {
    const controller = new AbortController();
    showsApi.list(controller.signal).then(
      shows => setState({shows, error: null}),
      (error: unknown) => {
        if (!controller.signal.aborted) setState(s => ({shows: s.shows, error}));
      },
    );
    return () => controller.abort();
  }, [nonce]);

  useLibraryEvents({
    onAdded: () => setNonce(n => n + 1),
    onRemoved: () => setNonce(n => n + 1),
    onChanged: () => {
      setVersion(Date.now());
      setNonce(n => n + 1);
    },
  });

  const update = useCallback(
    (change: Record<string, string | null>) =>
      setParams(
        current => {
          const next = new URLSearchParams(current);
          for (const [key, value] of Object.entries(change)) {
            if (!value) next.delete(key);
            else next.set(key, value);
          }
          return next;
        },
        {replace: true},
      ),
    [setParams],
  );
  const onQueryChange = useCallback((value: string) => update({q: value || null}), [update]);

  const visible = useMemo(() => {
    if (!state.shows) return [];
    const needle = q.toLocaleLowerCase();
    const list = needle ? state.shows.filter(show => show.show_name.toLocaleLowerCase().includes(needle)) : [...state.shows];
    list.sort((a, b) =>
      sort === 'title' ? sortKey(a.show_name).localeCompare(sortKey(b.show_name)) : b.last_added_at - a.last_added_at,
    );
    return list;
  }, [state.shows, q, sort]);

  const isLoading = state.shows === null && state.error === null;
  const hasShows = Boolean(state.shows && state.shows.length > 0);

  let body: ReactNode;
  if (isLoading) {
    body = <MediaGridSkeleton shape="poster" count={12} />;
  } else if (state.error && !state.shows) {
    body = <LoadError title="Shows could not be loaded" error={state.error} onRetry={() => setNonce(n => n + 1)} />;
  } else if (!hasShows) {
    body = (
      <EmptyPanel
        title="No shows yet"
        icon={<Tv size={28} strokeWidth={1.5} />}
        description="Episodes named like “Show Name S01E02” are grouped into shows and seasons automatically. Add a link or magnet in Downloads, or drop episode files anywhere in this window."
        actions={<Button label="Add a link" variant="primary" href="/downloads" />}
      />
    );
  } else if (visible.length === 0) {
    body = (
      <EmptyPanel
        title={`No shows match “${q}”`}
        description="Check the spelling, or search for part of the name."
        actions={<Button label="Clear search" variant="secondary" onClick={() => update({q: null})} />}
      />
    );
  } else {
    body = (
      <MediaGrid shape="poster" label="Shows">
        {visible.map(show => {
          const poster = posterSources(show.poster_item_id, version);
          return (
            <MediaTile
              key={show.show_name}
              href={showHref(show.show_name)}
              title={show.show_name}
              meta={showMeta(show)}
              imageSrc={poster?.src}
              imageSrcSet={poster?.srcSet}
              shape="poster"
            />
          );
        })}
      </MediaGrid>
    );
  }

  return (
    <>
      <PageHeader title="Shows" subtitle="Series grouped by show and season." />
      {hasShows ? (
        <LibraryToolbar
          searchLabel="Search shows"
          query={q}
          onQueryChange={onQueryChange}
          sort={sort}
          sortOptions={SORT_OPTIONS}
          onSortChange={value => update({sort: value === 'added' ? null : value})}
        />
      ) : null}
      {body}
    </>
  );
}
