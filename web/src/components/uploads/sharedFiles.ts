/**
 * Files shared to PopcornStash from Android's share sheet.
 *
 * The service worker (public/sw.js) receives the share, keeps each file in
 * Cache Storage on this device with a note listing them, and opens
 * `/share?files=<id>`. This module reads them back for the share page, and
 * deletes each kept copy once its upload has finished or was cancelled (the
 * upload queue calls `releaseSharedCopy`), or the whole share on Cancel.
 * While a copy is kept, an upload of it that a reload cut off continues by
 * itself (`findSharedCopy`).
 *
 * Every function here is safe to call where Cache Storage is missing (an
 * insecure origin) or fails: it then finds nothing and keeps nothing.
 */

/** Keep in step with public/sw.js. */
const SHARE_CACHE = 'popcornstash-shared-files-v1';
const SHARE_PREFIX = '/shared-files/';
const SHARE_MAX_AGE_MS = 7 * 24 * 3600 * 1000;
const LIST_NAME = 'share.json';
const SHARE_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

export interface SharedFileRecord {
  /** Cache key of the kept copy (`/shared-files/<id>/<n>`). */
  key: string;
  name: string;
  type: string;
  size: number;
  lastModified: number;
}

interface ShareList {
  id: string;
  savedAt: number;
  files: SharedFileRecord[];
}

export interface SharedFile {
  record: SharedFileRecord;
  file: File;
}

export interface Share {
  id: string;
  savedAt: number;
  /** The files still kept, in the order they were shared. */
  files: SharedFile[];
}

/** Files read back from a share, so the upload queue can tell them apart. */
const sharedCopies = new WeakMap<File, SharedFileRecord>();

export function isShareId(value: string | null): value is string {
  return value !== null && SHARE_ID.test(value);
}

async function openCache(): Promise<Cache | null> {
  if (typeof caches === 'undefined') return null;
  try {
    if (!(await caches.has(SHARE_CACHE))) return null;
    return await caches.open(SHARE_CACHE);
  } catch {
    return null;
  }
}

function listKey(id: string): string {
  return `${SHARE_PREFIX}${id}/${LIST_NAME}`;
}

function shareIdOf(key: string): string | null {
  const id = key.slice(SHARE_PREFIX.length).split('/')[0] ?? '';
  return key.startsWith(SHARE_PREFIX) && isShareId(id) ? id : null;
}

function isRecord(value: unknown, id: string): value is SharedFileRecord {
  const record = value as SharedFileRecord;
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof record.key === 'string' &&
    record.key.startsWith(`${SHARE_PREFIX}${id}/`) &&
    typeof record.name === 'string' &&
    typeof record.type === 'string' &&
    typeof record.size === 'number' &&
    typeof record.lastModified === 'number'
  );
}

async function readList(cache: Cache, id: string): Promise<ShareList | null> {
  try {
    const response = await cache.match(listKey(id));
    if (!response) return null;
    const value = (await response.json()) as Partial<ShareList>;
    if (typeof value.savedAt !== 'number' || !Array.isArray(value.files)) return null;
    return {id, savedAt: value.savedAt, files: value.files.filter(record => isRecord(record, id))};
  } catch {
    return null;
  }
}

/** `name|size|lastModified`, the upload queue's way of recognising a file. */
function fileKeyOfRecord(record: SharedFileRecord): string {
  return `${record.name}|${record.size}|${record.lastModified}`;
}

async function readFile(cache: Cache, record: SharedFileRecord): Promise<File | null> {
  try {
    const response = await cache.match(record.key);
    if (!response) return null;
    const blob = await response.blob();
    const file = new File([blob], record.name, {type: record.type || blob.type, lastModified: record.lastModified});
    sharedCopies.set(file, record);
    return file;
  } catch {
    return null;
  }
}

async function deleteShare(cache: Cache, id: string): Promise<void> {
  const prefix = `${SHARE_PREFIX}${id}/`;
  const requests = await cache.keys();
  // Files first and the list last, so a list never points at nothing it once held.
  const files = requests.filter(request => {
    const path = new URL(request.url).pathname;
    return path.startsWith(prefix) && !path.endsWith(`/${LIST_NAME}`);
  });
  await Promise.all(files.map(request => cache.delete(request)));
  await cache.delete(listKey(id));
}

/** The files of a share that are still kept here, or null when none are left. */
export async function loadShare(id: string): Promise<Share | null> {
  if (!isShareId(id)) return null;
  const cache = await openCache();
  if (!cache) return null;
  const list = await readList(cache, id);
  if (!list) return null;
  if (Date.now() - list.savedAt >= SHARE_MAX_AGE_MS) {
    await deleteShare(cache, id).catch(() => undefined);
    return null;
  }
  const files: SharedFile[] = [];
  for (const record of list.files) {
    const file = await readFile(cache, record);
    if (file) files.push({record, file});
  }
  if (files.length === 0) {
    await cache.delete(listKey(id)).catch(() => undefined);
    return null;
  }
  return {id, savedAt: list.savedAt, files};
}

/** Deletes everything kept for a share (Cancel on the share page). */
export async function discardShare(id: string): Promise<void> {
  if (!isShareId(id)) return;
  const cache = await openCache();
  if (cache) await deleteShare(cache, id).catch(() => undefined);
}

/** The kept copy behind a file read from a share, if it is one. */
export function sharedRecordOf(file: File): SharedFileRecord | undefined {
  return sharedCopies.get(file);
}

/**
 * Deletes one kept copy after its upload finished or was cancelled, and the
 * share's list once nothing in it is kept any more.
 */
export async function releaseSharedCopy(record: SharedFileRecord): Promise<void> {
  const id = shareIdOf(record.key);
  if (!id) return;
  const cache = await openCache();
  if (!cache) return;
  try {
    await cache.delete(record.key);
    const list = await readList(cache, id);
    const left = list ? await Promise.all(list.files.map(file => cache.match(file.key))) : [];
    if (left.every(response => !response)) await cache.delete(listKey(id));
  } catch {
    // Old shares are deleted after a week in any case.
  }
}

/**
 * A kept copy of the file the upload queue knows as `fileKey`, so an upload
 * that a reload cut off can continue without choosing the file again.
 */
export async function findSharedCopy(fileKey: string): Promise<File | null> {
  const cache = await openCache();
  if (!cache) return null;
  try {
    const requests = await cache.keys();
    for (const request of requests) {
      const id = shareIdOf(new URL(request.url).pathname);
      if (!id || !new URL(request.url).pathname.endsWith(`/${LIST_NAME}`)) continue;
      const list = await readList(cache, id);
      if (!list || Date.now() - list.savedAt >= SHARE_MAX_AGE_MS) continue;
      const record = list.files.find(candidate => fileKeyOfRecord(candidate) === fileKey);
      if (record) {
        const file = await readFile(cache, record);
        if (file) return file;
      }
    }
  } catch {
    // Nothing to continue from.
  }
  return null;
}

/** Deletes shares that were never uploaded or cancelled within a week. */
export async function deleteOldShares(): Promise<void> {
  const cache = await openCache();
  if (!cache) return;
  try {
    const requests = await cache.keys();
    for (const request of requests) {
      const path = new URL(request.url).pathname;
      const id = shareIdOf(path);
      if (!id || !path.endsWith(`/${LIST_NAME}`)) continue;
      const list = await readList(cache, id);
      if (!list || Date.now() - list.savedAt >= SHARE_MAX_AGE_MS) await deleteShare(cache, id);
    }
  } catch {
    // Tried again on the next visit.
  }
}
