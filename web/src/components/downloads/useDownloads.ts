/**
 * Downloads list for the Downloads page: loads active and finished
 * downloads, then follows `download` events. After the event stream drops
 * and reconnects, it reloads once so nothing missed in between is lost.
 */
import {useCallback, useEffect, useReducer, useRef} from 'react';
import {downloads as downloadsApi} from '../../api/client';
import {useStashEvent} from '../../api/events';
import type {Download} from '../../api/types';
import {errorMessage} from '../../lib/errors';

export type DownloadsStatus = 'loading' | 'ready' | 'error';

interface State {
  status: DownloadsStatus;
  byId: Map<string, Download>;
  error: string | null;
}

type Action =
  | {type: 'loading'}
  | {type: 'loaded'; items: Download[]}
  | {type: 'failed'; error: string}
  | {type: 'upsert'; item: Download}
  | {type: 'remove'; id: string};

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case 'loading':
      return state.status === 'ready' ? state : {...state, status: 'loading', error: null};
    case 'loaded':
      return {status: 'ready', error: null, byId: new Map(action.items.map(item => [item.id, item]))};
    case 'failed':
      // Keep showing what we have when a background reload fails.
      return state.status === 'ready' ? state : {...state, status: 'error', error: action.error};
    case 'upsert': {
      const byId = new Map(state.byId);
      byId.set(action.item.id, action.item);
      return {...state, byId};
    }
    case 'remove': {
      if (!state.byId.has(action.id)) return state;
      const byId = new Map(state.byId);
      byId.delete(action.id);
      return {...state, byId};
    }
    default:
      return state;
  }
}

export interface DownloadsData {
  status: DownloadsStatus;
  error: string | null;
  items: Download[];
  reload: () => void;
  upsert: (item: Download) => void;
  remove: (id: string) => void;
}

const FINISHED_LIMIT = 100;

export function useDownloads(): DownloadsData {
  const [state, dispatch] = useReducer(reducer, {status: 'loading', byId: new Map(), error: null});
  const generation = useRef(0);

  const load = useCallback(() => {
    generation.current += 1;
    const current = generation.current;
    dispatch({type: 'loading'});
    Promise.all([downloadsApi.list('active'), downloadsApi.list('finished', FINISHED_LIMIT)])
      .then(([active, finished]) => {
        if (current !== generation.current) return;
        const seen = new Set<string>();
        const items: Download[] = [];
        for (const item of [...active.items, ...finished.items]) {
          if (seen.has(item.id)) continue;
          seen.add(item.id);
          items.push(item);
        }
        dispatch({type: 'loaded', items});
      })
      .catch((error: unknown) => {
        if (current !== generation.current) return;
        dispatch({type: 'failed', error: errorMessage(error, 'Downloads could not be loaded.')});
      });
  }, []);

  useEffect(() => {
    load();
    return () => {
      generation.current += 1;
    };
  }, [load]);

  const streamStatus = useStashEvent('download', item => dispatch({type: 'upsert', item}));

  // Reload after a reconnect: events sent while the stream was down are gone.
  const wasDown = useRef(false);
  useEffect(() => {
    if (streamStatus === 'reconnecting') {
      wasDown.current = true;
    } else if (streamStatus === 'open' && wasDown.current) {
      wasDown.current = false;
      load();
    }
  }, [streamStatus, load]);

  const upsert = useCallback((item: Download) => dispatch({type: 'upsert', item}), []);
  const remove = useCallback((id: string) => dispatch({type: 'remove', id}), []);

  return {
    status: state.status,
    error: state.error,
    items: Array.from(state.byId.values()),
    reload: load,
    upsert,
    remove,
  };
}
