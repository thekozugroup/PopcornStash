/**
 * Turn API failures into plain sentences for notifications, the popup and
 * the options page. The server answers errors as
 * `{"error": {"code": "not_found", "message": "Human readable sentence."}}`.
 */

export type ProblemKind =
  | 'not_paired'
  | 'token_revoked'
  | 'no_access'
  | 'unreachable'
  | 'not_popcornstash'
  | 'vpn_unavailable'
  | 'rate_limited'
  | 'forbidden'
  | 'rejected'
  | 'setup_required'
  | 'server_error';

/** What the person can do about it: pair again, allow host access again, or nothing special. */
export type Repair = 'pair' | 'grant' | null;

export interface Problem {
  kind: ProblemKind;
  title: string;
  message: string;
  repair: Repair;
}

export interface ProblemContext {
  /** What the extension was doing. */
  action: 'send' | 'list' | 'status' | 'pair' | 'poll';
  /** The link being sent is a magnet link. */
  magnet?: boolean;
  /** Server host for messages ("stash.example.com"). */
  host?: string;
}

/** `{code, message}` from an API error body, or null when the body is not that shape. */
export function parseErrorBody(body: unknown): {code: string; message: string} | null {
  if (typeof body !== 'object' || body === null || !('error' in body)) return null;
  const error = (body as {error: unknown}).error;
  if (typeof error !== 'object' || error === null) return null;
  const {code, message} = error as {code?: unknown; message?: unknown};
  if (typeof code !== 'string' || code === '') return null;
  return {code, message: typeof message === 'string' ? message.trim() : ''};
}

const CODE_FOR_STATUS: Record<number, string> = {
  400: 'bad_request',
  401: 'unauthorized',
  403: 'forbidden',
  404: 'not_found',
  409: 'conflict',
  429: 'rate_limited',
};

export const NOT_PAIRED: Problem = {
  kind: 'not_paired',
  title: 'Connect PopcornStash first',
  message: 'Open the extension options and pair this browser with your PopcornStash server.',
  repair: 'pair',
};

export function tokenRevoked(): Problem {
  return {
    kind: 'token_revoked',
    title: 'Pair this browser again',
    message:
      'PopcornStash no longer accepts this browser. Its access was removed in Settings, Devices, or it expired. Open the extension options to pair again.',
    repair: 'pair',
  };
}

export function noHostAccess(host?: string): Problem {
  return {
    kind: 'no_access',
    title: 'Allow access to your server again',
    message: `Chrome no longer lets this extension reach ${host ?? 'your PopcornStash server'}. Open the extension options and connect again.`,
    repair: 'grant',
  };
}

export function unreachable(host?: string): Problem {
  return {
    kind: 'unreachable',
    title: 'Cannot reach PopcornStash',
    message: `Check that the server is running and that this computer can reach ${host ?? 'it'}.`,
    repair: null,
  };
}

/**
 * Map an HTTP error response. `body` is the parsed JSON (or null when the body
 * was not JSON). Server messages are already written for people, so they are
 * kept where they explain the specific problem.
 */
export function problemFromResponse(status: number, body: unknown, context: ProblemContext): Problem {
  const parsed = parseErrorBody(body);
  const code = parsed?.code ?? CODE_FOR_STATUS[status] ?? (status >= 500 ? 'internal' : 'bad_request');
  const serverMessage = parsed?.message ?? '';

  // A 404 without the API's error shape means the address is some other website.
  if (!parsed && (status === 404 || status === 405) && context.action !== 'poll') {
    return {
      kind: 'not_popcornstash',
      title: 'That address is not PopcornStash',
      message: `${context.host ?? 'The server'} answered, but not like a PopcornStash server. Check the address in the extension options.`,
      repair: null,
    };
  }

  switch (code) {
    case 'unauthorized':
      if (context.action === 'pair' || context.action === 'poll') {
        return {kind: 'rejected', title: 'Pairing did not start', message: serverMessage || 'Start pairing again.', repair: null};
      }
      return tokenRevoked();
    case 'forbidden':
      return {
        kind: 'forbidden',
        title: 'Not allowed for this browser',
        message: serverMessage || 'This browser can add downloads and see their progress, but not do that.',
        repair: null,
      };
    case 'vpn_unavailable':
      return {
        kind: 'vpn_unavailable',
        title: context.magnet ? 'Torrents are off: the VPN is not ready' : 'The VPN is not ready',
        message: context.magnet
          ? 'PopcornStash only downloads torrents through its VPN, and the VPN is not set up or not running. Web links still work.'
          : serverMessage || 'PopcornStash needs its VPN for this, and the VPN is not running.',
        repair: null,
      };
    case 'rate_limited':
      return {
        kind: 'rate_limited',
        title: 'Too many tries',
        message: 'PopcornStash asked this browser to slow down. Wait a minute, then try again.',
        repair: null,
      };
    case 'setup_required':
      return {
        kind: 'setup_required',
        title: 'Finish setting up PopcornStash',
        message: 'Open PopcornStash and create the owner passkey first, then pair this browser.',
        repair: null,
      };
    case 'bad_request':
    case 'not_found':
    case 'conflict':
      return {
        kind: 'rejected',
        title: rejectedTitle(context),
        message: serverMessage || 'The server did not accept the request.',
        repair: null,
      };
    case 'passkeys_unavailable':
      return {
        kind: 'rejected',
        title: rejectedTitle(context),
        message: serverMessage || 'Passkeys are not available on this server address.',
        repair: null,
      };
    default:
      return {
        kind: 'server_error',
        title: 'Something went wrong on the server',
        message: serverMessage || 'Try again in a moment. If it keeps happening, check the PopcornStash logs.',
        repair: null,
      };
  }
}

function rejectedTitle(context: ProblemContext): string {
  switch (context.action) {
    case 'send':
      return 'PopcornStash did not take that link';
    case 'pair':
    case 'poll':
      return 'Pairing did not work';
    default:
      return 'PopcornStash refused the request';
  }
}

/** A failed `fetch` (DNS, refused connection, TLS, CORS without permission) or a timeout. */
export function problemFromNetwork(error: unknown, host?: string): Problem {
  const timedOut = error instanceof DOMException && (error.name === 'TimeoutError' || error.name === 'AbortError');
  if (timedOut) {
    return {
      kind: 'unreachable',
      title: 'PopcornStash did not answer',
      message: `${host ?? 'The server'} took too long to answer. Check that it is running, then try again.`,
      repair: null,
    };
  }
  return unreachable(host);
}

/** An error carrying a Problem, thrown by the API client. */
export class ProblemError extends Error {
  readonly problem: Problem;
  readonly status: number | null;

  constructor(problem: Problem, status: number | null = null) {
    super(problem.message);
    this.name = 'ProblemError';
    this.problem = problem;
    this.status = status;
  }
}

export function toProblem(error: unknown, host?: string): Problem {
  if (error instanceof ProblemError) return error.problem;
  return problemFromNetwork(error, host);
}
