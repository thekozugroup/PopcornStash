/**
 * Can this page use passkeys right now? Combines what the browser reports
 * (secure context, WebAuthn) with what the server reports (`passkeys_ready`,
 * `public_url`, `rp_id`) and says why not, in plain words.
 */
import type {AuthState} from '../api/types';
import {passkeySupport} from '../api/webauthn';

export type PasskeyBlocker =
  | 'insecure-context'
  | 'browser-unsupported'
  | 'wrong-address'
  | 'server-not-ready';

export interface PasskeyAvailability {
  ok: boolean;
  blocker: PasskeyBlocker | null;
  title: string;
  description: string;
  /** Where passkeys will work, when the server knows its public address. */
  publicUrl: string | null;
}

function hostMatchesRpId(hostname: string, rpId: string): boolean {
  const host = hostname.toLowerCase();
  const rp = rpId.toLowerCase();
  return host === rp || host.endsWith(`.${rp}`);
}

function safeOrigin(url: string | null): string | null {
  if (!url) return null;
  try {
    return new URL(url).origin;
  } catch {
    return null;
  }
}

export function passkeyAvailability(state: AuthState | null): PasskeyAvailability {
  const publicUrl = safeOrigin(state?.public_url ?? null);
  const here = window.location.origin;
  const support = passkeySupport();

  if (!support.ok && support.reason === 'insecure-context') {
    return {
      ok: false,
      blocker: 'insecure-context',
      title: 'Passkeys need a secure address',
      description: publicUrl
        ? `This page is open at ${here}, which browsers do not trust for passkeys. Open PopcornStash at ${publicUrl} instead.`
        : `This page is open at ${here}, which browsers do not trust for passkeys. Passkeys work over HTTPS on a domain name, or on this computer at localhost. Put PopcornStash behind HTTPS (a reverse proxy, Tailscale or the built-in TLS) and set STASH_PUBLIC_URL to that address.`,
      publicUrl,
    };
  }

  if (!support.ok) {
    return {
      ok: false,
      blocker: 'browser-unsupported',
      title: 'This browser cannot use passkeys',
      description:
        'Open PopcornStash in a current version of Safari, Chrome, Edge or Firefox, or on a phone that supports passkeys.',
      publicUrl,
    };
  }

  if (state && state.rp_id && !hostMatchesRpId(window.location.hostname, state.rp_id)) {
    return {
      ok: false,
      blocker: 'wrong-address',
      title: 'Open PopcornStash at its own address',
      description: publicUrl
        ? `Passkeys for this server belong to ${state.rp_id}. Open ${publicUrl} to sign in.`
        : `Passkeys for this server belong to ${state.rp_id}. Open PopcornStash at that address to sign in.`,
      publicUrl,
    };
  }

  if (state && !state.passkeys_ready) {
    return {
      ok: false,
      blocker: 'server-not-ready',
      title: 'Passkeys are not set up on the server yet',
      description:
        state.reason ??
        'Set STASH_PUBLIC_URL to the HTTPS address you use to open PopcornStash, then restart the container.',
      publicUrl,
    };
  }

  return {ok: true, blocker: null, title: '', description: '', publicUrl};
}

/** Same path and query, on the server's public address. */
export function publicUrlForCurrentPage(publicUrl: string): string {
  const {pathname, search} = window.location;
  return `${publicUrl}${pathname}${search}`;
}
