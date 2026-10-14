/**
 * One library (`/libraries/:id`): its name exactly as the owner typed it,
 * one line that counts what is there, then the arrangement its `layout`
 * names: a poster or frame grid for one-offs, show posters for a series, the
 * month timeline for photos. The frame, the toolbar and the tiles are the
 * same whatever the library.
 *
 * A library that is deleted or hidden while open (the server answers 404,
 * or it leaves the person's list) shows "This library is not here any more."
 */
import * as stylex from '@stylexjs/stylex';
import {Button} from '@astryxdesign/core/Button';
import {Skeleton} from '@astryxdesign/core/Skeleton';
import {useEffect, useState} from 'react';
import {useParams} from 'react-router';
import {ApiError, libraries as librariesApi} from '../api/client';
import type {Library} from '../api/types';
import {useLibraries} from '../app/LibrariesProvider';
import {useAuth} from '../auth/AuthProvider';
import {EmptyPanel} from '../components/EmptyPanel';
import {ItemGridView, type ItemGridConfig} from '../components/library/ItemGridView';
import {LoadError} from '../components/library/LoadError';
import {SeriesView} from '../components/library/SeriesView';
import {TimelineView} from '../components/library/TimelineView';
import {useLibraryEvents} from '../components/library/useLibraryEvents';
import {MediaGridSkeleton} from '../components/MediaGrid';
import {PageHeader} from '../components/PageHeader';
import {emptyLibraryLine, layoutShape, libraryCountLine} from '../lib/libraries';
import {LibraryGlyph, libraryIcon} from '../lib/libraryIcons';

const styles = stylex.create({
  // The same footprint as the page header, so nothing jumps when the name arrives.
  loadingHeader: {
    display: 'flex',
    flexDirection: 'column',
    gap: 14,
    marginBottom: 32,
  },
});

/** The page for a library that is gone or was never shared with this person. */
export function LibraryGone() {
  return (
    <PageHeader
      title="This library is not here any more"
      subtitle="It was deleted, or it is no longer shared with you."
      actions={<Button label="Go to Home" variant="primary" href="/" />}
      isActionBelow
    />
  );
}

interface PageState {
  id: string;
  library: Library | null;
  error: unknown;
}

export default function LibraryPage() {
  const {id = ''} = useParams();
  const {isOwner} = useAuth();
  const {find, status} = useLibraries();
  const listed = find(id);
  const [nonce, setNonce] = useState(0);
  const [state, setState] = useState<PageState>({id: '', library: null, error: null});

  // The page's own copy keeps the count current; the shared list refetches only
  // when the library itself changes. A library that leaves the list is asked
  // for again, which answers 404 once it is gone.
  const listedVersion = listed ? `${listed.updated_at}:${listed.position}` : 'missing';
  useEffect(() => {
    const controller = new AbortController();
    librariesApi.get(id, controller.signal).then(
      library => setState({id, library, error: null}),
      (error: unknown) => {
        if (!controller.signal.aborted) setState(s => ({id, library: s.id === id ? s.library : null, error}));
      },
    );
    return () => controller.abort();
  }, [id, nonce, listedVersion]);

  useLibraryEvents({
    onAdded: () => setNonce(n => n + 1),
    onRemoved: () => setNonce(n => n + 1),
    onUpdated: () => setNonce(n => n + 1),
  });

  const fetched = state.id === id ? state.library : null;
  const error = state.id === id ? state.error : null;
  const isGone = error instanceof ApiError && error.code === 'not_found';
  const library = isGone ? null : (fetched ?? listed ?? null);

  if (isGone || (status === 'ready' && !listed && !fetched && error)) return <LibraryGone />;

  if (!library) {
    if (error) {
      return (
        <>
          <PageHeader title="Library" />
          <LoadError title="This library could not be loaded" error={error} onRetry={() => setNonce(n => n + 1)} />
        </>
      );
    }
    return (
      <div role="status" aria-busy="true" aria-label="Loading">
        <div {...stylex.props(styles.loadingHeader)}>
          <Skeleton width={220} height={30} radius={1} />
          <Skeleton width={120} height={18} radius={1} />
        </div>
        <MediaGridSkeleton shape="poster" count={12} />
      </div>
    );
  }

  const Icon = libraryIcon(library.icon);
  const empty = (
    <EmptyPanel
      title={`Nothing in ${library.name} yet`}
      description={emptyLibraryLine(library)}
      icon={<LibraryGlyph name={library.icon} size={28} />}
      actions={
        isOwner ? <Button label="Edit library" variant="secondary" href={`/settings/libraries/${encodeURIComponent(library.id)}`} /> : undefined
      }
    />
  );

  let view;
  if (library.layout === 'series') {
    view = <SeriesView key={library.id} libraryId={library.id} libraryName={library.name} icon={Icon} empty={empty} />;
  } else if (library.layout === 'timeline') {
    view = <TimelineView key={library.id} libraryId={library.id} libraryName={library.name} icon={Icon} empty={empty} />;
  } else {
    const isFrames = library.layout === 'frames';
    const config: ItemGridConfig = {
      scope: {library_id: library.id},
      label: library.name,
      shape: layoutShape(library.layout),
      noun: 'videos',
      // Frames show the date each clip was filmed, so that is their default order:
      // a phone backup imported in one go still reads as a timeline. Films carry a
      // release year, not a date taken, so posters leave that sort out.
      sortOptions: isFrames
        ? [
            {value: 'taken', label: 'Date filmed'},
            {value: 'added', label: 'Recently added'},
            {value: 'title', label: 'Title'},
            {value: 'size', label: 'Largest first'},
          ]
        : [
            {value: 'added', label: 'Recently added'},
            {value: 'title', label: 'Title'},
            {value: 'size', label: 'Largest first'},
          ],
      defaultSort: isFrames ? 'taken' : 'added',
      empty,
      placeholderFor: () => Icon,
    };
    view = <ItemGridView key={library.id} config={config} />;
  }

  return (
    <>
      <PageHeader title={library.name} subtitle={libraryCountLine(library)} />
      {view}
    </>
  );
}
