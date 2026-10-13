/**
 * The body of `POST /api/downloads`:
 * `{url, filename?, headers?: {cookie?, referer?, user_agent?}}`.
 *
 * Headers only help web links. Cookies are added only when the person turned
 * on "Include this site's sign-in cookies" and Chrome granted the permission,
 * and never for the PopcornStash server itself: its own session cookie must
 * not travel inside a download request. Cookies are matched by host name, not
 * origin, because browsers share cookies across ports.
 */

import {isMagnet, isWebUrl} from './links';

export interface ForwardedHeaders {
  cookie?: string;
  referer?: string;
  user_agent?: string;
}

export interface DownloadPayload {
  url: string;
  filename?: string;
  headers?: ForwardedHeaders;
}

export interface CookieLike {
  name: string;
  value: string;
}

export interface PayloadInput {
  url: string;
  /** Suggested file name, for example from Chrome's download item. A path is reduced to its last part. */
  filename?: string | null | undefined;
  /** The page the link or media was on. */
  pageUrl?: string | null | undefined;
  userAgent?: string | null | undefined;
  /** Cookies Chrome holds for `url`. Ignored unless `allowCookies`. */
  cookies?: readonly CookieLike[] | null | undefined;
  /** The cookie option is on and the cookies permission is granted. */
  allowCookies: boolean;
  /** The PopcornStash server origin, e.g. "https://stash.example.com". */
  serverOrigin: string;
}

function hostnameOf(value: string): string | null {
  try {
    return new URL(value).hostname.toLowerCase();
  } catch {
    return null;
  }
}

/** True when `url` is on the PopcornStash server's host (any port). */
export function isOnServerHost(url: string, serverOrigin: string): boolean {
  const host = hostnameOf(url);
  const server = hostnameOf(serverOrigin);
  return host !== null && server !== null && host === server;
}

/**
 * The Referer Chrome itself would send for a request from `pageUrl` to
 * `targetUrl` under the default `strict-origin-when-cross-origin` policy:
 * the full page address (without fragment) for the same origin, only the
 * origin across origins, nothing when going from https to http.
 */
export function refererFor(pageUrl: string | null | undefined, targetUrl: string): string | undefined {
  if (!pageUrl) return undefined;
  let page: URL;
  let target: URL;
  try {
    page = new URL(pageUrl);
    target = new URL(targetUrl);
  } catch {
    return undefined;
  }
  if (page.protocol !== 'http:' && page.protocol !== 'https:') return undefined;
  if (page.username !== '' || page.password !== '') return undefined;
  if (page.protocol === 'https:' && target.protocol === 'http:') return undefined;
  if (page.origin === target.origin) {
    page.hash = '';
    return page.href;
  }
  return `${page.origin}/`;
}

/** `name=value; name2=value2`, skipping cookies with empty names. */
export function cookieHeader(cookies: readonly CookieLike[]): string | undefined {
  const parts = cookies.filter(c => c.name !== '').map(c => `${c.name}=${c.value}`);
  return parts.length > 0 ? parts.join('; ') : undefined;
}

/** Last path component, without directory separators or control characters. */
export function cleanFilename(value: string | null | undefined): string | undefined {
  if (!value) return undefined;
  const last = value.split(/[\\/]/).pop() ?? '';
  // Strip ASCII control characters (U+0000 to U+001F and U+007F).
  const cleaned = Array.from(last)
    .filter(ch => {
      const code = ch.charCodeAt(0);
      return code > 0x1f && code !== 0x7f;
    })
    .join('')
    .trim();
  return cleaned === '' || cleaned === '.' || cleaned === '..' ? undefined : cleaned;
}

/** May cookies be attached to a request for `url`? */
export function cookiesAllowedFor(url: string, allowCookies: boolean, serverOrigin: string): boolean {
  return allowCookies && isWebUrl(url) && !isOnServerHost(url, serverOrigin);
}

export function buildDownloadPayload(input: PayloadInput): DownloadPayload {
  const url = input.url.trim();
  const payload: DownloadPayload = {url};

  const filename = cleanFilename(input.filename);
  if (filename) payload.filename = filename;

  // Magnet links go to the torrent engine; HTTP headers mean nothing there.
  if (isMagnet(url) || !isWebUrl(url)) return payload;

  const headers: ForwardedHeaders = {};
  const referer = refererFor(input.pageUrl, url);
  if (referer) headers.referer = referer;
  const userAgent = input.userAgent?.trim();
  if (userAgent) headers.user_agent = userAgent;
  if (cookiesAllowedFor(url, input.allowCookies, input.serverOrigin) && input.cookies) {
    const cookie = cookieHeader(input.cookies);
    if (cookie) headers.cookie = cookie;
  }
  if (Object.keys(headers).length > 0) payload.headers = headers;
  return payload;
}
