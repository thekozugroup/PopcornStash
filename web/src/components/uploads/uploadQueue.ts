/**
 * The browser upload queue.
 *
 * One queue per tab, kept outside React so uploads keep going while the
 * person browses the library. Each file is a `ResumableUpload`
 * (src/api/upload.ts, 16 MiB chunks). Two files send at a time; the rest wait.
 *
 * Interrupted uploads are remembered in localStorage by file name, size and
 * modification time. Choosing the same file again (after a reload, a lost
 * connection or a closed tab) continues from what the server already holds.
 * Files shared from Android are kept on the device until uploaded
 * (sharedFiles.ts), so those continue by themselves, and their kept copies
 * are deleted once each upload finishes or is cancelled.
 * While anything is sending, closing or reloading the tab asks first.
 *
 * The queue belongs to the person signed in. Remembered uploads carry their
 * user id and continue only for that person; when someone else signs in, or
 * nobody is, the queue is emptied. Signing out deletes what the person left on
 * the device: their partial uploads, the remembered uploads and the files kept
 * from shares (`clearUploadsForSignOut`).
 */
import {useSyncExternalStore} from 'react';
import {ApiError, uploads} from '../../api/client';
import type {Download} from '../../api/types';
import {ResumableUpload, type UploadProgress} from '../../api/upload';
import {errorMessage} from '../../lib/errors';
import {
  deleteOldShares,
  discardAllShares,
  findSharedCopy,
  releaseSharedCopy,
  sharedRecordOf,
  type SharedFileRecord,
} from './sharedFiles';

export type UploadEntryState =
  | 'waiting'
  | 'starting'
  | 'uploading'
  | 'paused'
  | 'finishing'
  | 'failed'
  | 'interrupted';

export interface UploadEntry {
  /** Local key, stable for the life of the entry. */
  key: string;
  /** name|size|lastModified, used to recognise the same file again. */
  fileKey: string;
  name: string;
  size: number;
  state: UploadEntryState;
  /** Bytes acknowledged plus in flight. */
  loaded: number;
  bytesPerSecond: number;
  error: string | null;
  /** Bytes the server already held when this upload continued an earlier one. */
  resumedFrom: number | null;
  uploadId: string | null;
  /** False for interrupted uploads remembered from an earlier visit (the file must be chosen again). */
  hasFile: boolean;
  addedAt: number;
}

interface StoredUpload {
  /** The person who started the upload; it continues only for them. */
  userId: string;
  fileKey: string;
  uploadId: string;
  name: string;
  size: number;
  savedAt: number;
}

export interface AddResult {
  /** New uploads starting from the beginning. */
  started: number;
  /** Uploads continuing from what the server already holds. */
  resumed: number;
  /** Files already in the queue. */
  duplicates: number;
}

const STORAGE_KEY = 'popcornstash.uploads.v1';
const MAX_PARALLEL = 2;
/** Interrupted uploads older than this are forgotten (the server cleans up partial files too). */
const STORED_MAX_AGE_MS = 7 * 24 * 3600 * 1000;

type Listener = () => void;
type UploadedListener = (download: Download, entry: UploadEntry) => void;

const SENDING: ReadonlySet<UploadEntryState> = new Set(['waiting', 'starting', 'uploading', 'finishing']);

let entries: UploadEntry[] = [];
const runners = new Map<string, ResumableUpload>();
const files = new Map<string, File>();
/** Entries sending a file shared from Android, with the kept copy to delete once done. */
const sharedRecords = new Map<string, SharedFileRecord>();
const listeners = new Set<Listener>();
const uploadedListeners = new Set<UploadedListener>();
let restored = false;
let keySeed = 0;
/** The person whose uploads the queue holds, or null before anyone has used it. */
let owner: string | null = null;
/** Bumped each time the queue is emptied; answers that arrive afterwards are dropped. */
let generation = 0;

// ---------------------------------------------------------------------------
// Persistence of interrupted uploads
// ---------------------------------------------------------------------------

/** Remembered uploads of everyone who used this device. */
function readAllStored(): StoredUpload[] {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    const now = Date.now();
    return parsed.filter(
      (value): value is StoredUpload =>
        typeof value === 'object' &&
        value !== null &&
        typeof (value as StoredUpload).userId === 'string' &&
        typeof (value as StoredUpload).fileKey === 'string' &&
        typeof (value as StoredUpload).uploadId === 'string' &&
        typeof (value as StoredUpload).name === 'string' &&
        typeof (value as StoredUpload).size === 'number' &&
        typeof (value as StoredUpload).savedAt === 'number' &&
        now - (value as StoredUpload).savedAt < STORED_MAX_AGE_MS,
    );
  } catch {
    return [];
  }
}

/** Remembered uploads of `userId` only. */
function readStored(userId: string | null): StoredUpload[] {
  return userId === null ? [] : readAllStored().filter(stored => stored.userId === userId);
}

function writeStored(list: StoredUpload[]) {
  try {
    if (list.length === 0) window.localStorage.removeItem(STORAGE_KEY);
    else window.localStorage.setItem(STORAGE_KEY, JSON.stringify(list));
  } catch {
    // Private mode or storage full: resuming after a reload is a convenience.
  }
}

function remember(entry: UploadEntry, uploadId: string) {
  if (owner === null) return;
  const userId = owner;
  const list = readAllStored().filter(stored => !(stored.userId === userId && stored.fileKey === entry.fileKey));
  list.push({userId, fileKey: entry.fileKey, uploadId, name: entry.name, size: entry.size, savedAt: Date.now()});
  writeStored(list);
}

function forget(fileKey: string) {
  if (owner === null) return;
  const userId = owner;
  writeStored(readAllStored().filter(stored => !(stored.userId === userId && stored.fileKey === fileKey)));
}

/**
 * Empties the queue: every upload stops (the server keeps what it has) and the
 * entries go. Runs and server answers still on their way are ignored.
 */
function resetQueue() {
  generation += 1;
  for (const runner of runners.values()) runner.pause();
  entries = [];
  runners.clear();
  files.clear();
  sharedRecords.clear();
  samples.clear();
  pendingOps.clear();
  activeRuns.clear();
  resumeAfterRun.clear();
  restored = false;
  emit();
}

/**
 * Hands the queue to the person signed in (null when nobody is). Uploads of
 * someone else are stopped and dropped from the queue; their remembered
 * uploads stay on the device for them.
 */
export function setUploadUser(userId: string | null): void {
  if (owner !== null && owner !== userId) resetQueue();
  owner = userId;
}

/**
 * Signing out on this device: stops every upload and deletes what the person
 * leaves behind (their partial uploads on the server, their remembered
 * uploads and the files kept from shares), so none of it reaches whoever
 * signs in next. Call it while still signed in.
 */
export async function clearUploadsForSignOut(): Promise<void> {
  const userId = owner;
  const partial = new Set<string>();
  for (const entry of entries) if (entry.uploadId) partial.add(entry.uploadId);
  for (const stored of readStored(userId)) partial.add(stored.uploadId);
  resetQueue();
  if (userId !== null) writeStored(readAllStored().filter(stored => stored.userId !== userId));
  owner = null;
  await Promise.allSettled([...[...partial].map(id => uploads.cancel(id)), discardAllShares()]);
}

// ---------------------------------------------------------------------------
// Store plumbing
// ---------------------------------------------------------------------------

function emit() {
  syncUnloadGuard();
  for (const listener of listeners) listener();
}

function update(key: string, patch: Partial<UploadEntry>) {
  let changed = false;
  entries = entries.map(entry => {
    if (entry.key !== key) return entry;
    changed = true;
    return {...entry, ...patch};
  });
  if (changed) emit();
}

function removeEntry(key: string) {
  samples.delete(key);
  sharedRecords.delete(key);
  entries = entries.filter(entry => entry.key !== key);
  runners.delete(key);
  files.delete(key);
  emit();
}

function find(key: string): UploadEntry | undefined {
  return entries.find(entry => entry.key === key);
}

export function fileKeyOf(file: File): string {
  return `${file.name}|${file.size}|${file.lastModified}`;
}

function nextKey(): string {
  keySeed += 1;
  return `upload-${Date.now().toString(36)}-${keySeed}`;
}

// ---------------------------------------------------------------------------
// Leaving the page while files send
// ---------------------------------------------------------------------------

let unloadGuardOn = false;

function onBeforeUnload(event: BeforeUnloadEvent) {
  event.preventDefault();
  // Older browsers need returnValue set to show the prompt.
  event.returnValue = '';
}

function syncUnloadGuard() {
  const sending = entries.some(entry => SENDING.has(entry.state));
  if (sending && !unloadGuardOn) {
    window.addEventListener('beforeunload', onBeforeUnload);
    unloadGuardOn = true;
  } else if (!sending && unloadGuardOn) {
    window.removeEventListener('beforeunload', onBeforeUnload);
    unloadGuardOn = false;
  }
}

// ---------------------------------------------------------------------------
// Running uploads
// ---------------------------------------------------------------------------

/**
 * Transfer rate per upload, measured here rather than taken from the
 * uploader: its first sample after continuing an upload counts the bytes the
 * server already held as if they were sent in a moment.
 */
const samples = new Map<string, {time: number; loaded: number; rate: number}>();

function measureRate(key: string, loaded: number, sending: boolean): number {
  const now = performance.now();
  const previous = samples.get(key);
  if (!sending || !previous) {
    samples.set(key, {time: now, loaded, rate: 0});
    return 0;
  }
  const seconds = (now - previous.time) / 1000;
  if (seconds < 0.5) return previous.rate;
  const instant = Math.max(0, loaded - previous.loaded) / seconds;
  const rate = previous.rate === 0 ? instant : previous.rate * 0.7 + instant * 0.3;
  samples.set(key, {time: now, loaded, rate});
  return rate;
}

function onProgress(key: string, progress: UploadProgress) {
  const entry = find(key);
  if (!entry) return;
  const isPaused = entry.state === 'paused';
  // The first "uploading" event after starting carries what the server already
  // held; it only sets the baseline, so continuing an upload shows no spike.
  const sending = !isPaused && progress.phase === 'uploading' && entry.state === 'uploading';
  const patch: Partial<UploadEntry> = {
    loaded: progress.loaded,
    bytesPerSecond: measureRate(key, progress.loaded, sending),
  };
  if (!isPaused && (progress.phase === 'uploading' || progress.phase === 'finishing' || progress.phase === 'starting')) {
    patch.state = progress.phase;
  }
  if (progress.uploadId) patch.uploadId = progress.uploadId;
  // The first progress after continuing an earlier upload carries what the server already had.
  if (entry.resumedFrom === -1 && progress.phase === 'uploading') {
    patch.resumedFrom = progress.loaded > 0 ? progress.loaded : null;
  }
  update(key, patch);
}

/**
 * Pause and cancel requested while the upload is still being created are
 * applied once the first chunk is on its way. `ResumableUpload` only notices
 * an abort while a request is in flight, so aborting earlier would let the
 * chunks go out anyway. The microtask runs right after the chunk request is
 * opened, which is synchronous with the "uploading" progress event.
 */
const pendingOps = new Map<ResumableUpload, 'pause' | 'cancel'>();

/**
 * Entries whose `run()` has not settled yet. A runner is never started twice
 * at once: resuming while its earlier run is still settling either cancels a
 * pending pause or waits for the run to finish (`resumeAfterRun`).
 */
const activeRuns = new Set<string>();
const resumeAfterRun = new Set<string>();

function applyPending(runner: ResumableUpload) {
  const op = pendingOps.get(runner);
  if (!op) return;
  pendingOps.delete(runner);
  queueMicrotask(() => {
    if (op === 'pause') runner.pause();
    else void runner.cancel();
  });
}

async function run(key: string) {
  const runner = runners.get(key);
  if (!runner || activeRuns.has(key)) return;
  const started = generation;
  activeRuns.add(key);
  update(key, {state: 'starting', error: null});
  try {
    const download = await runner.start();
    if (started !== generation) return;
    const entry = find(key);
    forget(entry?.fileKey ?? fileKeyOf(runner.file));
    releaseShared(key);
    if (entry) {
      for (const listener of uploadedListeners) listener(download, entry);
    }
    removeEntry(key);
  } catch (error) {
    const entry = find(key);
    if (!entry) return;
    if (error instanceof ApiError && error.code === 'aborted') {
      // Paused (the entry state is already set) or cancelled (already removed).
      // Resumed while this run was stopping: it starts again below.
      if (resumeAfterRun.has(key)) update(key, {state: 'waiting', bytesPerSecond: 0});
      else if (entry.state !== 'paused') update(key, {state: 'paused', bytesPerSecond: 0});
    } else {
      update(key, {
        state: 'failed',
        bytesPerSecond: 0,
        error: errorMessage(error, 'The upload stopped. Try again.'),
      });
    }
  } finally {
    if (started === generation) {
      activeRuns.delete(key);
      resumeAfterRun.delete(key);
      pendingOps.delete(runner);
      pump();
    }
  }
}

function pump() {
  let slots = MAX_PARALLEL - activeRuns.size;
  if (slots <= 0) return;
  for (const entry of entries) {
    if (slots <= 0) break;
    if (entry.state !== 'waiting' || !runners.has(entry.key) || activeRuns.has(entry.key)) continue;
    slots -= 1;
    void run(entry.key);
  }
}

function makeRunner(key: string, file: File, resumeId: string | undefined): ResumableUpload {
  const runner: ResumableUpload = new ResumableUpload(file, {
    resumeId,
    onProgress: progress => {
      if (progress.phase === 'uploading') applyPending(runner);
      onProgress(key, progress);
    },
    onCreated: uploadId => {
      const entry = find(key);
      if (entry) {
        remember(entry, uploadId);
        update(key, {uploadId});
      }
    },
  });
  runners.set(key, runner);
  files.set(key, file);
  return runner;
}

/** Deletes the kept copy of a shared file once its upload is done or cancelled. */
function releaseShared(key: string) {
  const record = sharedRecords.get(key);
  if (!record) return;
  sharedRecords.delete(key);
  void releaseSharedCopy(record);
}

/**
 * An interrupted upload of a file shared from Android continues from the copy
 * kept on this device. Resolves false when there is no such copy.
 */
async function continueFromSharedCopy(entry: UploadEntry, userId: string): Promise<boolean> {
  const started = generation;
  const file = await findSharedCopy(entry.fileKey);
  if (!file) return false;
  // Unless it was discarded or chosen again meanwhile, or someone else signed in.
  // A server that no longer has the partial upload gets a new one (ResumableUpload
  // starts over).
  const current = find(entry.key);
  if (started === generation && current && !current.hasFile) addFiles([file], userId);
  return true;
}

/** Asks the server whether an interrupted upload can still continue, and drops it when not. */
function checkWithServer(entry: UploadEntry) {
  if (!entry.uploadId) return;
  const started = generation;
  uploads
    .status(entry.uploadId)
    .then(status => {
      if (started !== generation || find(entry.key)?.hasFile) return;
      if (status.state !== 'receiving' || status.size !== entry.size) {
        forget(entry.fileKey);
        removeEntry(entry.key);
        return;
      }
      update(entry.key, {loaded: status.received});
    })
    .catch((error: unknown) => {
      // Gone on the server: nothing left to continue.
      if (started !== generation || find(entry.key)?.hasFile) return;
      if (error instanceof ApiError && (error.status === 404 || error.status === 410)) {
        forget(entry.fileKey);
        removeEntry(entry.key);
      }
    });
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Loads the interrupted uploads `userId` started earlier on this device and
 * checks them with the server. Their shared files kept on this device
 * continue by themselves; other people's uploads never do.
 */
export function restoreInterrupted(userId: string): void {
  if (typeof window === 'undefined') return;
  setUploadUser(userId);
  if (restored) return;
  restored = true;
  void deleteOldShares();
  const stored = readStored(userId);
  if (stored.length === 0) return;
  const known = new Set(entries.map(entry => entry.fileKey));
  const fresh: UploadEntry[] = stored
    .filter(record => !known.has(record.fileKey))
    .map(record => ({
      key: nextKey(),
      fileKey: record.fileKey,
      name: record.name,
      size: record.size,
      state: 'interrupted' as const,
      loaded: 0,
      bytesPerSecond: 0,
      error: null,
      resumedFrom: null,
      uploadId: record.uploadId,
      hasFile: false,
      addedAt: record.savedAt,
    }));
  if (fresh.length === 0) return;
  entries = [...entries, ...fresh];
  emit();
  for (const entry of fresh) {
    void continueFromSharedCopy(entry, userId).then(continued => {
      if (!continued) checkWithServer(entry);
    });
  }
}

/**
 * Queues files for upload by `userId`. Files that match an interrupted upload
 * of theirs continue it.
 */
export function addFiles(list: File[], userId: string): AddResult {
  setUploadUser(userId);
  const result: AddResult = {started: 0, resumed: 0, duplicates: 0};
  const stored = readStored(userId);
  for (const file of list) {
    const fileKey = fileKeyOf(file);
    const existing = entries.find(entry => entry.fileKey === fileKey);
    const sharedRecord = sharedRecordOf(file);
    if (existing && existing.hasFile) {
      // The same file shared again: its kept copy goes once the upload already running is done.
      if (sharedRecord && !sharedRecords.has(existing.key)) sharedRecords.set(existing.key, sharedRecord);
      result.duplicates += 1;
      continue;
    }
    const resumeId = existing?.uploadId ?? stored.find(record => record.fileKey === fileKey)?.uploadId;
    const key = existing?.key ?? nextKey();
    const entry: UploadEntry = {
      key,
      fileKey,
      name: file.name,
      size: file.size,
      state: 'waiting',
      loaded: existing?.loaded ?? 0,
      bytesPerSecond: 0,
      error: null,
      // -1 marks "find out on the first progress event".
      resumedFrom: resumeId ? -1 : null,
      uploadId: resumeId ?? null,
      hasFile: true,
      addedAt: existing?.addedAt ?? Date.now(),
    };
    makeRunner(key, file, resumeId);
    if (sharedRecord) sharedRecords.set(key, sharedRecord);
    if (existing) {
      entries = entries.map(current => (current.key === key ? entry : current));
    } else {
      entries = [...entries, entry];
    }
    if (resumeId) result.resumed += 1;
    else result.started += 1;
  }
  emit();
  pump();
  return result;
}

/** Stops sending; the server keeps what it has. */
export function pauseUpload(key: string): void {
  const entry = find(key);
  if (!entry) return;
  const runner = runners.get(key);
  const wasState = entry.state;
  resumeAfterRun.delete(key);
  update(key, {state: 'paused', bytesPerSecond: 0});
  if (!runner || wasState === 'waiting' || wasState === 'paused') return;
  if (wasState === 'starting') pendingOps.set(runner, 'pause');
  else runner.pause();
}

/** Continues a paused or failed upload. */
export function resumeUpload(key: string): void {
  const entry = find(key);
  const runner = runners.get(key);
  if (!entry || !runner) return;
  if (activeRuns.has(key)) {
    if (pendingOps.get(runner) === 'pause') {
      // Paused while the upload was still being created: simply carry on.
      pendingOps.delete(runner);
      update(key, {state: 'starting', error: null});
    } else {
      // The earlier run is still stopping; start again once it has.
      resumeAfterRun.add(key);
      update(key, {state: 'waiting', error: null});
    }
    return;
  }
  update(key, {state: 'waiting', error: null});
  pump();
}

/** Stops and deletes the partial file on the server. */
export async function cancelUpload(key: string): Promise<void> {
  const entry = find(key);
  if (!entry) return;
  const runner = runners.get(key);
  forget(entry.fileKey);
  releaseShared(key);
  removeEntry(key);
  if (runner && entry.state === 'starting') {
    // Still being created: cancel once it exists, so the server copy is deleted too.
    pendingOps.set(runner, 'cancel');
  } else if (runner) {
    await runner.cancel();
  } else if (entry.uploadId) {
    try {
      await uploads.cancel(entry.uploadId);
    } catch {
      // Already gone; nothing else to clean up.
    }
  }
  pump();
}

/** Called with the server's Download each time a file finishes uploading. */
export function onUploaded(listener: UploadedListener): () => void {
  uploadedListeners.add(listener);
  return () => uploadedListeners.delete(listener);
}

function subscribe(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

const EMPTY: UploadEntry[] = [];

/** Live list of queued, running, paused, failed and interrupted uploads. */
export function useUploadQueue(): UploadEntry[] {
  return useSyncExternalStore(subscribe, () => entries, () => EMPTY);
}

/** True while at least one file is being sent. */
export function isSending(entry: UploadEntry): boolean {
  return SENDING.has(entry.state);
}
