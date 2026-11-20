import {useCallback, useEffect, useState} from 'react';
import {ApiError} from '../../api/client';
import {getTransferFiles, type TransferFiles} from '../../api/downloadFiles';
import {errorMessage} from '../../lib/errors';

interface ManifestObserver {
  received: (next: TransferFiles) => void;
  failed: (cause: unknown) => void;
  stale: () => void;
}

/** One request at a time for one visible dialog. Each visible period owns its abort signal. */
export function watchTransferFiles(id: string, observer: ManifestObserver) {
  let active: AbortController | null = null;
  let poll: ReturnType<typeof setTimeout>;
  let expiry: ReturnType<typeof setTimeout>;

  async function load(signal: AbortSignal) {
    if (signal.aborted || document.hidden) return;
    try {
      const next = await getTransferFiles(id, signal);
      if (signal.aborted || document.hidden) return;
      clearTimeout(expiry);
      observer.received(next);
      // A hung refresh must expire the previous progress snapshot.
      expiry = setTimeout(observer.stale, 9000);
    } catch (cause) {
      if (signal.aborted || document.hidden) return;
      observer.failed(cause);
      if (cause instanceof ApiError && [401, 403, 404].includes(cause.status)) return;
    }
    poll = setTimeout(() => void load(signal), 3000);
  }

  function stop() {
    active?.abort();
    active = null;
    clearTimeout(poll);
    clearTimeout(expiry);
  }

  function syncVisibility() {
    if (document.hidden) {
      stop();
      observer.stale();
    } else if (!active) {
      active = new AbortController();
      void load(active.signal);
    }
  }

  document.addEventListener('visibilitychange', syncVisibility);
  syncVisibility();
  return () => {
    document.removeEventListener('visibilitychange', syncVisibility);
    stop();
  };
}

/** The dialog is keyed by download id. Its local choices survive polling pauses. */
export function useTransferFiles(id: string, busy: boolean) {
  const [data, setData] = useState<TransferFiles | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [fresh, setFresh] = useState(false);
  const [revision, setRevision] = useState(0);
  const refresh = useCallback(() => {
    setFresh(false);
    setRevision(value => value + 1);
  }, []);

  useEffect(() => {
    if (busy) return;
    return watchTransferFiles(id, {
      received: next => {
        // A transient metadata wait keeps the open editor and its unsaved selection mounted.
        setData(current => !next.ready && current?.ready ? current : next);
        setError(null);
        setFresh(next.ready);
      },
      failed: cause => {
        setFresh(false);
        setError(errorMessage(cause, 'The file list could not be refreshed.'));
        if (cause instanceof ApiError && [401, 403, 404].includes(cause.status)) setData(null);
      },
      stale: () => setFresh(false),
    });
  }, [id, busy, revision]);
  return {data, error, fresh, refresh, setData};
}
