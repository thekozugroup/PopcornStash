/**
 * Everything the extension remembers, in chrome.storage.local only (never
 * storage.sync: the device token must stay on this computer).
 */

export interface StoredConnection {
  /** Origin, e.g. "https://stash.example.com". */
  server: string;
  /** Device token (`pst_…`). Never logged, never shown. */
  token: string;
  /** Server host name reported at pairing time. */
  serverName: string;
  scopes: string[];
  /** Unix milliseconds. */
  pairedAt: number;
}

export interface Prefs {
  /** "Send all my browser downloads to PopcornStash". */
  interceptDownloads: boolean;
  /** "Include this site's sign-in cookies so protected downloads work". */
  includeCookies: boolean;
}

/** A pairing waiting for approval, kept so reopening the options page resumes it. */
export interface PendingPairing {
  server: string;
  pairingId: string;
  pollSecret: string;
  userCode: string;
  verifyUrl: string;
  /** Unix seconds. */
  expiresAt: number;
  /** Seconds between polls. */
  interval: number;
}

/** Why the connection ended without the person asking. */
export type ConnectionNotice = {kind: 'revoked'; server: string; at: number} | null;

interface StorageShape {
  connection?: StoredConnection;
  prefs?: Partial<Prefs>;
  pairing?: PendingPairing;
  notice?: ConnectionNotice;
  /** The last server address typed, to prefill the form. */
  lastServer?: string;
}

export const DEFAULT_PREFS: Prefs = {interceptDownloads: false, includeCookies: false};

async function read<K extends keyof StorageShape>(key: K): Promise<StorageShape[K]> {
  const result = (await chrome.storage.local.get(key)) as StorageShape;
  return result[key];
}

export async function getConnection(): Promise<StoredConnection | null> {
  const value = await read('connection');
  if (!value || typeof value.server !== 'string' || typeof value.token !== 'string' || value.token === '') return null;
  return value;
}

export async function saveConnection(connection: StoredConnection): Promise<void> {
  await chrome.storage.local.set({connection, lastServer: connection.server, notice: null});
  await chrome.storage.local.remove('pairing');
}

/** Forget the token. `notice` explains why when the person did not ask for it. */
export async function forgetConnection(notice: ConnectionNotice = null): Promise<void> {
  // The notice is written first so listeners that see the connection go can read why.
  await chrome.storage.local.set({notice});
  await chrome.storage.local.remove('connection');
}

export async function getPrefs(): Promise<Prefs> {
  const stored = (await read('prefs')) ?? {};
  return {
    interceptDownloads: stored.interceptDownloads === true,
    includeCookies: stored.includeCookies === true,
  };
}

export async function setPref<K extends keyof Prefs>(key: K, value: Prefs[K]): Promise<Prefs> {
  const prefs = {...(await getPrefs()), [key]: value};
  await chrome.storage.local.set({prefs});
  return prefs;
}

export async function getPairing(): Promise<PendingPairing | null> {
  return (await read('pairing')) ?? null;
}

export async function savePairing(pairing: PendingPairing): Promise<void> {
  await chrome.storage.local.set({pairing, lastServer: pairing.server});
}

export async function clearPairing(): Promise<void> {
  await chrome.storage.local.remove('pairing');
}

export async function getNotice(): Promise<ConnectionNotice> {
  return (await read('notice')) ?? null;
}

export async function getLastServer(): Promise<string | null> {
  return (await read('lastServer')) ?? null;
}
