/**
 * Inbox items (status needs_review) with live updates: the `inbox` event
 * carries the current count, and a different count than the list holds
 * means something arrived or was decided elsewhere, so the list reloads.
 */
import {useCallback, useEffect, useRef, useState} from 'react';
import {inbox as inboxApi} from '../../api/client';
import {useStashEvents} from '../../api/events';
import type {ItemDetail} from '../../api/types';
import {errorMessage} from '../../lib/errors';

export type InboxStatus = 'loading' | 'ready' | 'error';

export interface InboxData {
  status: InboxStatus;
  error: string | null;
  items: ItemDetail[];
  reload: () => void;
  /** Drops an item that was just decided. */
  removeItem: (id: string) => void;
}

const RELOAD_DELAY_MS = 400;

export function useInbox(): InboxData {
  const [status, setStatus] = useState<InboxStatus>('loading');
  const [error, setError] = useState<string | null>(null);
  const [items, setItems] = useState<ItemDetail[]>([]);
  const generation = useRef(0);
  const countRef = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  /** Fetches the list; state changes only happen when the answer arrives. */
  const fetchList = useCallback(() => {
    generation.current += 1;
    const current = generation.current;
    inboxApi
      .list()
      .then(result => {
        if (current !== generation.current) return;
        setItems(result.items);
        countRef.current = result.items.length;
        setStatus('ready');
        setError(null);
      })
      .catch((cause: unknown) => {
        if (current !== generation.current) return;
        setError(errorMessage(cause, 'Inbox could not be loaded.'));
        setStatus(previous => (previous === 'ready' ? previous : 'error'));
      });
  }, []);

  useEffect(() => {
    fetchList();
    return () => {
      generation.current += 1;
      if (timer.current) clearTimeout(timer.current);
    };
  }, [fetchList]);

  const reload = useCallback(() => {
    setStatus(previous => (previous === 'ready' ? previous : 'loading'));
    fetchList();
  }, [fetchList]);

  const scheduleReload = useCallback(() => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      timer.current = null;
      fetchList();
    }, RELOAD_DELAY_MS);
  }, [fetchList]);

  useStashEvents({
    inbox: event => {
      if (event.count !== countRef.current) scheduleReload();
    },
    library: event => {
      if (event.type !== 'added' && items.some(item => item.id === event.item_id)) scheduleReload();
    },
  });

  const removeItem = useCallback((id: string) => {
    setItems(current => {
      const next = current.filter(item => item.id !== id);
      countRef.current = next.length;
      return next;
    });
  }, []);

  return {status, error, items, reload, removeItem};
}
