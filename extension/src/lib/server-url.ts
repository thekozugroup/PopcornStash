/**
 * The PopcornStash server address the person types on the options page.
 *
 * Only https:// addresses are accepted, plus plain http:// for localhost and
 * 127.0.0.1 (the same rule as passkeys and secure cookies on the server).
 * Paths, queries and fragments are dropped: the API always lives at /api on
 * the origin. Addresses with a user name or password are refused, because the
 * extension signs in with a pairing code and must never carry credentials in
 * a URL.
 */

export type ServerAddress =
  | {ok: true; origin: string; host: string; hostname: string; pattern: string}
  | {ok: false; reason: string};

/** Hosts that may use plain http. They match the manifest's optional host permissions. */
export const LOCAL_HOSTS: ReadonlySet<string> = new Set(['localhost', '127.0.0.1']);

const WITH_AUTHORITY = /^[a-z][a-z0-9+.-]*:\/\//i;
/** "mailto:", "javascript:"; not "stash.example.com:8443", where a port follows the colon. */
const OTHER_SCHEME = /^[a-z][a-z0-9+.-]*:(?!\d)/i;

export function parseServerAddress(input: string): ServerAddress {
  const typed = input.trim();
  if (typed === '') {
    return {ok: false, reason: 'Enter the address you open PopcornStash at.'};
  }
  if (/\s/.test(typed)) {
    return {ok: false, reason: 'A web address has no spaces. Check what you typed.'};
  }

  // "owner:secret@host" would otherwise read as a scheme named "owner".
  if (/^(?:[a-z][a-z0-9+.-]*:\/\/)?[^/?#]*@/i.test(typed)) {
    return {
      ok: false,
      reason: 'Remove the user name and password from the address. This browser connects with a pairing code instead.',
    };
  }

  // "stash.example.com" means https; "localhost:8788" means http.
  let candidate = typed;
  const hasScheme = WITH_AUTHORITY.test(typed) || OTHER_SCHEME.test(typed);
  if (!hasScheme) {
    const bareHost = typed.split(/[/:?#]/, 1)[0]?.toLowerCase() ?? '';
    candidate = `${LOCAL_HOSTS.has(bareHost) ? 'http' : 'https'}://${typed.replace(/^\/\//, '')}`;
  }

  let url: URL;
  try {
    url = new URL(candidate);
  } catch {
    return {ok: false, reason: 'That does not look like a web address. Try something like https://stash.example.com.'};
  }

  if (url.username !== '' || url.password !== '') {
    return {
      ok: false,
      reason: 'Remove the user name and password from the address. This browser connects with a pairing code instead.',
    };
  }

  const hostname = url.hostname.toLowerCase();
  if (url.protocol === 'http:') {
    if (!LOCAL_HOSTS.has(hostname)) {
      return {
        ok: false,
        reason: 'Use the https:// address. Plain http only works for localhost, because PopcornStash needs HTTPS to keep sign-in safe.',
      };
    }
  } else if (url.protocol !== 'https:') {
    return {ok: false, reason: 'Use an https:// address, like https://stash.example.com.'};
  }

  if (hostname === '') {
    return {ok: false, reason: 'The address is missing a server name.'};
  }

  return {
    ok: true,
    origin: url.origin,
    host: url.host,
    hostname,
    pattern: `${url.origin}/*`,
  };
}

/** The host to show people ("stash.example.com", "localhost:8788"). */
export function displayHost(origin: string): string {
  try {
    return new URL(origin).host;
  } catch {
    return origin;
  }
}

/**
 * Resolve the server's `verify_url` against the origin the extension talks
 * to. Servers without a public URL answer with a path ("/pair?code=…").
 * Anything that is not http(s) falls back to the pairing page on `origin`.
 */
export function resolveVerifyUrl(verifyUrl: string | null | undefined, origin: string, userCode: string): string {
  const fallback = `${origin}/pair?code=${encodeURIComponent(userCode)}`;
  if (!verifyUrl) return fallback;
  try {
    const url = new URL(verifyUrl, origin);
    if (url.protocol !== 'https:' && url.protocol !== 'http:') return fallback;
    if (url.username !== '' || url.password !== '') return fallback;
    return url.href;
  } catch {
    return fallback;
  }
}
