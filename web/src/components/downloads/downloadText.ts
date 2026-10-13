/**
 * Downloads in plain words: kind, state, and where finished files went.
 */
import type {Download, DownloadKind, DownloadState, ItemDetail} from '../../api/types';
import {formatCount} from '../../lib/format';
import {CATEGORY_FOLDER, itemTitle} from '../inbox/places';

export const KIND_LABEL: Record<DownloadKind, string> = {
  torrent: 'Torrent',
  http: 'Web link',
  upload: 'Upload',
};

export const ACTIVE_STATES: ReadonlySet<DownloadState> = new Set(['queued', 'downloading', 'paused', 'seeding']);

export function isActive(download: Download): boolean {
  return ACTIVE_STATES.has(download.state);
}

/** One short phrase for the state of an in-progress download. */
export function activeStateWords(download: Download): string {
  switch (download.state) {
    case 'queued':
      return download.error ?? 'Waiting to start';
    case 'downloading':
      if (download.kind === 'torrent' && download.peers === 0) return 'Looking for peers';
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

export type FinishedOutcome =
  | {kind: 'sorted'; text: string; linkLabel: string | null; href: string | null}
  | {kind: 'review'; text: string; href: string}
  | {kind: 'waiting'; text: string}
  | {kind: 'sorting'; text: string}
  | {kind: 'nothing'; text: string}
  | {kind: 'failed'; text: string}
  | {kind: 'cancelled'; text: string}
  | {kind: 'unknown'; text: string};

/**
 * Where a finished download's files went, from the download and whatever of
 * its items have loaded. `isOwner` decides whether folder names may appear.
 */
export function finishedOutcome(
  download: Download,
  items: (ItemDetail | undefined)[],
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

  if (download.item_ids.length === 0) {
    return {
      kind: 'nothing',
      text: isOwner
        ? 'No videos or photos inside. The files stay in Downloads/Completed.'
        : 'No videos or photos inside, so nothing was added to the library.',
    };
  }

  const loaded = items.filter((item): item is ItemDetail => item !== undefined);
  if (loaded.some(item => item.status === 'needs_review')) {
    return {kind: 'review', text: 'Waiting for a decision', href: '/inbox'};
  }
  if (loaded.length === 0) return {kind: 'unknown', text: 'Sorted into your library.'};

  const folders = Array.from(new Set(loaded.map(item => CATEGORY_FOLDER[item.category])));
  const into = folders.length === 1 ? folders[0] : `${folders.slice(0, -1).join(', ')} and ${folders[folders.length - 1]}`;
  const text = `Sorted into ${into ?? 'your library'}`;
  const first = loaded[0];
  if (!first) return {kind: 'sorted', text, linkLabel: null, href: null};

  const total = download.item_ids.length;
  if (total === 1) {
    return {kind: 'sorted', text, linkLabel: itemTitle(first), href: `/item/${encodeURIComponent(first.id)}`};
  }
  const shows = new Set(loaded.map(item => item.show_name));
  if (first.category === 'episode' && first.show_name && shows.size === 1 && folders.length === 1) {
    return {
      kind: 'sorted',
      text,
      linkLabel: `${formatCount(total, 'episode')} of ${first.show_name}`,
      href: `/shows/${encodeURIComponent(first.show_name)}`,
    };
  }
  if (first.category === 'photo' && folders.length === 1) {
    return {kind: 'sorted', text, linkLabel: formatCount(total, 'photo'), href: '/photos'};
  }
  return {kind: 'sorted', text, linkLabel: formatCount(total, 'file'), href: `/item/${encodeURIComponent(first.id)}`};
}
