/**
 * Server-sent events from `GET /api/events` (architecture section 8).
 *
 * One EventSource is shared by every subscriber in the tab. It opens with the
 * first subscriber, closes with the last, and reconnects with backoff when the
 * server drops it (EventSource only retries on its own for network errors, not
 * for non-200 answers such as a restart or a 401).
 */
import {useEffect, useRef, useSyncExternalStore} from 'react';
import type {StashEventMap, StashEventName} from './types';

export type EventStreamStatus = 'idle' | 'connecting' | 'open' | 'reconnecting';

type Handler<K extends StashEventName> = (data: StashEventMap[K]) => void;
type AnyHandler = (data: unknown) => void;

export const STASH_EVENT_NAMES: readonly StashEventName[] = [
  'download',
  'library',
  'vpn',
  'inbox',
  'job',
] as const;

const EVENTS_URL = '/api/events';
const MAX_BACKOFF_MS = 30_000;

const handlers = new Map<StashEventName, Set<AnyHandler>>();
const statusListeners = new Set<() => void>();
let source: EventSource | null = null;
let status: EventStreamStatus = 'idle';
let retryTimer: ReturnType<typeof setTimeout> | null = null;
let attempt = 0;

function setStatus(next: EventStreamStatus) {
  if (status === next) return;
  status = next;
  for (const listener of statusListeners) listener();
}

function subscriberCount(): number {
  let count = 0;
  for (const set of handlers.values()) count += set.size;
  return count;
}

function dispatch(name: StashEventName, event: MessageEvent<string>) {
  const set = handlers.get(name);
  if (!set || set.size === 0) return;
  let data: unknown;
  try {
    data = JSON.parse(event.data) as unknown;
  } catch {
    return;
  }
  for (const handler of set) handler(data);
}

function open() {
  if (source || typeof EventSource === 'undefined') return;
  setStatus(attempt === 0 ? 'connecting' : 'reconnecting');
  const es = new EventSource(EVENTS_URL, {withCredentials: true});
  source = es;
  for (const name of STASH_EVENT_NAMES) {
    es.addEventListener(name, event => dispatch(name, event as MessageEvent<string>));
  }
  es.onopen = () => {
    attempt = 0;
    setStatus('open');
  };
  es.onerror = () => {
    if (es.readyState === EventSource.CLOSED) {
      // The browser gave up (non-200 answer). Retry ourselves.
      close();
      scheduleReconnect();
    } else {
      setStatus('reconnecting');
    }
  };
}

function close() {
  if (source) {
    source.close();
    source = null;
  }
}

function scheduleReconnect() {
  if (retryTimer || subscriberCount() === 0) return;
  attempt += 1;
  const delay = Math.min(MAX_BACKOFF_MS, 1000 * 2 ** Math.min(attempt, 5));
  setStatus('reconnecting');
  retryTimer = setTimeout(() => {
    retryTimer = null;
    if (subscriberCount() > 0) open();
  }, delay);
}

function stopIfUnused() {
  if (subscriberCount() > 0) return;
  if (retryTimer) {
    clearTimeout(retryTimer);
    retryTimer = null;
  }
  close();
  attempt = 0;
  setStatus('idle');
}

/** Subscribes outside React. Returns an unsubscribe function. */
export function subscribe<K extends StashEventName>(name: K, handler: Handler<K>): () => void {
  let set = handlers.get(name);
  if (!set) {
    set = new Set();
    handlers.set(name, set);
  }
  const anyHandler = handler as AnyHandler;
  set.add(anyHandler);
  open();
  return () => {
    set.delete(anyHandler);
    stopIfUnused();
  };
}

export type StashEventHandlers = {[K in StashEventName]?: Handler<K>};

/**
 * Listens to typed server events while the component is mounted. Handlers may
 * change between renders without reconnecting.
 *
 *   useStashEvents({
 *     download: d => updateRow(d),
 *     inbox: ({count}) => setInboxCount(count),
 *   });
 */
export function useStashEvents(eventHandlers: StashEventHandlers, enabled = true): EventStreamStatus {
  const latest = useRef(eventHandlers);
  useEffect(() => {
    latest.current = eventHandlers;
  });

  const names = STASH_EVENT_NAMES.filter(name => eventHandlers[name] !== undefined).join(',');

  useEffect(() => {
    if (!enabled || !names) return;
    const unsubscribers = (names.split(',') as StashEventName[]).map(name =>
      subscribe(name, ((data: unknown) => {
        const handler = latest.current[name] as ((value: unknown) => void) | undefined;
        handler?.(data);
      }) as Handler<typeof name>),
    );
    return () => {
      for (const unsubscribe of unsubscribers) unsubscribe();
    };
  }, [enabled, names]);

  return useEventStreamStatus();
}

/** Listens to one typed server event. */
export function useStashEvent<K extends StashEventName>(
  name: K,
  handler: Handler<K>,
  enabled = true,
): EventStreamStatus {
  return useStashEvents({[name]: handler} as StashEventHandlers, enabled);
}

/** Connection state of the shared event stream. */
export function useEventStreamStatus(): EventStreamStatus {
  return useSyncExternalStore(
    listener => {
      statusListeners.add(listener);
      return () => statusListeners.delete(listener);
    },
    () => status,
    () => 'idle',
  );
}
