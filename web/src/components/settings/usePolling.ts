/**
 * Calls `reload` every few seconds while the tab is visible, and again when
 * the tab comes back, for settings lists that change on another device
 * (a pairing request, an invite being used) and have no live event.
 */
import {useEffect} from 'react';

export function usePolling(reload: () => void, intervalMs: number, enabled = true) {
  useEffect(() => {
    if (!enabled) return;
    const tick = () => {
      if (document.visibilityState === 'visible') reload();
    };
    const timer = window.setInterval(tick, intervalMs);
    document.addEventListener('visibilitychange', tick);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', tick);
    };
  }, [reload, intervalMs, enabled]);
}
