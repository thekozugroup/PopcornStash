import {Button} from '@astryxdesign/core/Button';
import {VStack} from '@astryxdesign/core/Layout';
import {Text} from '@astryxdesign/core/Text';
import {Folder} from 'lucide-react';
import {useEffect, useRef, useState, type ReactNode} from 'react';
import {libraries} from '../../api/client';
import type {Item, VideoFeed} from '../../api/types';
import {formatCount, watchedFraction} from '../../lib/format';
import {MediaTile} from '../MediaTile';
import {LoadError} from './LoadError';
import {displayTitle, itemHref, itemMeta, thumbSources, timeLeft} from './libraryFormat';
import {Shelf, ShelfSkeleton} from './Shelf';
import {useLibraryEvents} from './useLibraryEvents';

export function VideoShelf({title, items, resume = false, href}: {title: string; items: Item[]; resume?: boolean; href?: string}) {
  if (!items.length) return null;
  return <Shelf title={title} shape="frame" headingLevel={2} href={href}>
    {items.map(item => {
      const thumb = thumbSources(item);
      return <MediaTile key={item.id} href={itemHref(item)} title={displayTitle(item)} meta={resume ? timeLeft(item) : itemMeta(item)} imageSrc={thumb?.src} imageSrcSet={thumb?.srcSet} shape="frame" progress={watchedFraction(item.progress)} />;
    })}
  </Shelf>;
}

export function VideoFeedView({id, empty}: {id: string; empty: ReactNode}) {
  const [data, setData] = useState<VideoFeed | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [moreError, setMoreError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const [revision, setRevision] = useState(0);
  const request = useRef<AbortController | null>(null);
  const loaded = useRef(0);
  useLibraryEvents({onAdded: () => setRevision(n => n + 1), onRemoved: () => setRevision(n => n + 1), onUpdated: () => setRevision(n => n + 1)});
  useEffect(() => {
    const controller = new AbortController();
    request.current?.abort(); request.current = controller;
    const refresh = async () => {
      try {
        const next = await libraries.feed(id, undefined, controller.signal);
        const seen = new Set<string>();
        while (next.collections.length < loaded.current && next.next_cursor && !seen.has(next.next_cursor)) {
          seen.add(next.next_cursor);
          const page = await libraries.feed(id, next.next_cursor, controller.signal);
          next.collections = [...new Map([...next.collections, ...page.collections].map(row => [row.folder.key, row])).values()];
          next.next_cursor = page.next_cursor;
        }
        if (!controller.signal.aborted && request.current === controller) {loaded.current = next.collections.length; setData(next); setError(null); setBusy(false); setMoreError(null); request.current = null;}
      } catch (failure) {if (!controller.signal.aborted && request.current === controller) {setData(null); setError(failure); setBusy(false); request.current = null;}}
    };
    void refresh();
    return () => controller.abort();
  }, [id, revision]);
  useEffect(() => () => request.current?.abort(), []);
  const more = async () => {
    if (!data?.next_cursor || busy || request.current) return;
    const controller = new AbortController(); request.current = controller;
    setBusy(true); setMoreError(null);
    try {
      const next = await libraries.feed(id, data.next_cursor, controller.signal);
      if (!controller.signal.aborted && request.current === controller) setData(current => {
        if (!current) return current;
        const collections = [...new Map([...current.collections, ...next.collections].map(row => [row.folder.key, row])).values()];
        loaded.current = collections.length;
        return {...current, collections, next_cursor: next.next_cursor};
      });
    } catch (failure) {if (!controller.signal.aborted && request.current === controller) setMoreError(failure);}
    finally {if (!controller.signal.aborted && request.current === controller) {request.current = null; setBusy(false);}}
  };
  if (error) return <LoadError title="Videos could not be loaded" error={error} onRetry={() => setRevision(n => n + 1)} />;
  if (!data) return <VStack gap={8}><ShelfSkeleton shape="frame" count={4} /><ShelfSkeleton shape="frame" count={4} /></VStack>;
  if (!data.recent.length && !data.collections.length) return empty;
  return <VStack gap={8}>
    <VideoShelf title="Continue watching" items={data.continue_watching} resume />
    <VideoShelf title="Suggested for you" items={data.suggested} />
    <VideoShelf title="Recently added" items={data.recent} href={`?view=all`} />
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
