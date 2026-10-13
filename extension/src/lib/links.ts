/**
 * What the extension can send: web links (http/https) and magnet links.
 * blob:, data: and file: URLs only exist inside this browser, so the server
 * could never fetch them.
 */

export type LinkCheck =
  | {ok: true; kind: 'magnet' | 'web'; url: string}
  | {ok: false; reason: string};

const MAGNET_PREFIX = /^magnet:\?/i;
/** A BitTorrent exact topic: v1 (btih) or v2 (btmh) info hash. */
const TORRENT_TOPIC = /(?:^|&)xt=urn:bt(?:ih|mh):[^&]+/i;

/** True for `magnet:?…` links that name a torrent (`xt=urn:btih:` or `xt=urn:btmh:`). */
export function isMagnet(value: string): boolean {
  const text = value.trim();
  if (!MAGNET_PREFIX.test(text)) return false;
  const query = text.slice(text.indexOf('?') + 1);
  return TORRENT_TOPIC.test(query);
}

export function isWebUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return (url.protocol === 'http:' || url.protocol === 'https:') && url.hostname !== '';
  } catch {
    return false;
  }
}

const LOCAL_ONLY: Record<string, string> = {
  'blob:': 'This file only exists inside this browser tab, so PopcornStash cannot fetch it. Send the page link instead.',
  'data:': 'This file is built into the page itself, so PopcornStash cannot fetch it. Save it from Chrome instead.',
  'file:': 'This file is already on this computer. Upload it in PopcornStash instead.',
  'filesystem:': 'This file only exists inside this browser, so PopcornStash cannot fetch it.',
};

/** Check a pasted or clicked link before sending it. */
export function checkLink(input: string): LinkCheck {
  const text = input.trim();
  if (text === '') return {ok: false, reason: 'Paste a web link or a magnet link.'};

  if (MAGNET_PREFIX.test(text)) {
    return isMagnet(text)
      ? {ok: true, kind: 'magnet', url: text}
      : {ok: false, reason: 'That magnet link has no torrent in it. Copy the whole link again.'};
  }

  // People often paste "www.example.com/file.mkv" without a scheme.
  const candidate = /^www\./i.test(text) ? `https://${text}` : text;
  let url: URL;
  try {
    url = new URL(candidate);
  } catch {
    return {ok: false, reason: 'That is not a link. Paste a web address (https://…) or a magnet link.'};
  }
  const localReason = LOCAL_ONLY[url.protocol];
  if (localReason) return {ok: false, reason: localReason};
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    return {ok: false, reason: 'PopcornStash can download web links (http or https) and magnet links.'};
  }
  if (url.hostname === '') return {ok: false, reason: 'That link is missing a site name.'};
  if (url.username !== '' || url.password !== '') {
    return {ok: false, reason: 'Links with a user name and password in them are not sent. Remove them and try again.'};
  }
  return {ok: true, kind: 'web', url: url.href};
}

/** A readable name for a link, used until the server answers with the real one. */
export function linkName(value: string): string {
  const text = value.trim();
  if (MAGNET_PREFIX.test(text)) {
    const params = new URLSearchParams(text.slice(text.indexOf('?') + 1));
    const name = params.get('dn');
    if (name && name.trim() !== '') return name.trim();
    const topic = params.get('xt') ?? '';
    const hash = topic.split(':').pop() ?? '';
    return hash ? `Torrent ${hash.slice(0, 8).toLowerCase()}` : 'Torrent';
  }
  try {
    const url = new URL(text);
    const last = url.pathname.split('/').filter(Boolean).pop();
    if (last) {
      try {
        return decodeURIComponent(last);
      } catch {
        return last;
      }
    }
    return url.hostname;
  } catch {
    return text;
  }
}
