import {Button} from '@astryxdesign/core/Button';
import {VStack} from '@astryxdesign/core/Layout';
import {Text} from '@astryxdesign/core/Text';
import {Folder} from 'lucide-react';
import {useEffect, useEffectEvent, useReducer, useRef, useState, type ReactNode} from 'react';
import {libraries} from '../../api/client';
import type {Item} from '../../api/types';
import {formatCount, watchedFraction} from '../../lib/format';
import {MediaTile} from '../MediaTile';
import {LoadError} from './LoadError';
import {displayTitle, itemHref, itemMeta, thumbSources, timeLeft} from './libraryFormat';
import {Shelf, ShelfSkeleton} from './Shelf';
import {useLibraryEvents} from './useLibraryEvents';
import {appendCollections, emptyFeedState, videoFeedCache} from './videoFeedCache';

export function VideoShelf({title, items, resume = false, leading = resume, href}: {title: string; items: Item[]; resume?: boolean; leading?: boolean; href?: string}) {
  if (!items.length) return null;
  return <Shelf title={title} shape="frame" headingLevel={2} href={href} resume={resume}>
    {items.map((item, index) => {
      const thumb = thumbSources(item);
      return <MediaTile key={item.id} resume={resume} priority={leading && index === 0} href={itemHref(item)} title={displayTitle(item)} meta={resume ? timeLeft(item) : itemMeta(item)} imageSrc={thumb?.src} imageSrcSet={thumb?.srcSet} shape="frame" progress={watchedFraction(item.progress)} />;
    })}
  </Shelf>;
}

export function VideoFeedView({id, empty}: {id: string; empty: ReactNode}) {
  const [{data, error, moreError, busy}, dispatch] = useReducer(videoFeedCache, emptyFeedState);
  const [revision, setRevision] = useState(0);
  const request = useRef<AbortController | null>(null);
  const loadedCollectionCount = useEffectEvent(() => data?.collections.length ?? 0);
  const invalidate = () => {
    request.current?.abort();
    setRevision(n => n + 1);
  };
  useLibraryEvents({
    onAdded: invalidate,
    onUpdated: invalidate,
    onRemoved: itemId => {
      dispatch({type: 'removed', id: itemId});
      invalidate();
    },
  });
  useEffect(() => {
    const controller = new AbortController();
    request.current?.abort(); request.current = controller;
    const refresh = async () => {
      try {
        const loaded = loadedCollectionCount();
        let next = await libraries.feed(id, undefined, controller.signal);
        const seen = new Set<string>();
        while (!controller.signal.aborted && next.collections.length < loaded && next.next_cursor && !seen.has(next.next_cursor)) {
          seen.add(next.next_cursor);
          const page = await libraries.feed(id, next.next_cursor, controller.signal);
          next = appendCollections(next, page);
        }
        if (!controller.signal.aborted) dispatch({type: 'refreshed', page: next});
      } catch (failure) {
        if (!controller.signal.aborted) dispatch({type: 'failed', during: 'refresh', error: failure});
      } finally {
        if (request.current === controller) request.current = null;
      }
    };
    void refresh();
    return () => controller.abort();
  }, [id, revision]);
  useEffect(() => () => request.current?.abort(), []);
  const more = async () => {
    if (!data?.next_cursor || busy || request.current) return;
    const controller = new AbortController(); request.current = controller;
    dispatch({type: 'loadingMore'});
    try {
      const next = await libraries.feed(id, data.next_cursor, controller.signal);
      if (!controller.signal.aborted) dispatch({type: 'appended', page: next});
    } catch (failure) {
      if (!controller.signal.aborted) dispatch({type: 'failed', during: 'pagination', error: failure});
    } finally {
      if (request.current === controller) request.current = null;
    }
  };
  if (error && !data) return <LoadError title="Videos could not be loaded" error={error} onRetry={() => setRevision(n => n + 1)} />;
  if (!data) return <VStack gap={8}><ShelfSkeleton shape="frame" count={4} /><ShelfSkeleton shape="frame" count={4} /></VStack>;
  if (!data.continue_watching.length && !data.suggested.length && !data.recent.length && !data.collections.length && !error) return empty;
  return <VStack gap={8}>
    {error ? <LoadError title="The feed could not be refreshed" error={error} onRetry={() => setRevision(n => n + 1)} /> : null}
    <VideoShelf title="Continue watching" items={data.continue_watching} resume />
    <VideoShelf title="Suggested for you" items={data.suggested} leading={!data.continue_watching.length} />
    <VideoShelf title="Recently added" items={data.recent} leading={!data.suggested.length && !data.continue_watching.length} href={`?view=all`} />
    {data.collections.map(({folder, recent}) => <Shelf key={folder.key} title={folder.name} icon={Folder} shape="frame" headingLevel={2} note={formatCount(folder.item_count, 'video')} href={`?download=${encodeURIComponent(folder.key)}&subfolders=collapsed`} linkLabel="Open collection" emptyText={recent.length ? undefined : "No videos in this collection yet."}>
      {recent.map(item => {
        const thumb = thumbSources(item);
        return <MediaTile key={item.id} href={itemHref(item)} title={displayTitle(item)} meta={itemMeta(item)} imageSrc={thumb?.src} imageSrcSet={thumb?.srcSet} shape="frame" progress={watchedFraction(item.progress)} />;
      })}
    </Shelf>)}
    {moreError ? <Text role="alert">More collections could not be loaded. Try again.</Text> : null}
    {data.next_cursor ? <Button label={moreError ? 'Try loading collections again' : 'More collections'} variant="secondary" isLoading={busy} onClick={() => void more()} /> : null}
  </VStack>;
}
