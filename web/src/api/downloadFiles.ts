import {request} from './client';
import type {Download, TorrentFile, TorrentFiles} from './types';

export interface TransferFile extends TorrentFile {
  /** Hash-verified bytes can be unknown during a recheck or absent on older servers. */
  verified_bytes?: number | null;
  complete?: boolean;
}

export interface TransferFiles extends TorrentFiles {
  files: TransferFile[];
}

export function getTransferFiles(id: string, signal?: AbortSignal) {
  return request<TransferFiles>(`/api/downloads/${encodeURIComponent(id)}/files`, {signal});
}

export function skipTransferFile(id: string, index: number) {
  return request<void>(`/api/downloads/${encodeURIComponent(id)}/files/${index}/skip`, {
    method: 'POST', json: {},
  });
}

export function finishCompletedFiles(id: string, reviewed?: {
  expectedSelected: readonly number[];
  expectedComplete: ReadonlySet<number>;
}) {
  return request<Download>(`/api/downloads/${encodeURIComponent(id)}/finish-completed`, {
    method: 'POST',
    json: reviewed ? {
      expected_selected: reviewed.expectedSelected,
      expected_complete: [...reviewed.expectedComplete].sort((a, b) => a - b),
    } : {},
  });
}
