/** Downloads in plain words for the popup list (mirrors the web app's phrasing). */

import type {Download} from './types';

const ACTIVE = new Set<Download['state']>(['queued', 'downloading', 'paused', 'seeding']);

export function isActive(download: Download): boolean {
  return ACTIVE.has(download.state);
}

/** 0..100, rounded down so "100%" only shows when it is really done. */
export function percent(download: Download): number {
  const value = Number.isFinite(download.progress) ? download.progress : 0;
  return Math.max(0, Math.min(100, Math.floor(value * 100)));
}

/** The state as a short phrase, with the percentage while bytes are moving. */
export function stateWords(download: Download): string {
  switch (download.state) {
    case 'queued':
      return download.error ?? 'Waiting to start';
    case 'downloading':
      if (download.kind === 'torrent' && download.peers === 0) return `Looking for peers · ${percent(download)}%`;
      return `Downloading · ${percent(download)}%`;
    case 'paused':
      return `Paused · ${percent(download)}%`;
    case 'seeding':
      return 'Downloaded. Sharing before sorting';
    case 'completed':
      return 'Finished. Waiting to be sorted';
    case 'sorting':
      return 'Sorting into your library';
    case 'sorted':
      return 'In your library';
    case 'failed':
      return download.error ? `Failed: ${download.error}` : 'Failed';
    case 'cancelled':
      return 'Cancelled';
    default:
      return 'Downloading';
  }
}

/** The progress bar shows only while bytes are moving or paused part-way. */
export function showsProgress(download: Download): boolean {
  return download.state === 'downloading' || download.state === 'paused';
}

/**
 * One compact popup row: the name with a short state beside it, a progress
 * bar while bytes move, and a second line only for a failure's reason.
 */
export interface DownloadRow {
  name: string;
  /** Short state beside the name: "64%", "Paused", "In library". */
  meta: string;
  /** The failure reason, shown under the name. */
  detail: string | null;
  /** 0..100 for the progress bar, or null for no bar. */
  bar: number | null;
  error: boolean;
  /** The whole state as a sentence, for screen readers and the tooltip. */
  description: string;
}

/**
 * Why a queued torrent waits, in a few words. The server's `error` says it in
 * full (architecture section 11): "Waiting for the VPN to connect." or
 * "Waiting for a free slot (N at once)."
 */
function waitingState(download: Download): string {
  const reason = download.kind === 'torrent' ? (download.error ?? '') : '';
  if (reason.startsWith('Waiting for a free slot')) return 'Waiting for a slot';
  if (reason.startsWith('Waiting for the VPN')) return 'Waiting for VPN';
  return 'Waiting';
}

function shortState(download: Download): string {
  const value = percent(download);
  switch (download.state) {
    case 'queued':
      return waitingState(download);
    case 'downloading':
      return download.kind === 'torrent' && download.peers === 0 ? 'Finding peers' : `${value}%`;
    case 'paused':
      return `Paused ${value}%`;
    case 'seeding':
      return 'Sharing';
    case 'completed':
      return 'Finished';
    case 'sorting':
      return 'Sorting';
    case 'sorted':
      return 'In library';
    case 'failed':
      return 'Failed';
    case 'cancelled':
      return 'Cancelled';
    default:
      return '';
  }
}

export function downloadRow(download: Download): DownloadRow {
  const name = download.name || download.source;
  const failed = download.state === 'failed';
  return {
    name,
    meta: shortState(download),
    detail: failed ? (download.error ?? null) : null,
    bar: showsProgress(download) ? percent(download) : null,
    error: failed,
    description: stateWords(download),
  };
}
