/**
 * PopcornStash API client for the extension (docs/architecture.md section 8).
 *
 * Every request goes to the paired origin only, carries the device token as
 * `Authorization: Bearer`, and omits browser cookies: the extension never
 * rides on the person's PopcornStash browser session.
 */

import {ProblemError, problemFromNetwork, problemFromResponse, type ProblemContext} from './errors';
import type {DownloadPayload} from './payload';
import {displayHost} from './server-url';
import type {Download, PairingPoll, PairingStarted, VpnStatus} from './types';

export const REQUEST_TIMEOUT_MS = 15_000;

export interface Connection {
  /** Origin, e.g. "https://stash.example.com". */
  server: string;
  token: string;
}

interface RequestOptions {
  method?: 'GET' | 'POST';
  body?: unknown;
  token?: string;
  context: ProblemContext;
  /** Status codes that are answers, not errors (pairing poll uses 202 and 410). */
  accept?: readonly number[];
}

async function readJson(response: Response): Promise<unknown> {
  const type = response.headers.get('content-type') ?? '';
  if (!type.includes('json')) return null;
  try {
    return await response.json();
  } catch {
    return null;
  }
}

async function request<T>(origin: string, path: string, options: RequestOptions): Promise<{status: number; body: T}> {
  const headers: Record<string, string> = {Accept: 'application/json'};
  if (options.body !== undefined) headers['Content-Type'] = 'application/json';
  if (options.token) headers.Authorization = `Bearer ${options.token}`;
  const host = displayHost(origin);
  const context: ProblemContext = {...options.context, host};

  let response: Response;
  try {
    response = await fetch(`${origin}${path}`, {
      method: options.method ?? 'GET',
      headers,
      body: options.body === undefined ? null : JSON.stringify(options.body),
      credentials: 'omit',
      cache: 'no-store',
      // Never follow a redirect with the token attached; ask for the final address instead.
      redirect: 'manual',
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    });
  } catch (error) {
    throw new ProblemError(problemFromNetwork(error, host));
  }

  if (response.type === 'opaqueredirect' || (response.status >= 300 && response.status < 400)) {
    throw new ProblemError({
      kind: 'not_popcornstash',
      title: 'The server sent this browser elsewhere',
      message: `${host} redirects to another address. Connect to the address PopcornStash finally opens at.`,
      repair: null,
    });
  }

  const body = await readJson(response);
  if (response.ok || options.accept?.includes(response.status)) {
    if (body === null && response.status !== 204) {
      throw new ProblemError(problemFromResponse(404, null, context), response.status);
    }
    return {status: response.status, body: body as T};
  }
  throw new ProblemError(problemFromResponse(response.status, body, context), response.status);
}

export async function requestPairing(origin: string, clientName: string): Promise<PairingStarted> {
  const {body} = await request<PairingStarted>(origin, '/api/pairing/request', {
    method: 'POST',
    body: {client_name: clientName, client_kind: 'chrome-extension'},
    context: {action: 'pair'},
  });
  if (typeof body?.pairing_id !== 'string' || typeof body.user_code !== 'string' || typeof body.poll_secret !== 'string') {
    throw new ProblemError(problemFromResponse(404, null, {action: 'pair', host: displayHost(origin)}));
  }
  return body;
}

export async function pollPairing(origin: string, pairingId: string, pollSecret: string): Promise<PairingPoll> {
  const {status, body} = await request<PairingPoll>(origin, '/api/pairing/poll', {
    method: 'POST',
    body: {pairing_id: pairingId, poll_secret: pollSecret},
    context: {action: 'poll'},
    accept: [202, 410],
  });
  if (status === 202) return {status: 'pending'};
  if (status === 410) return body?.status === 'denied' ? {status: 'denied'} : {status: 'expired'};
  if (body?.status === 'approved' && typeof body.token === 'string' && body.token !== '') {
    return {
      status: 'approved',
      token: body.token,
      scopes: Array.isArray(body.scopes) ? body.scopes : [],
      server_name: typeof body.server_name === 'string' ? body.server_name : '',
    };
  }
  return {status: 'pending'};
}

export async function addDownload(connection: Connection, payload: DownloadPayload, magnet: boolean): Promise<Download> {
  const {body} = await request<Download>(connection.server, '/api/downloads', {
    method: 'POST',
    body: payload,
    token: connection.token,
    context: {action: 'send', magnet},
  });
  return body;
}

export async function listDownloads(connection: Connection, limit: number): Promise<Download[]> {
  const {body} = await request<{items: Download[]}>(connection.server, `/api/downloads?state=all&limit=${limit}`, {
    token: connection.token,
    context: {action: 'list'},
  });
  return Array.isArray(body?.items) ? body.items : [];
}

/**
 * Sign this browser out: `POST /api/auth/logout` with the device token
 * revokes that token on the server (it disappears from Settings, Devices).
 */
export async function revokeToken(connection: Connection): Promise<void> {
  await request<null>(connection.server, '/api/auth/logout', {
    method: 'POST',
    token: connection.token,
    context: {action: 'revoke'},
  });
}

export async function vpnStatus(connection: Connection): Promise<VpnStatus> {
  const {body} = await request<VpnStatus>(connection.server, '/api/vpn/status', {
    token: connection.token,
    context: {action: 'status'},
  });
  return body;
}

/** Narrow metadata available to download-creation tokens; no library contents. */
export async function destinations(connection: Connection): Promise<Array<{id: string; name: string; icon: string; kinds: string[]}>> {
  const {body} = await request<Array<{id: string; name: string; icon: string; kinds: string[]}>>(
    connection.server, '/api/downloads/destinations', {token: connection.token, context: {action: 'send'}});
  if (!Array.isArray(body) || body.some(row => typeof row.id !== 'string' || typeof row.name !== 'string')) {
    throw new Error('The server did not return its libraries.');
  }
  return body;
}
