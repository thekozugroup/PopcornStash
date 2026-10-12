/**
 * Number of items waiting for review in Inbox: loads once, then follows
 * `inbox` events.
 */
import {useEffect, useState} from 'react';
import {inbox} from '../api/client';
import {useStashEvent} from '../api/events';

export function useInboxCount(enabled = true): number | null {
  const [count, setCount] = useState<number | null>(null);

  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    inbox
      .list(controller.signal)
      .then(result => setCount(result.items.length))
      .catch(() => undefined);
    return () => controller.abort();
  }, [enabled]);

  useStashEvent('inbox', event => setCount(event.count), enabled);

  return count;
}
