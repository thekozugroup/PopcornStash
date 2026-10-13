/**
 * "Send all my browser downloads to PopcornStash": which Chrome downloads to
 * take over, and how to read the server's first answer about them.
 *
 * Flow (background.ts): Chrome starts the download, the extension pauses it,
 * sends the link to PopcornStash, and waits a few seconds for the server to
 * start fetching. Only then is Chrome's copy cancelled and removed from the
 * downloads list. When anything fails, Chrome's own download resumes.
 */

import {isOnServerHost, type ServerIdentity} from './payload';
import type {Download} from './types';

/** The parts of chrome.downloads.DownloadItem the decision reads. */
export interface DownloadItemLike {
  url: string;
  finalUrl?: string | undefined;
  state: string;
  incognito: boolean;
  byExtensionId?: string | undefined;
}

export type InterceptDecision = {intercept: true; url: string} | {intercept: false; reason: string};

function isHttp(value: string | undefined): boolean {
  if (!value) return false;
  try {
    const url = new URL(value);
    return url.protocol === 'http:' || url.protocol === 'https:';
  } catch {
    return false;
  }
}

export function shouldIntercept(item: DownloadItemLike, server: ServerIdentity): InterceptDecision {
  if (item.state !== 'in_progress') return {intercept: false, reason: 'not in progress'};
  if (item.incognito) return {intercept: false, reason: 'incognito'};
  if (item.byExtensionId) return {intercept: false, reason: 'started by an extension'};
  // blob:, data:, file: and filesystem: only exist in this browser.
  if (!isHttp(item.url)) return {intercept: false, reason: 'not a web link'};
  if (item.finalUrl && item.finalUrl !== item.url && !isHttp(item.finalUrl)) {
    return {intercept: false, reason: 'not a web link'};
  }
  // Files saved from PopcornStash itself stay in Chrome (and must never loop back).
  if (isOnServerHost(item.url, server) || (item.finalUrl && isOnServerHost(item.finalUrl, server))) {
    return {intercept: false, reason: 'from PopcornStash'};
  }
  return {intercept: true, url: item.url};
}

export type ServerProgress = 'started' | 'failed' | 'waiting';

/**
 * Has the server started fetching a web download it accepted? "started"
 * means bytes are arriving or it already finished; "failed" means Chrome
 * should keep its own copy.
 */
export function serverProgress(download: Download | undefined): ServerProgress {
  if (!download) return 'waiting';
  switch (download.state) {
    case 'failed':
    case 'cancelled':
      return 'failed';
    case 'completed':
    case 'sorting':
    case 'sorted':
    case 'seeding':
      return 'started';
    case 'downloading':
      return download.done_bytes > 0 ? 'started' : 'waiting';
    default:
      return 'waiting';
  }
}

/**
 * What to do with Chrome's own copy once PopcornStash has the download, from
 * that copy's current state (undefined when it is gone from Chrome's list).
 */
export function chromeCopyOutcome(state: string | undefined): {cancel: boolean; note: string} {
  switch (state) {
    case 'in_progress':
      return {cancel: true, note: 'Chrome stopped its own copy.'};
    case 'complete':
      return {cancel: false, note: 'Chrome had already finished its copy, so it stays in your Downloads folder too.'};
    default:
      return {cancel: false, note: "Chrome's own copy had already stopped."};
  }
}
