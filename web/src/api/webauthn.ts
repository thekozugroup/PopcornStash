/**
 * WebAuthn helpers between webauthn-rs JSON (base64url binary fields) and the
 * browser's `navigator.credentials` API.
 *
 * Uses `PublicKeyCredential.parseCreationOptionsFromJSON` /
 * `parseRequestOptionsFromJSON` and `credential.toJSON()` where the browser
 * has them, with a manual base64url fallback for older engines. webauthn-rs
 * accepts the standard JSON (`clientExtensionResults` is an accepted alias).
 */
import type {
  AuthenticationCredentialJSON,
  CreationChallengeResponse,
  RegistrationCredentialJSON,
  RequestChallengeResponse,
} from './types';

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

export type PasskeySupport =
  | {ok: true}
  | {ok: false; reason: 'insecure-context' | 'unsupported'};

/**
 * Passkeys need a secure context (HTTPS on a domain name, or localhost) and a
 * browser with WebAuthn. A bare LAN IP over http is not a secure context.
 */
export function passkeySupport(): PasskeySupport {
  if (typeof window === 'undefined') return {ok: false, reason: 'unsupported'};
  if (!window.isSecureContext) return {ok: false, reason: 'insecure-context'};
  if (!('PublicKeyCredential' in window) || !window.PublicKeyCredential) {
    return {ok: false, reason: 'unsupported'};
  }
  return {ok: true};
}

/** True when the browser can offer passkeys from the username field (conditional UI). */
export async function supportsConditionalMediation(): Promise<boolean> {
  const PKC = typeof window !== 'undefined' ? window.PublicKeyCredential : undefined;
  if (!PKC || typeof PKC.isConditionalMediationAvailable !== 'function') return false;
  try {
    return await PKC.isConditionalMediationAvailable();
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// base64url
// ---------------------------------------------------------------------------

export function base64urlToBuffer(value: string): ArrayBuffer {
  const base64 = value.replace(/-/g, '+').replace(/_/g, '/');
  const padded = base64 + '='.repeat((4 - (base64.length % 4)) % 4);
  const binary = atob(padded);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes.buffer;
}

export function bufferToBase64url(buffer: ArrayBuffer | ArrayBufferView): string {
  const bytes =
    buffer instanceof ArrayBuffer
      ? new Uint8Array(buffer)
      : new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
  let binary = '';
  for (let i = 0; i < bytes.length; i += 1) binary += String.fromCharCode(bytes[i] ?? 0);
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

/** WebIDL dictionaries reject `null` for optional enum members; drop them. */
function stripNulls<T>(value: T): T {
  if (Array.isArray(value)) return value.map(stripNulls) as T;
  if (value && typeof value === 'object' && !(value instanceof ArrayBuffer)) {
    const out: Record<string, unknown> = {};
    for (const [key, inner] of Object.entries(value as Record<string, unknown>)) {
      if (inner === null || inner === undefined) continue;
      out[key] = stripNulls(inner);
    }
    return out as T;
  }
  return value;
}

type PublicKeyCredentialStatic = typeof PublicKeyCredential & {
  parseCreationOptionsFromJSON?: (
    options: PublicKeyCredentialCreationOptionsJSON,
  ) => PublicKeyCredentialCreationOptions;
  parseRequestOptionsFromJSON?: (
    options: PublicKeyCredentialRequestOptionsJSON,
  ) => PublicKeyCredentialRequestOptions;
};

function pkcStatic(): PublicKeyCredentialStatic | undefined {
  return typeof window !== 'undefined'
    ? (window.PublicKeyCredential as PublicKeyCredentialStatic | undefined)
    : undefined;
}

// ---------------------------------------------------------------------------
// Options: server JSON -> browser
// ---------------------------------------------------------------------------

type DescriptorJSON = {id: string; type: string; transports?: string[]};

function decodeDescriptors(list: DescriptorJSON[] | undefined): PublicKeyCredentialDescriptor[] | undefined {
  return list?.map(descriptor => ({
    ...descriptor,
    type: 'public-key' as const,
    id: base64urlToBuffer(descriptor.id),
    transports: descriptor.transports as AuthenticatorTransport[] | undefined,
  }));
}

export function toCreationOptions(
  response: CreationChallengeResponse,
  signal?: AbortSignal,
): CredentialCreationOptions {
  const json = stripNulls(response.publicKey);
  const parse = pkcStatic()?.parseCreationOptionsFromJSON;
  let publicKey: PublicKeyCredentialCreationOptions | undefined;
  if (typeof parse === 'function') {
    try {
      publicKey = parse.call(pkcStatic(), json);
    } catch {
      publicKey = undefined;
    }
  }
  if (!publicKey) {
    const raw = json as unknown as Record<string, unknown> & {
      challenge: string;
      user: {id: string; name: string; displayName: string};
      excludeCredentials?: DescriptorJSON[];
    };
    publicKey = {
      ...(raw as unknown as PublicKeyCredentialCreationOptions),
      challenge: base64urlToBuffer(raw.challenge),
      user: {...raw.user, id: base64urlToBuffer(raw.user.id)},
      excludeCredentials: decodeDescriptors(raw.excludeCredentials),
    };
  }
  return signal ? {publicKey, signal} : {publicKey};
}

export function toRequestOptions(
  response: RequestChallengeResponse,
  options: {signal?: AbortSignal; conditional?: boolean} = {},
): CredentialRequestOptions {
  const json = stripNulls(response.publicKey);
  const parse = pkcStatic()?.parseRequestOptionsFromJSON;
  let publicKey: PublicKeyCredentialRequestOptions | undefined;
  if (typeof parse === 'function') {
    try {
      publicKey = parse.call(pkcStatic(), json);
    } catch {
      publicKey = undefined;
    }
  }
  if (!publicKey) {
    const raw = json as unknown as Record<string, unknown> & {
      challenge: string;
      allowCredentials?: DescriptorJSON[];
    };
    publicKey = {
      ...(raw as unknown as PublicKeyCredentialRequestOptions),
      challenge: base64urlToBuffer(raw.challenge),
      allowCredentials: decodeDescriptors(raw.allowCredentials),
    };
  }
  const result: CredentialRequestOptions = {publicKey};
  if (options.signal) result.signal = options.signal;
  if (options.conditional) result.mediation = 'conditional';
  return result;
}

// ---------------------------------------------------------------------------
// Credentials: browser -> server JSON
// ---------------------------------------------------------------------------

type JsonableCredential = PublicKeyCredential & {toJSON?: () => unknown};

export function serializeRegistration(credential: PublicKeyCredential): RegistrationCredentialJSON {
  const withJson = credential as JsonableCredential;
  if (typeof withJson.toJSON === 'function') {
    try {
      return withJson.toJSON() as unknown as RegistrationCredentialJSON;
    } catch {
      // Fall through to the manual path.
    }
  }
  const response = credential.response as AuthenticatorAttestationResponse;
  return {
    id: credential.id,
    rawId: bufferToBase64url(credential.rawId),
    type: 'public-key',
    response: {
      clientDataJSON: bufferToBase64url(response.clientDataJSON),
      attestationObject: bufferToBase64url(response.attestationObject),
      transports:
        typeof response.getTransports === 'function' ? response.getTransports() : undefined,
    },
    authenticatorAttachment: credential.authenticatorAttachment ?? null,
    clientExtensionResults: credential.getClientExtensionResults() as Record<string, unknown>,
  };
}

export function serializeAuthentication(credential: PublicKeyCredential): AuthenticationCredentialJSON {
  const withJson = credential as JsonableCredential;
  if (typeof withJson.toJSON === 'function') {
    try {
      return withJson.toJSON() as unknown as AuthenticationCredentialJSON;
    } catch {
      // Fall through to the manual path.
    }
  }
  const response = credential.response as AuthenticatorAssertionResponse;
  return {
    id: credential.id,
    rawId: bufferToBase64url(credential.rawId),
    type: 'public-key',
    response: {
      clientDataJSON: bufferToBase64url(response.clientDataJSON),
      authenticatorData: bufferToBase64url(response.authenticatorData),
      signature: bufferToBase64url(response.signature),
      userHandle: response.userHandle ? bufferToBase64url(response.userHandle) : null,
    },
    authenticatorAttachment: credential.authenticatorAttachment ?? null,
    clientExtensionResults: credential.getClientExtensionResults() as Record<string, unknown>,
  };
}

// ---------------------------------------------------------------------------
// One-call ceremonies
// ---------------------------------------------------------------------------

export class PasskeyError extends Error {
  override readonly name = 'PasskeyError';
  readonly kind: 'cancelled' | 'not-allowed' | 'invalid-state' | 'security' | 'unsupported' | 'unknown';

  constructor(kind: PasskeyError['kind'], message: string) {
    super(message);
    this.kind = kind;
  }
}

function toPasskeyError(error: unknown): PasskeyError {
  if (error instanceof PasskeyError) return error;
  if (error instanceof DOMException) {
    switch (error.name) {
      case 'AbortError':
        return new PasskeyError('cancelled', 'The passkey request was cancelled.');
      case 'NotAllowedError':
        return new PasskeyError(
          'not-allowed',
          'The passkey request was dismissed or timed out. Try again when you are ready.',
        );
      case 'InvalidStateError':
        return new PasskeyError(
          'invalid-state',
          'This device already has a passkey for this account.',
        );
      case 'SecurityError':
        return new PasskeyError(
          'security',
          'This address cannot use passkeys. Open PopcornStash at its HTTPS address.',
        );
      case 'NotSupportedError':
        return new PasskeyError('unsupported', 'This browser cannot create that kind of passkey.');
      default:
        break;
    }
  }
  return new PasskeyError('unknown', 'The passkey could not be used. Try again.');
}

/** Runs `navigator.credentials.create` for a webauthn-rs creation challenge. */
export async function createPasskey(
  options: CreationChallengeResponse,
  signal?: AbortSignal,
): Promise<RegistrationCredentialJSON> {
  const support = passkeySupport();
  if (!support.ok) {
    throw new PasskeyError('unsupported', 'Passkeys are not available on this page.');
  }
  try {
    const credential = await navigator.credentials.create(toCreationOptions(options, signal));
    if (!credential || !(credential instanceof PublicKeyCredential)) {
      throw new PasskeyError('unknown', 'No passkey was created.');
    }
    return serializeRegistration(credential);
  } catch (error) {
    throw toPasskeyError(error);
  }
}

/** Runs `navigator.credentials.get` for a webauthn-rs request challenge. */
export async function getPasskey(
  options: RequestChallengeResponse,
  config: {signal?: AbortSignal; conditional?: boolean} = {},
): Promise<AuthenticationCredentialJSON> {
  const support = passkeySupport();
  if (!support.ok) {
    throw new PasskeyError('unsupported', 'Passkeys are not available on this page.');
  }
  try {
    const credential = await navigator.credentials.get(toRequestOptions(options, config));
    if (!credential || !(credential instanceof PublicKeyCredential)) {
      throw new PasskeyError('unknown', 'No passkey was chosen.');
    }
    return serializeAuthentication(credential);
  } catch (error) {
    throw toPasskeyError(error);
  }
}
