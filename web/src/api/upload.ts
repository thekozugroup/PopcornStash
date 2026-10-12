/**
 * Resumable chunked uploads for the `/api/uploads` protocol
 * (architecture section 8):
 *
 *   POST   /api/uploads               {filename, size, mime?, last_modified?} -> {id, chunk_size, received}
 *   PUT    /api/uploads/:id?offset=N  raw bytes (<= 64 MiB)                     -> {received}  (409 {received} on mismatch)
 *   GET    /api/uploads/:id                                                    -> {id, filename, size, received, state}
 *   POST   /api/uploads/:id/complete                                           -> Download
 *   DELETE /api/uploads/:id                                                    -> 204
 *
 * Chunks are 16 MiB (the server's `chunk_size`). After a dropped connection
 * or an offset mismatch the uploader asks the server how much it holds and
 * continues from there. Chunks go over XHR so progress is reported within a
 * chunk, not only between chunks.
 */
import {
  ApiError,
  FALLBACK_MESSAGES,
  STATUS_CODES,
  isErrorEnvelope,
  notifyUnauthorized,
  uploads,
} from './client';
import type {Download, UploadCreated} from './types';

export const UPLOAD_CHUNK_SIZE = 16 * 1024 * 1024;
const MAX_CHUNK_SIZE = 64 * 1024 * 1024;
const DEFAULT_RETRIES = 6;

export type UploadPhase = 'starting' | 'uploading' | 'finishing' | 'done' | 'paused' | 'cancelled' | 'failed';

export interface UploadProgress {
  uploadId: string | null;
  phase: UploadPhase;
  /** Bytes the server has acknowledged plus bytes in flight. */
  loaded: number;
  total: number;
  /** 0..1 */
  fraction: number;
  /** Smoothed transfer rate. */
  bytesPerSecond: number;
}

export interface UploadOptions {
  /** Resume an upload created earlier (for example after a reload). */
  resumeId?: string;
  onProgress?: (progress: UploadProgress) => void;
  /** Called once the server has assigned an id, so callers can persist it for resuming. */
  onCreated?: (uploadId: string) => void;
  /** Attempts per chunk before giving up. */
  retries?: number;
}

function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(resolve, ms);
    signal.addEventListener(
      'abort',
      () => {
        clearTimeout(timer);
        reject(new ApiError('aborted', FALLBACK_MESSAGES.aborted ?? '', 0));
      },
      {once: true},
    );
  });
}

/** PUT one chunk with XHR so the browser reports byte progress. */
function putChunk(
  uploadId: string,
  offset: number,
  chunk: Blob,
  signal: AbortSignal,
  onBytes: (sent: number) => void,
): Promise<number> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open('PUT', `/api/uploads/${encodeURIComponent(uploadId)}?offset=${offset}`);
    xhr.withCredentials = true;
    xhr.responseType = 'text';
    xhr.setRequestHeader('Content-Type', 'application/octet-stream');
    xhr.setRequestHeader('Accept', 'application/json');

    const onAbort = () => xhr.abort();
    signal.addEventListener('abort', onAbort, {once: true});
    const cleanup = () => signal.removeEventListener('abort', onAbort);

    xhr.upload.onprogress = event => onBytes(event.loaded);
    xhr.onabort = () => {
      cleanup();
      reject(new ApiError('aborted', FALLBACK_MESSAGES.aborted ?? '', 0));
    };
    xhr.onerror = () => {
      cleanup();
      reject(new ApiError('network', FALLBACK_MESSAGES.network ?? '', 0));
    };
    xhr.onload = () => {
      cleanup();
      let body: unknown;
      try {
        body = xhr.responseText ? (JSON.parse(xhr.responseText) as unknown) : undefined;
      } catch {
        body = undefined;
      }
      if (xhr.status >= 200 && xhr.status < 300) {
        const received = (body as {received?: unknown} | undefined)?.received;
        resolve(typeof received === 'number' ? received : offset + chunk.size);
        return;
      }
      const code = isErrorEnvelope(body)
        ? body.error.code
        : (STATUS_CODES[xhr.status] ?? (xhr.status >= 500 ? 'internal' : 'unknown'));
      const message = isErrorEnvelope(body)
        ? body.error.message
        : (FALLBACK_MESSAGES[code] ?? FALLBACK_MESSAGES.unknown ?? '');
      const error = new ApiError(code, message, xhr.status, body);
      if (xhr.status === 401) notifyUnauthorized(error);
      reject(error);
    };
    xhr.send(chunk);
  });
}

function receivedFrom(error: unknown): number | null {
  if (!(error instanceof ApiError) || error.status !== 409) return null;
  const received = (error.body as {received?: unknown} | undefined)?.received;
  return typeof received === 'number' ? received : null;
}

function isRetryable(error: unknown): boolean {
  if (!(error instanceof ApiError)) return true;
  if (error.code === 'aborted') return false;
  return error.code === 'network' || error.status >= 500 || error.status === 429 || error.status === 409;
}

/**
 * A single resumable upload. `start()` resolves with the `Download` the server
 * creates when the file is complete; `pause()` stops sending and keeps the
 * server's partial file; `cancel()` stops and deletes it.
 */
export class ResumableUpload {
  readonly file: File;
  private readonly options: UploadOptions;
  private controller = new AbortController();
  private uploadId: string | null;
  private chunkSize = UPLOAD_CHUNK_SIZE;
  private acknowledged = 0;
  private phase: UploadPhase = 'starting';
  private isCancelled = false;
  private rate = 0;
  private lastSample: {time: number; bytes: number} | null = null;

  constructor(file: File, options: UploadOptions = {}) {
    this.file = file;
    this.options = options;
    this.uploadId = options.resumeId ?? null;
  }

  get id(): string | null {
    return this.uploadId;
  }

  private emit(inFlight = 0) {
    const loaded = Math.min(this.file.size, this.acknowledged + inFlight);
    const now = performance.now();
    if (this.lastSample) {
      const dt = (now - this.lastSample.time) / 1000;
      if (dt >= 0.25) {
        const instant = Math.max(0, loaded - this.lastSample.bytes) / dt;
        this.rate = this.rate === 0 ? instant : this.rate * 0.7 + instant * 0.3;
        this.lastSample = {time: now, bytes: loaded};
      }
    } else {
      this.lastSample = {time: now, bytes: loaded};
    }
    this.options.onProgress?.({
      uploadId: this.uploadId,
      phase: this.phase,
      loaded,
      total: this.file.size,
      fraction: this.file.size === 0 ? 1 : loaded / this.file.size,
      bytesPerSecond: this.rate,
    });
  }

  private async ensureSession(): Promise<void> {
    if (this.uploadId) {
      try {
        const status = await uploads.status(this.uploadId, this.controller.signal);
        if (status.state === 'receiving' && status.size === this.file.size) {
          this.acknowledged = status.received;
          return;
        }
      } catch (error) {
        if (error instanceof ApiError && error.code === 'aborted') throw error;
        // Unknown or finished upload: start a new one below.
      }
    }
    const created: UploadCreated = await uploads.create({
      filename: this.file.name,
      size: this.file.size,
      mime: this.file.type || undefined,
      last_modified: this.file.lastModified ? Math.floor(this.file.lastModified / 1000) : undefined,
    });
    this.uploadId = created.id;
    this.chunkSize = Math.min(MAX_CHUNK_SIZE, created.chunk_size > 0 ? created.chunk_size : UPLOAD_CHUNK_SIZE);
    this.acknowledged = created.received;
    this.options.onCreated?.(created.id);
  }

  async start(): Promise<Download> {
    if (this.isCancelled) throw new ApiError('aborted', FALLBACK_MESSAGES.aborted ?? '', 0);
    if (this.controller.signal.aborted) this.controller = new AbortController();
    const signal = this.controller.signal;
    const retries = this.options.retries ?? DEFAULT_RETRIES;
    this.phase = 'starting';
    this.emit();

    try {
      await this.ensureSession();
      const uploadId = this.uploadId;
      if (!uploadId) throw new ApiError('unknown', FALLBACK_MESSAGES.unknown ?? '', 0);

      this.phase = 'uploading';
      this.emit();
      let failures = 0;
      while (this.acknowledged < this.file.size) {
        const offset = this.acknowledged;
        const chunk = this.file.slice(offset, Math.min(this.file.size, offset + this.chunkSize));
        try {
          this.acknowledged = await putChunk(uploadId, offset, chunk, signal, sent => this.emit(sent));
          failures = 0;
          this.emit();
        } catch (error) {
          const serverReceived = receivedFrom(error);
          if (serverReceived !== null) {
            // The server holds a different amount: continue from its count.
            this.acknowledged = serverReceived;
            this.emit();
            continue;
          }
          if (!isRetryable(error) || failures >= retries) throw error;
          failures += 1;
          await sleep(Math.min(30_000, 1000 * 2 ** failures), signal);
          try {
            const status = await uploads.status(uploadId, signal);
            this.acknowledged = status.received;
          } catch (statusError) {
            if (statusError instanceof ApiError && statusError.code === 'aborted') throw statusError;
            // Keep the last acknowledged offset and try the chunk again.
          }
        }
      }

      this.phase = 'finishing';
      this.emit();
      const download = await uploads.complete(uploadId);
      this.phase = 'done';
      this.emit();
      return download;
    } catch (error) {
      if (error instanceof ApiError && error.code === 'aborted') {
        this.phase = this.isCancelled ? 'cancelled' : 'paused';
      } else {
        this.phase = 'failed';
      }
      this.emit();
      throw error;
    }
  }

  /** Stops sending. The partial upload stays on the server; call `start()` to continue. */
  pause(): void {
    this.phase = 'paused';
    this.controller.abort();
  }

  /** Stops sending and deletes the partial upload. */
  async cancel(): Promise<void> {
    this.isCancelled = true;
    this.phase = 'cancelled';
    this.controller.abort();
    if (this.uploadId) {
      try {
        await uploads.cancel(this.uploadId);
      } catch {
        // Already gone or unreachable; nothing else to do.
      }
    }
  }
}

/** Uploads one file to completion. Abort with `signal` to pause (the server keeps the partial file). */
export function uploadFile(
  file: File,
  options: UploadOptions & {signal?: AbortSignal} = {},
): Promise<Download> {
  const upload = new ResumableUpload(file, options);
  options.signal?.addEventListener('abort', () => upload.pause(), {once: true});
  return upload.start();
}
