/**
 * Requests that must survive the page going away (tab closed, app switched
 * on a phone). They use the shared client's `keepalive` option; a failure is
 * ignored because the page may already be gone.
 */
import {request} from '../../api/client';
import type {ProgressUpdate} from '../../api/types';

/** `PUT /api/items/:id/progress` that outlives the page. */
export function saveProgressOnExit(itemId: string, body: ProgressUpdate): void {
  void request(`/api/items/${encodeURIComponent(itemId)}/progress`, {
    method: 'PUT',
    json: body,
    keepalive: true,
    skipAuthRefresh: true,
  }).catch(() => undefined);
}

/** `DELETE /api/stream/:session` that outlives the page. */
export function endStreamOnExit(sessionId: string): void {
  void request(`/api/stream/${encodeURIComponent(sessionId)}`, {
    method: 'DELETE',
    keepalive: true,
    skipAuthRefresh: true,
  }).catch(() => undefined);
}
