/**
 * Library items created from finished downloads, so each finished row can
 * show the poster or frame (where the files went comes from the download's
 * own `destinations`). Loads a couple of items per download, caches them for
 * the session, and refreshes an item when a `library` event says it changed
 * (for example after an Inbox decision).
 */
import {useEffect, useRef, useState} from 'react';
import {items as itemsApi} from '../../api/client';
import {useStashEvent} from '../../api/events';
import type {Download, ItemDetail} from '../../api/types';

/** Items looked up per download; enough to find one with a picture. */
const ITEMS_PER_DOWNLOAD = 2;
const cache = new Map<string, ItemDetail>();

export function useDownloadItems(finished: Download[]): Map<string, ItemDetail> {
  const [, setVersion] = useState(0);
  const requested = useRef(new Set<string>());

  const wanted = finished
    .filter(download => download.state === 'sorted')
    .flatMap(download => download.item_ids.slice(0, ITEMS_PER_DOWNLOAD));
  const missingKey = wanted.filter(id => !cache.has(id)).join(',');

  useEffect(() => {
    if (!missingKey) return;
    const ids = missingKey.split(',').filter(id => !requested.current.has(id));
    if (ids.length === 0) return;
    for (const id of ids) requested.current.add(id);
    // No cancellation: whatever loads stays cached for the next render.
    void Promise.allSettled(
      ids.map(id =>
        itemsApi.get(id).then(item => {
          cache.set(id, item);
        }),
      ),
    ).then(() => setVersion(version => version + 1));
  }, [missingKey]);

  useStashEvent('library', event => {
    if (!cache.has(event.item_id) && !requested.current.has(event.item_id)) return;
    if (event.type === 'removed') {
      cache.delete(event.item_id);
      setVersion(version => version + 1);
      return;
    }
    itemsApi
      .get(event.item_id)
      .then(item => {
        cache.set(item.id, item);
        setVersion(version => version + 1);
      })
      .catch(() => undefined);
  });

  return cache;
}
