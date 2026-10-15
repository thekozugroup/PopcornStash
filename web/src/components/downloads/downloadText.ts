/**
 * Downloads in plain words: kind, state, and where finished files went.
 */
import type {Download, DownloadKind, DownloadState, Library} from '../../api/types';
import {formatCount} from '../../lib/format';
import {libraryHref} from '../../lib/libraries';

export const KIND_LABEL: Record<DownloadKind, string> = {
  torrent: 'Torrent',
  http: 'Web link',
  upload: 'Upload',
};

export const ACTIVE_STATES: ReadonlySet<DownloadState> = new Set(['queued', 'downloading', 'paused', 'seeding']);

export function isActive(download: Download): boolean {
  return ACTIVE_STATES.has(download.state);
}

/** A magnet has no file list yet; zero speed does not mean an idle transfer. */
export function fetchingTorrentDetails(download: Download): boolean {
  return download.kind === 'torrent' && download.state === 'downloading'
    && (download.total_bytes === null || download.total_bytes <= 0) && download.done_bytes === 0;
}

/** One short phrase for the state of an in-progress download. */
export function activeStateWords(download: Download): string {
  switch (download.state) {
    case 'queued':
      return download.error ?? 'Waiting to start';
    case 'downloading':
      if (fetchingTorrentDetails(download)) return 'Fetching torrent details';
      if (download.kind === 'torrent' && !download.peers && download.down_bps === 0) return 'Looking for peers';
      return 'Downloading';
    case 'paused':
      return 'Paused';
    case 'seeding':
      return 'Downloaded. Sharing with peers before sorting';
    default:
      return 'Downloading';
  }
}

/** Can this download be paused or resumed from the row? */
export function canPause(download: Download): boolean {
  return download.state === 'downloading' || download.state === 'queued' || download.state === 'seeding';
}

export function canResume(download: Download): boolean {
  return download.state === 'paused';
}

/** Retry is offered when the source can be fetched again: a link or a magnet. */
export function canRetry(download: Download): boolean {
  if (download.state !== 'failed') return false;
  if (download.kind === 'upload') return false;
  return /^(https?:|magnet:)/i.test(download.source);
}

/** A failed browser upload can only be fixed by choosing the file again. */
export function canUploadAgain(download: Download): boolean {
  return download.state === 'failed' && download.kind === 'upload';
}

/** Sorted downloads already live in the library; their files must not be offered for deletion. */
export function filesStillInDownloads(download: Download): boolean {
  return download.state !== 'sorted' && download.state !== 'cancelled';
}

/** One place a sorted download's files are now: "In Kids" (a link to the library). */
export interface OutcomePlace {
  label: string;
  href: string;
}

export type FinishedOutcome =
  | {kind: 'sorted'; text: string; places: OutcomePlace[]}
  | {kind: 'review'; text: string; href: string}
  | {kind: 'waiting'; text: string}
  | {kind: 'sorting'; text: string}
  | {kind: 'nothing'; text: string}
  | {kind: 'failed'; text: string}
  | {kind: 'cancelled'; text: string}
  | {kind: 'unknown'; text: string};

/**
 * Where a finished download's files went, from the server's `destinations`
 * (only places this person can see). `isOwner` decides whether folder names
 * may appear. A sorted download with nothing left to show says only "Sorted".
 */
export function finishedOutcome(
  download: Download,
  find: (id: string | null) => Library | undefined,
  isOwner: boolean,
): FinishedOutcome {
  switch (download.state) {
    case 'failed':
      return {kind: 'failed', text: download.error ?? 'The download stopped with an error.'};
    case 'cancelled':
      return {kind: 'cancelled', text: 'Cancelled before it finished.'};
    case 'completed':
      return {
        kind: 'waiting',
        text: isOwner ? 'Finished. Waiting in Downloads/Completed to be sorted.' : 'Finished. Waiting to be sorted.',
      };
    case 'sorting':
      return {kind: 'sorting', text: 'Sorting into your library.'};
    default:
      break;
  }

  const destinations = download.destinations ?? [];
  if (destinations.length === 0) {
    // Owners see every file, so nothing to show means nothing came in. Anyone else may
    // have had it sorted into a library they cannot see: that is only ever "Sorted".
    return isOwner && download.item_ids.length === 0
      ? {kind: 'nothing', text: 'No videos or photos inside. The files stay in Downloads/Completed.'}
      : {kind: 'sorted', text: 'Sorted', places: []};
  }

  const waiting = destinations
    .filter(destination => destination.place === 'incoming')
    .reduce((sum, destination) => sum + destination.count, 0);
  if (waiting > 0) {
    return {kind: 'review', text: `${formatCount(waiting, 'file')} waiting in Inbox`, href: '/inbox'};
  }

  const places: OutcomePlace[] = [];
  for (const destination of destinations) {
    if (destination.place === 'other_files') {
      places.push({label: 'Other files', href: '/other'});
      continue;
    }
    const library = find(destination.library_id);
    // A library this person cannot open is never named.
    if (library) places.push({label: library.name, href: libraryHref(library.id)});
  }
  if (places.length === 0) return {kind: 'sorted', text: 'Sorted', places: []};
  return {kind: 'sorted', text: 'In', places};
}
