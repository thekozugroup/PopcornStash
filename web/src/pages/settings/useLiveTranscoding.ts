import {useEffect, useRef, useState} from 'react';
import {libraries} from '../../api/client';
import {useStashEvents} from '../../api/events';
import type {ConversionView, TranscodingOverview} from '../../api/types';

/** Single in-flight request, event invalidation, and polling when SSE is unavailable. */
export function useLiveTranscoding() {
  const [data, setData] = useState<TranscodingOverview>();
  const [error, setError] = useState(false);
  const [updated, setUpdated] = useState(0);
  const [now, setNow] = useState(0);
  const epoch = useRef(0);
  const refresh = useRef(() => {});
  const stream = useStashEvents({
    job: event => { if (event.kind === 'library_conversion') refresh.current(); },
    libraries: () => refresh.current(),
    library: () => refresh.current(),
  });
  useEffect(() => {
    const lifetime = new AbortController();
    let running = false;
    let queued = false;
    const load = async () => {
      if (lifetime.signal.aborted) return;
      if (running) { queued = true; return; }
      running = true;
      const generation = epoch.current;
      const request = new AbortController();
      const abort = () => request.abort();
      lifetime.signal.addEventListener('abort', abort, {once: true});
      const timeout = setTimeout(abort, 15000);
      try {
        const next = await libraries.transcoding(request.signal);
        if (!lifetime.signal.aborted && generation === epoch.current) { setData(next); setUpdated(Date.now()); setError(false); }
      } catch { if (!lifetime.signal.aborted && generation === epoch.current) setError(true); }
      finally {
        clearTimeout(timeout);
        lifetime.signal.removeEventListener('abort', abort);
        running = false;
        if (queued && !lifetime.signal.aborted) { queued = false; void load(); }
      }
    };
    const reload = () => { void load(); };
    refresh.current = reload;
    const visible = () => { if (document.visibilityState === 'visible') reload(); };
    const timer = setInterval(() => { setNow(Date.now()); if (document.visibilityState === 'visible') reload(); }, 2000);
    window.addEventListener('focus', reload);
    document.addEventListener('visibilitychange', visible);
    reload();
    return () => { lifetime.abort(); clearInterval(timer); window.removeEventListener('focus', reload); document.removeEventListener('visibilitychange', visible); refresh.current = () => {}; };
  }, []);
  const accept = (id: string, view: ConversionView) => {
    epoch.current += 1;
    setData(current => current && ({...current, libraries: current.libraries.map(l => l.id === id ? {...l, policy: view.policy} : l)}));
    refresh.current();
  };
  return {data, error, updated, accept, stale: updated > 0 && now - updated > 10000, stream, reload: () => refresh.current()};
}
