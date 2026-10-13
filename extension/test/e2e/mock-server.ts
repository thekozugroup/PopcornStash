/**
 * A small stand-in for the PopcornStash API (docs/architecture.md section 8):
 * device pairing, downloads and VPN status, with the real error shape. It
 * records every request so tests can assert what the extension sent.
 *
 * A second listener on `localhost` plays "some other website" with a page
 * that links to a slow file download (for download interception tests).
 *
 * Links whose path contains `/refused/` are rejected with 400; links whose
 * path contains `/server-fails/` are accepted, then listed as failed (the
 * server's own fetch failed).
 */

import {createServer, type IncomingMessage, type Server, type ServerResponse} from 'node:http';
import type {AddressInfo} from 'node:net';

export interface RecordedRequest {
  method: string;
  path: string;
  headers: IncomingMessage['headers'];
  body: unknown;
}

export interface MockDownload {
  id: string;
  kind: 'torrent' | 'http' | 'upload';
  name: string;
  source: string;
  state: string;
  progress: number;
  total_bytes: number | null;
  done_bytes: number;
  down_bps: number;
  up_bps: number;
  peers: number | null;
  eta_s: number | null;
  error: string | null;
  created_at: number;
  completed_at: number | null;
  item_ids: string[];
}

export interface MockServer {
  origin: string;
  /** "Some other website" on a different host name (localhost), for interception tests. */
  siteOrigin: string;
  requests: RecordedRequest[];
  /** Approve the pending pairing request (as if someone clicked Approve in PopcornStash). */
  approve(): void;
  /** Revoke the device token: every bearer call answers 401 from now on. */
  revoke(): void;
  /** The device token was revoked (Settings, Devices, or the device signed out). */
  isRevoked(): boolean;
  /** Make `GET /api/downloads` answer 500 (or recover). */
  failList(fail: boolean): void;
  pairingRequested(): boolean;
  sentDownloads(): RecordedRequest[];
  close(): Promise<void>;
}

export const TOKEN = 'pst_e2eTestToken0000000000000000000000000000000';
export const USER_CODE = 'KQ7M-2XWD';

const now = () => Math.floor(Date.now() / 1000);

function seedDownloads(): MockDownload[] {
  const base = {
    up_bps: 0,
    eta_s: null,
    error: null,
    completed_at: null,
    item_ids: [],
  };
  return [
    {
      ...base,
      id: 'seed-1',
      kind: 'torrent',
      name: 'Night of the Living Dead (1968)',
      source: 'magnet:?xt=urn:btih:5b1e',
      state: 'downloading',
      progress: 0.64,
      total_bytes: 1_610_612_736,
      done_bytes: 1_030_792_151,
      down_bps: 4_200_000,
      peers: 23,
      eta_s: 140,
      created_at: now() - 600,
    },
    {
      ...base,
      id: 'seed-2',
      kind: 'torrent',
      name: 'Sintel (2010) 1080p',
      source: 'magnet:?xt=urn:btih:08ad',
      state: 'queued',
      progress: 0,
      total_bytes: null,
      done_bytes: 0,
      down_bps: 0,
      peers: null,
      error: 'Waiting for the VPN to connect.',
      created_at: now() - 900,
    },
    {
      ...base,
      id: 'seed-3',
      kind: 'http',
      name: 'Tears of Steel.mov',
      source: 'https://media.example.org/tears-of-steel.mov',
      state: 'sorted',
      progress: 1,
      total_bytes: 734_003_200,
      done_bytes: 734_003_200,
      down_bps: 0,
      peers: null,
      completed_at: now() - 3_000,
      created_at: now() - 3_600,
      item_ids: ['item-1'],
    },
    {
      ...base,
      id: 'seed-4',
      kind: 'http',
      name: 'grandma-80th-birthday.mp4',
      source: 'https://share.example.net/f/8812',
      state: 'failed',
      progress: 0.12,
      total_bytes: 2_147_483_648,
      done_bytes: 257_698_037,
      down_bps: 0,
      peers: null,
      error: 'The link expired (403).',
      created_at: now() - 7_200,
    },
  ];
}

const VPN_STATUS = {
  configured: true,
  reachable: true,
  state: 'connected',
  provider: 'nordvpn',
  protocol: 'wireguard',
  label: null,
  server: 'nl1042.nordvpn.com',
  country: 'Netherlands',
  public_ip: '203.0.113.42',
  protected: true,
  since: now() - 7_200,
  error: null,
  kill_switch: true,
  allow_no_vpn: false,
};

async function readBody(request: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = [];
  for await (const chunk of request) chunks.push(chunk as Buffer);
  const text = Buffer.concat(chunks).toString('utf8');
  if (text === '') return null;
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

function json(response: ServerResponse, status: number, body: unknown): void {
  response.writeHead(status, {'content-type': 'application/json', 'cache-control': 'no-store'});
  response.end(JSON.stringify(body));
}

function apiError(response: ServerResponse, status: number, code: string, message: string): void {
  json(response, status, {error: {code, message}});
}

/**
 * A 512 KiB file sent over about three seconds, so Chrome's download is still
 * running when the extension pauses it, and finishes once it is resumed.
 */
function trickle(request: IncomingMessage, response: ServerResponse, name: string): void {
  const total = 512 * 1024;
  const chunk = 32 * 1024;
  response.writeHead(200, {
    'content-type': 'application/octet-stream',
    'content-disposition': `attachment; filename="${name}"`,
    'content-length': String(total),
  });
  let sent = 0;
  const timer = setInterval(() => {
    if (sent >= total) {
      clearInterval(timer);
      response.end();
      return;
    }
    response.write(Buffer.alloc(chunk));
    sent += chunk;
  }, 200);
  request.on('close', () => clearInterval(timer));
}

function listen(server: Server, host: string): Promise<string> {
  return new Promise(resolve => {
    server.listen(0, host, () => {
      const {port} = server.address() as AddressInfo;
      resolve(`http://${host}:${port}`);
    });
  });
}

export async function startMockServer(): Promise<MockServer> {
  const requests: RecordedRequest[] = [];
  const downloads = seedDownloads();
  let pairing: {id: string; secret: string; status: 'pending' | 'approved' | 'delivered'} | null = null;
  let revoked = false;
  let listFails = false;
  let nextId = 1;
  let origin = '';

  const api = createServer(async (request, response) => {
    const url = new URL(request.url ?? '/', 'http://mock');
    const body = await readBody(request);
    requests.push({method: request.method ?? 'GET', path: url.pathname + url.search, headers: request.headers, body});

    const authorized = request.headers.authorization === `Bearer ${TOKEN}` && !revoked;
    const route = `${request.method} ${url.pathname}`;

    switch (route) {
      case 'POST /api/pairing/request': {
        const {client_name, client_kind} = (body ?? {}) as {client_name?: string; client_kind?: string};
        if (!client_name || client_kind !== 'chrome-extension') {
          apiError(response, 400, 'bad_request', 'Device type must be chrome-extension, android or other.');
          return;
        }
        pairing = {id: `pair-${nextId++}`, secret: `secret-${nextId++}`, status: 'pending'};
        json(response, 200, {
          pairing_id: pairing.id,
          user_code: USER_CODE,
          poll_secret: pairing.secret,
          expires_at: now() + 600,
          verify_url: `${origin}/pair?code=${USER_CODE}`,
          interval: 1,
        });
        return;
      }
      case 'POST /api/pairing/poll': {
        const {pairing_id, poll_secret} = (body ?? {}) as {pairing_id?: string; poll_secret?: string};
        if (!pairing || pairing.id !== pairing_id || pairing.secret !== poll_secret) {
          apiError(response, 404, 'not_found', 'That pairing request does not exist. Start pairing again.');
          return;
        }
        if (pairing.status === 'pending') {
          json(response, 202, {status: 'pending'});
        } else if (pairing.status === 'approved') {
          pairing.status = 'delivered';
          json(response, 200, {
            status: 'approved',
            token: TOKEN,
            scopes: ['downloads:create', 'downloads:read'],
            server_name: '127.0.0.1',
          });
        } else {
          json(response, 410, {status: 'expired'});
        }
        return;
      }
      case 'POST /api/auth/logout': {
        // A bearer request ends that device token (crates/stash-server/src/auth/sessions.rs).
        if (!authorized) {
          apiError(response, 401, 'unauthorized', 'Sign in again.');
          return;
        }
        revoked = true;
        response.writeHead(204);
        response.end();
        return;
      }
      case 'GET /api/downloads': {
        if (!authorized) {
          apiError(response, 401, 'unauthorized', 'Sign in again.');
          return;
        }
        if (listFails) {
          apiError(response, 500, 'internal', 'The database is busy.');
          return;
        }
        const limit = Number(url.searchParams.get('limit') ?? '100');
        json(response, 200, {items: downloads.slice(0, limit)});
        return;
      }
      case 'POST /api/downloads': {
        if (!authorized) {
          apiError(response, 401, 'unauthorized', 'Sign in again.');
          return;
        }
        const payload = (body ?? {}) as {url?: string};
        if (!payload.url) {
          apiError(response, 400, 'bad_request', 'Paste a link to download.');
          return;
        }
        if (payload.url.includes('/refused/')) {
          apiError(response, 400, 'bad_request', 'That site does not allow downloads from servers.');
          return;
        }
        const magnet = payload.url.startsWith('magnet:');
        const name = magnet
          ? (new URLSearchParams(payload.url.slice(8)).get('dn') ?? 'Torrent')
          : decodeURIComponent(new URL(payload.url).pathname.split('/').pop() || 'download');
        const created: MockDownload = {
          id: `dl-${nextId++}`,
          kind: magnet ? 'torrent' : 'http',
          name,
          source: payload.url.split('?')[0] ?? payload.url,
          state: 'downloading',
          progress: 0.08,
          total_bytes: 52_428_800,
          done_bytes: 4_194_304,
          down_bps: 2_400_000,
          up_bps: 0,
          peers: magnet ? 4 : null,
          eta_s: 20,
          error: null,
          created_at: now(),
          completed_at: null,
          item_ids: [],
        };
        downloads.unshift(
          payload.url.includes('/server-fails/')
            ? {...created, state: 'failed', progress: 0, done_bytes: 0, down_bps: 0, error: 'The site answered 403.'}
            : created,
        );
        json(response, 200, created);
        return;
      }
      case 'GET /api/vpn/status': {
        if (!authorized) {
          apiError(response, 401, 'unauthorized', 'Sign in again.');
          return;
        }
        json(response, 200, VPN_STATUS);
        return;
      }
      case 'GET /files/own.bin':
        // A file saved from PopcornStash itself (Download original).
        trickle(request, response, 'own.bin');
        return;
      case 'GET /pair':
      case 'GET /':
      case 'GET /downloads':
        response.writeHead(200, {'content-type': 'text/html'});
        response.end('<!doctype html><title>PopcornStash</title><p>PopcornStash (mock)</p>');
        return;
      default:
        if (url.pathname.startsWith('/api/')) {
          apiError(response, 404, 'not_found', 'Not found.');
        } else {
          response.writeHead(404, {'content-type': 'text/plain'});
          response.end('not found');
        }
    }
  });

  // "Some other website": a page with a download link, and a slow file.
  const site = createServer((request, response) => {
    const url = new URL(request.url ?? '/', 'http://site');
    if (url.pathname === '/files/page.html') {
      response.writeHead(200, {'content-type': 'text/html'});
      response.end(
        '<!doctype html><title>Films</title><a id="file" href="/files/Big%20Buck%20Bunny.mkv">Download Big Buck Bunny</a>',
      );
      return;
    }
    if (url.pathname === '/files/edge.html') {
      response.writeHead(200, {'content-type': 'text/html'});
      response.end(
        [
          '<!doctype html><title>Edge cases</title>',
          '<a id="refused" href="/files/refused/refused.bin">Refused by PopcornStash</a>',
          '<a id="server-fails" href="/files/server-fails/server-fails.bin">PopcornStash cannot fetch it</a>',
          '<a id="data" href="data:text/plain;base64,SGVsbG8=" download="inline.txt">Built into the page</a>',
          `<a id="own" href="${origin}/files/own.bin">From PopcornStash</a>`,
        ].join(''),
      );
      return;
    }
    const finite = /^\/files\/(?:refused|server-fails)\/([a-z-]+\.bin)$/.exec(url.pathname);
    if (finite?.[1]) {
      trickle(request, response, finite[1]);
      return;
    }
    if (url.pathname === '/files/Big%20Buck%20Bunny.mkv') {
      response.writeHead(200, {
        'content-type': 'video/x-matroska',
        'content-disposition': 'attachment; filename="Big Buck Bunny.mkv"',
        'content-length': String(64 * 1024 * 1024),
      });
      // Send a little, then trickle so the download stays in progress.
      response.write(Buffer.alloc(64 * 1024));
      const timer = setInterval(() => response.write(Buffer.alloc(1024)), 500);
      const stop = () => clearInterval(timer);
      request.on('close', stop);
      response.on('close', stop);
      setTimeout(() => {
        stop();
        response.destroy();
      }, 60_000).unref();
      return;
    }
    response.writeHead(404);
    response.end();
  });

  origin = await listen(api, '127.0.0.1');
  const siteOrigin = (await listen(site, '127.0.0.1')).replace('127.0.0.1', 'localhost');

  return {
    origin,
    siteOrigin,
    requests,
    approve() {
      if (pairing?.status === 'pending') pairing.status = 'approved';
    },
    revoke() {
      revoked = true;
    },
    isRevoked() {
      return revoked;
    },
    failList(fail: boolean) {
      listFails = fail;
    },
    pairingRequested() {
      return pairing !== null;
    },
    sentDownloads() {
      return requests.filter(r => r.method === 'POST' && r.path === '/api/downloads');
    },
    async close() {
      api.closeAllConnections();
      site.closeAllConnections();
      await Promise.all([
        new Promise<void>(resolve => api.close(() => resolve())),
        new Promise<void>(resolve => site.close(() => resolve())),
      ]);
    },
  };
}
