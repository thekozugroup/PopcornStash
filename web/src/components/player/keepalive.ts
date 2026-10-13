/**
 * Requests that must survive the page going away (tab closed, app switched
 * on a phone). The shared client in src/api/client.ts has no `keepalive`
 * option, so these two calls are sent with fetch directly, mirroring the
 * client's same-origin credentials and JSON content type (the server's CSRF
 * check needs both for cookie sessions).
 */
import type {ProgressUpdate} from '../../api/types';

/** `PUT /api/items/:id/progress` that outlives the page. */
export function saveProgressOnExit(itemId: string, body: ProgressUpdate): void {
  const url = `/api/items/${encodeURIComponent(itemId)}/progress`;
  try {
    void fetch(url, {
      method: 'PUT',
      credentials: 'same-origin',
      keepalive: true,
      headers: {'Content-Type': 'application/json', Accept: 'application/json'},
      body: JSON.stringify(body),
    }).catch(() => undefined);
  } catch {
    // Nothing to do: the page is closing.
  }
}

/** `DELETE /api/stream/:session` that outlives the page. */
export function endStreamOnExit(sessionId: string): void {
  const url = `/api/stream/${encodeURIComponent(sessionId)}`;
  try {
    void fetch(url, {method: 'DELETE', credentials: 'same-origin', keepalive: true}).catch(() => undefined);
  } catch {
    // Nothing to do: the page is closing.
  }
}
