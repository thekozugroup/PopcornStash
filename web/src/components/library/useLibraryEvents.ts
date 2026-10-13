/**
 * Live library updates from the shared event stream (`library` and `job`
 * events). Changes are coalesced: a scan or a batch of thumbnails can emit
 * hundreds of events, and each page only needs one refresh per burst.
 */
import {useEffect, useRef} from 'react';
import {useStashEvents} from '../../api/events';

export interface LibraryEventHandlers {
  /** Items whose metadata or thumbnail changed (batched). */
  onChanged?: (ids: string[]) => void;
  /**
   * Items the library reported as updated (batched with `onChanged`, thumbnail
   * and probe jobs left out). An item accepted from Inbox or organized into a
   * category is "updated", so a page that does not show it yet may need it now.
   */
  onUpdated?: (ids: string[]) => void;
  /** Something was added to the library (debounced, once per burst). */
  onAdded?: () => void;
  /** An item left the library (trash, missing). */
  onRemoved?: (id: string) => void;
}

const CHANGE_DELAY_MS = 400;
const ADD_DELAY_MS = 1500;

export function useLibraryEvents(handlers: LibraryEventHandlers, enabled = true) {
  const latest = useRef(handlers);
  const pending = useRef(new Set<string>());
  const pendingUpdated = useRef(new Set<string>());
  const changeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const addTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    latest.current = handlers;
  });

  useEffect(
    () => () => {
      if (changeTimer.current) clearTimeout(changeTimer.current);
      if (addTimer.current) clearTimeout(addTimer.current);
    },
    [],
  );

  const queueChange = (id: string, isLibraryUpdate = false) => {
    pending.current.add(id);
    if (isLibraryUpdate) pendingUpdated.current.add(id);
    if (changeTimer.current) return;
    changeTimer.current = setTimeout(() => {
      changeTimer.current = null;
      const ids = [...pending.current];
      const updated = [...pendingUpdated.current];
      pending.current.clear();
      pendingUpdated.current.clear();
      if (ids.length) latest.current.onChanged?.(ids);
      if (updated.length) latest.current.onUpdated?.(updated);
    }, CHANGE_DELAY_MS);
  };

  const queueAdd = () => {
    if (addTimer.current) clearTimeout(addTimer.current);
    addTimer.current = setTimeout(() => {
      addTimer.current = null;
      latest.current.onAdded?.();
    }, ADD_DELAY_MS);
  };

  useStashEvents(
    {
      library: event => {
        if (event.type === 'removed') latest.current.onRemoved?.(event.item_id);
        else if (event.type === 'added') queueAdd();
        else queueChange(event.item_id, true);
      },
      job: event => {
        // A finished thumbnail or probe changes what a tile shows.
        if (event.item_id && (event.kind === 'thumb' || event.kind === 'probe') && event.state === 'done') {
          queueChange(event.item_id);
        }
      },
    },
    enabled,
  );
}
