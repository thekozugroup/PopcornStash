/**
 * Loads one piece of server data for a settings section, with abort on
 * unmount, a `reload()` for retry and after changes, and `mutate()` for
 * applying a known change locally (from a response or a live event) without
 * waiting for a reload.
 */
import {useCallback, useEffect, useRef, useState} from 'react';

export interface Resource<T> {
  /** Last data that loaded; kept while reloading. */
  data: T | undefined;
  /** The error from the latest attempt, if it failed. */
  error: unknown;
  /** True until the first answer (or after a retry from an error). */
  isLoading: boolean;
  /** True while any request (including a reload) has not answered yet. */
  isPending: boolean;
  reload: () => void;
  mutate: (update: (current: T) => T) => void;
}

interface Settled<T> {
  data: T | undefined;
  error: unknown;
  /** Which request this state belongs to. */
  key: string | null;
}

export function useResource<T>(
  load: (signal: AbortSignal) => Promise<T>,
  options: {enabled?: boolean; key?: string} = {},
): Resource<T> {
  const {enabled = true, key = ''} = options;
  const loadRef = useRef(load);
  useEffect(() => {
    loadRef.current = load;
  });

  const [version, setVersion] = useState(0);
  const [settled, setSettled] = useState<Settled<T>>({data: undefined, error: undefined, key: null});
  const requestKey = `${key}#${version}`;

  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    loadRef.current(controller.signal).then(
      data => {
        if (!controller.signal.aborted) setSettled({data, error: undefined, key: requestKey});
      },
      (error: unknown) => {
        if (!controller.signal.aborted) setSettled(previous => ({data: previous.data, error, key: requestKey}));
      },
    );
    return () => controller.abort();
  }, [enabled, requestKey]);

  const reload = useCallback(() => setVersion(value => value + 1), []);
  const mutate = useCallback((update: (current: T) => T) => {
    setSettled(previous => (previous.data === undefined ? previous : {...previous, data: update(previous.data)}));
  }, []);

  const isCurrent = settled.key === requestKey;
  return {
    data: settled.data,
    error: isCurrent ? settled.error : undefined,
    isLoading: enabled && !isCurrent && (settled.data === undefined || settled.error !== undefined),
    isPending: enabled && !isCurrent,
    reload,
    mutate,
  };
}
