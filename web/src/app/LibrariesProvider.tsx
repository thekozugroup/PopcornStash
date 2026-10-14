/**
 * The libraries the signed-in person can see (`GET /api/libraries`), in the
 * owner's order, shared by the sidebar, the quick links, Home, the library
 * pages, Inbox and Settings. It loads once inside the app shell and refetches
 * on the server's `libraries` event (the person's own list changed), after
 * the event stream reconnects, and when a page that changed a library calls
 * `reload()`.
 */
import {createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode} from 'react';
import {libraries as librariesApi} from '../api/client';
import {useStashEvent} from '../api/events';
import type {ClaimKind, Library} from '../api/types';

export type LibrariesStatus = 'loading' | 'ready' | 'error';

export interface LibrariesState {
  /** Null until the first answer. */
  libraries: Library[] | null;
  status: LibrariesStatus;
  error: unknown;
  /** Refetches the list; resolves with it (or the previous list when the request fails). */
  reload: () => Promise<Library[] | null>;
  find: (id: string | null | undefined) => Library | undefined;
  /** The library that claims a kind of new file, when the person sees it. */
  claimedBy: (kind: ClaimKind) => Library | undefined;
}

const LibrariesContext = createContext<LibrariesState | null>(null);

export function LibrariesProvider({children}: {children: ReactNode}) {
  const [libraries, setLibraries] = useState<Library[] | null>(null);
  const [status, setStatus] = useState<LibrariesStatus>('loading');
  const [error, setError] = useState<unknown>(null);
  const generation = useRef(0);
  const latest = useRef<Library[] | null>(null);

  const reload = useCallback((): Promise<Library[] | null> => {
    generation.current += 1;
    const current = generation.current;
    return librariesApi.list().then(
      result => {
        if (current === generation.current) {
          latest.current = result.libraries;
          setLibraries(result.libraries);
          setStatus('ready');
          setError(null);
        }
        return result.libraries;
      },
      (cause: unknown) => {
        if (current === generation.current) {
          setError(cause);
          setStatus(previous => (previous === 'ready' ? previous : 'error'));
        }
        return latest.current;
      },
    );
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const streamStatus = useStashEvent('libraries', () => void reload());

  // Events sent while the stream was down are gone: reload once it is back.
  const wasDown = useRef(false);
  useEffect(() => {
    if (streamStatus === 'reconnecting') wasDown.current = true;
    else if (streamStatus === 'open' && wasDown.current) {
      wasDown.current = false;
      void reload();
    }
  }, [streamStatus, reload]);

  const value = useMemo<LibrariesState>(() => {
    const byId = new Map((libraries ?? []).map(library => [library.id, library]));
    return {
      libraries,
      status,
      error,
      reload,
      find: id => (id ? byId.get(id) : undefined),
      claimedBy: kind => (libraries ?? []).find(library => library.claims.includes(kind)),
    };
  }, [libraries, status, error, reload]);

  return <LibrariesContext.Provider value={value}>{children}</LibrariesContext.Provider>;
}

export function useLibraries(): LibrariesState {
  const value = useContext(LibrariesContext);
  if (!value) throw new Error('useLibraries must be used inside LibrariesProvider.');
  return value;
}
