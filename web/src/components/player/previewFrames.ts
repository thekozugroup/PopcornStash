import {ApiError, notifyUnauthorized} from '../../api/client';

const REQUEST_DELAY_MS = 40;
const BUSY_RETRY_MS = 300;
const MAX_FRAMES = 12;

/** A single mounted player's decoded frames. Nothing is written to browser storage. */
export class PreviewFrames {
  private readonly frames = new Map<number, HTMLImageElement>();
  private readonly listeners = new Set<() => void>();
  private target: number | null = null;
  private failed: number | null = null;
  private retried = false;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private pending: AbortController | null = null;

  constructor(private readonly itemId: string) {}

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  };

  read(second: number | null) {
    if (second === null) return null;
    return this.frames.get(second) ?? (this.failed === second ? 'unavailable' : null);
  }

  select(second: number | null) {
    if (second === this.target) return;
    this.target = second;
    this.failed = null;
    this.retried = false;
    if (second === null) {
      clearTimeout(this.timer);
      this.timer = undefined;
      this.pending?.abort();
      this.pending = null;
      return;
    }

    const cached = this.frames.get(second);
    if (cached) {
      this.frames.delete(second);
      this.frames.set(second, cached);
    } else {
      this.schedule(REQUEST_DELAY_MS);
    }
  }

  clear() {
    this.select(null);
    this.releaseFrames();
  }

  private releaseFrames() {
    for (const image of this.frames.values()) {
      // A hidden page can be frozen before React paints the dismissal.
      image.remove();
      URL.revokeObjectURL(image.src);
    }
    this.frames.clear();
  }

  // Movement updates the target without restarting the timer or competing
  // with the server's one extraction slot. Intermediate misses are skipped.
  private schedule(delay: number) {
    if (this.timer !== undefined || this.pending || this.target === null || this.read(this.target)) return;
    this.timer = setTimeout(() => {
      this.timer = undefined;
      const second = this.target;
      if (second !== null && !this.read(second)) void this.load(second);
    }, delay);
  }

  private async load(second: number) {
    const controller = new AbortController();
    const {signal} = controller;
    this.pending = controller;
    let nextDelay = REQUEST_DELAY_MS;
    let url: string | undefined;
    const revokePendingUrl = () => {
      if (url) URL.revokeObjectURL(url);
      url = undefined;
    };
    try {
      const path = `/api/items/${encodeURIComponent(this.itemId)}/preview?time_s=${second}`;
      const response = await fetch(path, {signal, credentials: 'same-origin', cache: 'no-store'});
      if (signal.aborted) return;
      if (response.status === 409 && (this.target !== second || !this.retried)) {
        this.retried = this.target === second;
        nextDelay = BUSY_RETRY_MS;
        return;
      }
      if (!response.ok) {
        if ([401, 403, 404].includes(response.status)) this.releaseFrames();
        if (response.status === 401) notifyUnauthorized(new ApiError('unauthorized', 'Sign in again.', 401));
        throw new Error('Preview unavailable');
      }
      const blob = await response.blob();
      if (signal.aborted) return;
      const image = new Image();
      url = URL.createObjectURL(blob);
      signal.addEventListener('abort', revokePendingUrl, {once: true});
      image.alt = '';
      image.src = url;
      await image.decode();
      if (signal.aborted) return;
      this.frames.set(second, image);
      url = undefined; // The cache owns the URL until eviction or teardown.
      const oldest = this.frames.entries().next().value;
      if (this.frames.size > MAX_FRAMES && oldest) {
        URL.revokeObjectURL(oldest[1].src);
        this.frames.delete(oldest[0]);
      }
    } catch {
      if (!signal.aborted && this.target === second) this.failed = second;
    } finally {
      signal.removeEventListener('abort', revokePendingUrl);
      revokePendingUrl();
      if (this.pending === controller) {
        this.pending = null;
        this.listeners.forEach(listener => listener());
        this.schedule(nextDelay);
      }
    }
  }
}
