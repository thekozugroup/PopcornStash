/**
 * Recognises what someone pasted or shared: a magnet link, a link to a
 * .torrent file, or an ordinary web link. Anything else is turned down with
 * one plain sentence. Used by the Downloads field (live, while typing) and the
 * share target (to pull the first link out of shared text).
 */

export type LinkKind = 'magnet' | 'torrent-url' | 'http';

export interface DetectedLink {
  kind: LinkKind;
  /** The exact link to send to `POST /api/downloads`. */
  url: string;
  /** Web links: the host name ("download.blender.org"). */
  host: string | null;
  /** A readable name: the magnet `dn` or the last part of the web address. */
  name: string | null;
}

export type LinkCheck =
  | {status: 'empty'}
  | {status: 'ok'; link: DetectedLink; /** The link was found inside longer text. */ extracted: boolean}
  | {status: 'invalid'; message: string};

const SCHEME = /^([a-z][a-z0-9+.-]*):/i;
const MAGNET_HASH = /(?:^|[?&])xt=urn:(?:btih|btmh):[^&]+/i;
const FIND_LINK = /\b(?:magnet:\?[^\s"'<>]+|https?:\/\/[^\s"'<>]+)/i;
const BARE_DOMAIN = /^(?:www\.)?[a-z0-9-]+(?:\.[a-z0-9-]+)+(?::\d+)?(?:[/?#]\S*)?$/i;
const TRAILING_PUNCTUATION = /[).,;:!?'"\]}>]+$/;

export const ONLY_LINKS_MESSAGE = 'Only web links (http or https) and magnet links can be downloaded.';

function decodeSafely(value: string): string {
  try {
    return decodeURIComponent(value.replace(/\+/g, ' '));
  } catch {
    return value;
  }
}

function magnetName(url: string): string | null {
  const query = url.slice(url.indexOf('?') + 1);
  for (const part of query.split('&')) {
    const [key, value] = part.split('=');
    if (key?.toLowerCase() === 'dn' && value) {
      const name = decodeSafely(value).trim();
      if (name) return name;
    }
  }
  return null;
}

function parseMagnet(url: string): DetectedLink | {error: string} {
  if (!MAGNET_HASH.test(url.slice(url.indexOf('?')))) {
    return {error: 'This magnet link is missing its torrent hash. Copy the whole link and paste it again.'};
  }
  return {kind: 'magnet', url, host: null, name: magnetName(url)};
}

function parseWeb(url: string): DetectedLink | {error: string} {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return {error: 'This web address is incomplete. Copy the whole link and paste it again.'};
  }
  if (!parsed.hostname) {
    return {error: 'This web address is incomplete. Copy the whole link and paste it again.'};
  }
  const segments = parsed.pathname.split('/').filter(Boolean);
  const last = segments.length > 0 ? decodeSafely(segments[segments.length - 1] ?? '') : null;
  const isTorrent = parsed.pathname.toLowerCase().endsWith('.torrent');
  return {
    kind: isTorrent ? 'torrent-url' : 'http',
    url: parsed.href,
    host: parsed.hostname.replace(/^www\./, ''),
    name: last && last.length > 0 ? last : null,
  };
}

function parseOne(candidate: string): DetectedLink | {error: string} | null {
  const scheme = SCHEME.exec(candidate)?.[1]?.toLowerCase();
  if (!scheme) return null;
  if (scheme === 'magnet') return parseMagnet(candidate);
  if (scheme === 'http' || scheme === 'https') return parseWeb(candidate);
  return {error: ONLY_LINKS_MESSAGE};
}

/** Finds the first magnet or http(s) link inside any text. */
export function findLink(text: string | null | undefined): DetectedLink | null {
  if (!text) return null;
  const match = FIND_LINK.exec(text);
  if (!match) return null;
  const candidate = match[0].replace(TRAILING_PUNCTUATION, '');
  const parsed = parseOne(candidate);
  return parsed && !('error' in parsed) ? parsed : null;
}

/** Checks one field value as the person types. */
export function checkLink(raw: string): LinkCheck {
  const value = raw.trim();
  if (!value) return {status: 'empty'};

  // A single token with a scheme: judge it as written.
  if (!/\s/.test(value) && SCHEME.test(value) && !/^[a-z0-9-]+\.[a-z]/i.test(value)) {
    const parsed = parseOne(value);
    if (parsed && 'error' in parsed) return {status: 'invalid', message: parsed.error};
    if (parsed) return {status: 'ok', link: parsed, extracted: false};
  }

  // Longer text (a pasted message): use the first link inside it.
  const found = findLink(value);
  if (found) return {status: 'ok', link: found, extracted: true};

  if (BARE_DOMAIN.test(value)) {
    return {status: 'invalid', message: 'Add https:// to the start of the web address.'};
  }
  if (SCHEME.test(value)) return {status: 'invalid', message: ONLY_LINKS_MESSAGE};
  return {status: 'invalid', message: 'This does not look like a link. Paste a web link or a magnet link.'};
}

/** Short description of a recognised link for the field's helper line. */
export function describeLink(link: DetectedLink): string {
  switch (link.kind) {
    case 'magnet':
      return 'Magnet link. Downloads as a torrent, only through the VPN.';
    case 'torrent-url':
      return `Link to a .torrent file on ${link.host ?? 'the web'}. Downloads as a torrent, only through the VPN.`;
    case 'http':
    default:
      return `Web link from ${link.host ?? 'the web'}. Downloads straight to PopcornStash.`;
  }
}

/** True when the link becomes a torrent (needs the VPN). */
export function isTorrentLink(link: DetectedLink): boolean {
  return link.kind === 'magnet' || link.kind === 'torrent-url';
}
