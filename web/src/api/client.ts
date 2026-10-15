/**
 * Typed client for the PopcornStash HTTP API (docs/architecture.md section 8).
 *
 * Every call is same-origin with credentials (the `__Host-stash` session
 * cookie), sends JSON with `Content-Type: application/json` (required by the
 * server's CSRF check for cookie-authenticated unsafe methods), and turns the
 * `{"error": {code, message}}` envelope into an `ApiError`. A 401 from any
 * authenticated endpoint notifies the registered handler so the auth gate can
 * refresh `/api/auth/state` and route to sign-in.
 */
import type {
  AuditEntry,
  AuthState,
  AuthenticationCredentialJSON,
  CreatedToken,
  CreationFlow,
  Download,
  DownloadCreateRequest,
  DownloadFilter,
  DownloadList,
  ErrorCode,
  HealthResponse,
  HomeResponse,
  InboxDecision,
  InboxResponse,
  Invite,
  InviteCreateRequest,
  InviteInfo,
  InviteListEntry,
  InviteStartRequest,
  ItemDetail,
  ItemPatch,
  ItemQuery,
  Item,
  JobAccepted,
  Library,
  LibraryCreateRequest,
  LibraryEditRequest,
  LibraryIcon,
  LibraryList,
  LoginFinishRequest,
  LoginFinishResponse,
  NordVpnCountry,
  NordVpnSetupRequest,
  OkResponse,
  OrganizeRequest,
  Page,
  PageParams,
  PairingPollBody,
  PairingPollResponse,
  PairingRequestBody,
  PairingRequestResponse,
  Passkey,
  PendingPairing,
  PhotoTimeline,
  PlaybackRequest,
  PlaybackResponse,
  ProgressUpdate,
  RecoverStartRequest,
  RegistrationCredentialJSON,
  RegistrationFinishRequest,
  RequestFlow,
  Role,
  Session,
  Settings,
  SettingsPatch,
  SetupStartRequest,
  ShowDetail,
  ShowSummary,
  SystemInfo,
  ThumbSize,
  Token,
  TokenCreateRequest,
  TrashEntry,
  UnassignedFolder,
  UploadCreateRequest,
  UploadCreated,
  UploadReceived,
  UploadStatus,
  User,
  UserResponse,
  VpnStatus,
} from './types';

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/** Client-side failure codes, in addition to the server's `ErrorCode`s. */
export type ClientErrorCode = ErrorCode | 'network' | 'aborted' | 'unknown';

export class ApiError extends Error {
  override readonly name = 'ApiError';
  readonly code: ClientErrorCode;
  readonly status: number;
  /** Parsed response body, when there was one (for example `{received}` on a 409 upload). */
  readonly body: unknown;
  /** The one request field the server blamed (`name`, `folder.name`, `library_id`), if any. */
  readonly field: string | null;

  constructor(code: ClientErrorCode, message: string, status: number, body?: unknown, field?: string | null) {
    super(message);
    this.code = code;
    this.status = status;
    this.body = body;
    this.field = field ?? null;
  }

  get isUnauthorized(): boolean {
    return this.status === 401 || this.code === 'unauthorized';
  }
}

export const STATUS_CODES: Record<number, ErrorCode> = {
  400: 'bad_request',
  401: 'unauthorized',
  403: 'forbidden',
  404: 'not_found',
  405: 'method_not_allowed',
  409: 'conflict',
  413: 'payload_too_large',
  429: 'rate_limited',
};

export const FALLBACK_MESSAGES: Partial<Record<ClientErrorCode, string>> = {
  bad_request: 'The request was not accepted.',
  unauthorized: 'Your session has ended. Sign in again.',
  forbidden: 'Only the owner can do that.',
  not_found: 'That could not be found.',
  conflict: 'That changed in the meantime. Try again.',
  payload_too_large: 'That is too large to send.',
  rate_limited: 'Too many attempts. Wait a minute and try again.',
  network: 'PopcornStash could not be reached. Check the connection and try again.',
  aborted: 'The request was cancelled.',
  internal: 'Something went wrong on the server.',
  unknown: 'Something went wrong.',
};

export function isErrorEnvelope(value: unknown): value is {error: {code: ErrorCode; message: string; field?: string}} {
  if (typeof value !== 'object' || value === null || !('error' in value)) return false;
  const error = (value as {error: unknown}).error;
  return (
    typeof error === 'object' &&
    error !== null &&
    typeof (error as {code?: unknown}).code === 'string' &&
    typeof (error as {message?: unknown}).message === 'string'
  );
}

// ---------------------------------------------------------------------------
// Unauthorized handling
// ---------------------------------------------------------------------------

type UnauthorizedHandler = (error: ApiError) => void;
let unauthorizedHandler: UnauthorizedHandler | null = null;

/** Reports a 401 seen outside `request` (for example an XHR upload) to the auth gate. */
export function notifyUnauthorized(error: ApiError): void {
  unauthorizedHandler?.(error);
}

/** Registers the callback run when an authenticated call answers 401. Returns an unregister function. */
export function setUnauthorizedHandler(handler: UnauthorizedHandler | null): () => void {
  unauthorizedHandler = handler;
  return () => {
    if (unauthorizedHandler === handler) unauthorizedHandler = null;
  };
}

// ---------------------------------------------------------------------------
// Request core
// ---------------------------------------------------------------------------

type QueryValue = string | number | boolean | null | undefined;
export type Query = Record<string, QueryValue>;

export interface RequestOptions {
  method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';
  query?: Query;
  /** JSON body. Serialized with `Content-Type: application/json`. */
  json?: unknown;
  /** Raw body (FormData, Blob, ArrayBuffer). Content-Type is set by the caller or the browser. */
  body?: BodyInit;
  headers?: Record<string, string>;
  signal?: AbortSignal;
  /** Non-2xx statuses that carry a normal response body (for example 410 on pairing poll). */
  acceptStatuses?: number[];
  /** Do not notify the unauthorized handler (auth flows answer 401 for bad credentials). */
  skipAuthRefresh?: boolean;
  /** Let the request outlive the page (final progress save, ending a stream on leave). */
  keepalive?: boolean;
}

export function buildUrl(path: string, query?: Query): string {
  if (!query) return path;
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value === undefined || value === null || value === '') continue;
    params.set(key, String(value));
  }
  const qs = params.toString();
  return qs ? `${path}?${qs}` : path;
}

async function readBody(response: Response): Promise<unknown> {
  if (response.status === 204 || response.status === 205) return undefined;
  const text = await response.text();
  if (!text) return undefined;
  const type = response.headers.get('content-type') ?? '';
  if (type.includes('json') || text.startsWith('{') || text.startsWith('[')) {
    try {
      return JSON.parse(text) as unknown;
    } catch {
      return text;
    }
  }
  return text;
}

/** Performs a request and returns the parsed JSON body (undefined for 204). */
export async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const {method = 'GET', query, json, body, headers = {}, signal, acceptStatuses, skipAuthRefresh, keepalive} =
    options;
  const init: RequestInit = {
    method,
    credentials: 'same-origin',
    headers: {Accept: 'application/json', ...headers},
    signal: signal ?? null,
    keepalive: keepalive ?? false,
  };
  if (json !== undefined) {
    init.body = JSON.stringify(json);
    (init.headers as Record<string, string>)['Content-Type'] = 'application/json';
  } else if (body !== undefined) {
    init.body = body;
  }

  let response: Response;
  try {
    response = await fetch(buildUrl(path, query), init);
  } catch (cause) {
    if (cause instanceof DOMException && cause.name === 'AbortError') {
      throw new ApiError('aborted', FALLBACK_MESSAGES.aborted ?? '', 0);
    }
    throw new ApiError('network', FALLBACK_MESSAGES.network ?? '', 0);
  }

  const parsed = await readBody(response);
  if (response.ok || acceptStatuses?.includes(response.status)) {
    return parsed as T;
  }

  const code: ClientErrorCode = isErrorEnvelope(parsed)
    ? parsed.error.code
    : (STATUS_CODES[response.status] ?? (response.status >= 500 ? 'internal' : 'unknown'));
  const message = isErrorEnvelope(parsed)
    ? parsed.error.message
    : (FALLBACK_MESSAGES[code] ?? FALLBACK_MESSAGES.unknown ?? '');
  const field = isErrorEnvelope(parsed) && typeof parsed.error.field === 'string' ? parsed.error.field : null;
  const error = new ApiError(code, message, response.status, parsed, field);
  if (response.status === 401 && !skipAuthRefresh) {
    unauthorizedHandler?.(error);
  }
  throw error;
}

const get = <T>(path: string, query?: Query, signal?: AbortSignal) =>
  request<T>(path, {query, signal});
const post = <T>(path: string, json?: unknown, extra?: Partial<RequestOptions>) =>
  request<T>(path, {method: 'POST', json: json ?? {}, ...extra});
const put = <T>(path: string, json?: unknown) => request<T>(path, {method: 'PUT', json: json ?? {}});
const patch = <T>(path: string, json: unknown) => request<T>(path, {method: 'PATCH', json});
const del = <T = void>(path: string, query?: Query) => request<T>(path, {method: 'DELETE', query});

const enc = encodeURIComponent;

function pageQuery(params?: PageParams): Query {
  return {limit: params?.limit, cursor: params?.cursor ?? undefined};
}

// ---------------------------------------------------------------------------
// Endpoints, grouped by area (architecture section 8)
// ---------------------------------------------------------------------------

export const health = {
  get: (signal?: AbortSignal) => get<HealthResponse>('/api/health', undefined, signal),
};

export const auth = {
  state: (signal?: AbortSignal) =>
    request<AuthState>('/api/auth/state', {signal, skipAuthRefresh: true}),

  setupStart: (body: SetupStartRequest) =>
    post<CreationFlow>('/api/auth/setup/start', body, {skipAuthRefresh: true}),
  setupFinish: (body: RegistrationFinishRequest) =>
    post<UserResponse>('/api/auth/setup/finish', body, {skipAuthRefresh: true}),

  loginStart: () => post<RequestFlow>('/api/auth/login/start', {}, {skipAuthRefresh: true}),
  loginFinish: (body: LoginFinishRequest) =>
    post<LoginFinishResponse>('/api/auth/login/finish', body, {skipAuthRefresh: true}),

  inviteStart: (body: InviteStartRequest) =>
    post<CreationFlow>('/api/auth/invite/start', body, {skipAuthRefresh: true}),
  inviteFinish: (body: RegistrationFinishRequest) =>
    post<UserResponse>('/api/auth/invite/finish', body, {skipAuthRefresh: true}),

  recoverStart: (body: RecoverStartRequest) =>
    post<CreationFlow>('/api/auth/recover/start', body, {skipAuthRefresh: true}),
  recoverFinish: (body: RegistrationFinishRequest) =>
    post<UserResponse>('/api/auth/recover/finish', body, {skipAuthRefresh: true}),

  logout: () => post<void>('/api/auth/logout', {}, {skipAuthRefresh: true}),
};

export const invites = {
  /** Public: checks an invite code without revealing anything else. */
  check: (code: string, signal?: AbortSignal) =>
    request<InviteInfo>(`/api/invites/${enc(code)}`, {signal, skipAuthRefresh: true}),
  /** Owner: every invite with its status (active, used, expired). */
  list: (signal?: AbortSignal) => get<InviteListEntry[]>('/api/invites', undefined, signal),
  create: (body: InviteCreateRequest) => post<Invite>('/api/invites', body),
  revoke: (id: string) => del(`/api/invites/${enc(id)}`),
};

export const pairing = {
  /** Public: a device (Chrome extension, Android) asks to pair. */
  request: (body: PairingRequestBody) =>
    post<PairingRequestResponse>('/api/pairing/request', body, {skipAuthRefresh: true}),
  /** Public: 202 pending, 200 approved, 410 denied or expired. */
  poll: (body: PairingPollBody) =>
    post<PairingPollResponse>('/api/pairing/poll', body, {
      acceptStatuses: [202, 410],
      skipAuthRefresh: true,
    }),
  pending: (signal?: AbortSignal) => get<PendingPairing[]>('/api/pairing/pending', undefined, signal),
  approve: (userCode: string) => post<OkResponse>('/api/pairing/approve', {user_code: userCode}),
  deny: (userCode: string) => post<OkResponse>('/api/pairing/deny', {user_code: userCode}),
};

export const me = {
  get: (signal?: AbortSignal) => get<User>('/api/me', undefined, signal),

  passkeys: (signal?: AbortSignal) => get<Passkey[]>('/api/me/passkeys', undefined, signal),
  passkeyAddStart: () => post<CreationFlow>('/api/me/passkeys/start', {}),
  passkeyAddFinish: (body: {
    flow_id: string;
    credential: RegistrationCredentialJSON;
    passkey_name?: string;
  }) => post<Passkey>('/api/me/passkeys/finish', body),
  renamePasskey: (id: string, name: string) => patch<Passkey>(`/api/me/passkeys/${enc(id)}`, {name}),
  /** Refused with 409 for the last passkey. */
  deletePasskey: (id: string) => del(`/api/me/passkeys/${enc(id)}`),

  sessions: (signal?: AbortSignal) => get<Session[]>('/api/me/sessions', undefined, signal),
  revokeSession: (id: string) => del(`/api/me/sessions/${enc(id)}`),
};

export const tokens = {
  list: (signal?: AbortSignal) => get<Token[]>('/api/tokens', undefined, signal),
  /** The plaintext `token` is returned once. */
  create: (body: TokenCreateRequest) => post<CreatedToken>('/api/tokens', body),
  revoke: (id: string) => del(`/api/tokens/${enc(id)}`),
};

export const library = {
  rescan: () => post<JobAccepted>('/api/library/rescan', {}),
};

export const home = {
  /** Continue watching, a shelf per library the caller sees, and counts. */
  get: (signal?: AbortSignal) => get<HomeResponse>('/api/home', undefined, signal),
};

export const libraries = {
  /** The libraries the caller sees, in order. */
  list: (signal?: AbortSignal) => get<LibraryList>('/api/libraries', undefined, signal),
  /** 404 for a library the caller cannot see. */
  get: (id: string, signal?: AbortSignal) => get<Library>(`/api/libraries/${enc(id)}`, undefined, signal),
  /** Owner. Errors carry `field`. */
  create: (body: LibraryCreateRequest) => post<Library>('/api/libraries', body),
  /** Owner. Only the fields sent change. */
  update: (id: string, body: LibraryEditRequest) => patch<Library>(`/api/libraries/${enc(id)}`, body),
  /** Owner. Every library exactly once, in the new order. */
  reorder: (ids: string[]) => put<LibraryList>('/api/libraries/order', {ids}),
  /** Owner. Takes the library out of the app; its folder and files stay. */
  remove: (id: string) => del(`/api/libraries/${enc(id)}`),
  /** Owner: top-level folders that are not a library. */
  folders: (signal?: AbortSignal) => get<{folders: UnassignedFolder[]}>('/api/libraries/folders', undefined, signal),
  /** The curated icon names, in picker order. */
  icons: (signal?: AbortSignal) => get<{icons: LibraryIcon[]}>('/api/libraries/icons', undefined, signal),
  /** Episodes libraries only. */
  shows: (id: string, signal?: AbortSignal) =>
    get<ShowSummary[]>(`/api/libraries/${enc(id)}/shows`, undefined, signal),
  show: (id: string, name: string, signal?: AbortSignal) =>
    get<ShowDetail>(`/api/libraries/${enc(id)}/shows/${enc(name)}`, undefined, signal),
  /** Photos libraries only. `limit` is a photo budget. */
  timeline: (id: string, params?: PageParams, signal?: AbortSignal) =>
    get<PhotoTimeline>(`/api/libraries/${enc(id)}/timeline`, pageQuery(params), signal),
};

export const items = {
  list: (query: ItemQuery = {}, signal?: AbortSignal) =>
    get<Page<Item>>(
      '/api/items',
      {
        library_id: query.library_id,
        place: query.place,
        category: query.category,
        q: query.q,
        sort: query.sort,
        order: query.order,
        favorite: query.favorite,
        year: query.year,
        status: query.status,
        ...pageQuery(query),
      },
      signal,
    ),
  get: (id: string, signal?: AbortSignal) => get<ItemDetail>(`/api/items/${enc(id)}`, undefined, signal),
  /** Metadata only; never moves the file. */
  update: (id: string, body: ItemPatch) => patch<ItemDetail>(`/api/items/${enc(id)}`, body),
  /** Moves the file. Explicit user action only. */
  organize: (id: string, body: OrganizeRequest) =>
    post<ItemDetail>(`/api/items/${enc(id)}/organize`, body),
  /** Owner only. Moves the file to `.popcornstash/Trash`. */
  remove: (id: string) => del(`/api/items/${enc(id)}`),
  playback: (id: string, body: PlaybackRequest) =>
    post<PlaybackResponse>(`/api/items/${enc(id)}/playback`, body),
  saveProgress: (id: string, body: ProgressUpdate) => put<void>(`/api/items/${enc(id)}/progress`, body),

  thumbUrl: (id: string, size: ThumbSize = 'sm') => buildUrl(`/api/items/${enc(id)}/thumb`, {size}),
  fileUrl: (id: string, download = false) =>
    buildUrl(`/api/items/${enc(id)}/file`, {download: download ? 1 : undefined}),
  viewUrl: (id: string) => `/api/items/${enc(id)}/view`,
};

export const stream = {
  playlistUrl: (sessionId: string) => `/api/stream/${enc(sessionId)}/index.m3u8`,
  end: (sessionId: string) => del(`/api/stream/${enc(sessionId)}`),
};

export const inbox = {
  list: (signal?: AbortSignal) => get<InboxResponse>('/api/inbox', undefined, signal),
  decide: (id: string, body: InboxDecision) => post<ItemDetail>(`/api/inbox/${enc(id)}`, body),
};

export const trash = {
  /** Owner only: every trashed item with where it was (architecture section 11). */
  list: (signal?: AbortSignal) => get<TrashEntry[]>('/api/trash', undefined, signal),
  /** Moves the file back to where it was and answers the restored item. */
  restore: (id: string) => post<ItemDetail>(`/api/trash/${enc(id)}/restore`, {}),
  /** Deletes everything in the trash for good and answers how many items went. */
  empty: () => del<{deleted: number}>('/api/trash'),
};

export const downloads = {
  list: (state: DownloadFilter = 'all', limit?: number, signal?: AbortSignal) =>
    get<DownloadList>('/api/downloads', {state, limit}, signal),
  /** http(s) URL or magnet link. */
  create: (body: DownloadCreateRequest) => post<Download>('/api/downloads', body),
  /** `.torrent` file up to 10 MB. */
  addTorrent: (file: File, libraryId?: string) => {
    const form = new FormData();
    form.append('file', file, file.name);
    if (libraryId) form.append('library_id', libraryId);
    return request<Download>('/api/downloads/torrent', {method: 'POST', body: form});
  },
  pause: (id: string) => post<Download>(`/api/downloads/${enc(id)}/pause`, {}),
  resume: (id: string) => post<Download>(`/api/downloads/${enc(id)}/resume`, {}),
  remove: (id: string, deleteFiles = false) =>
    del(`/api/downloads/${enc(id)}`, {delete_files: deleteFiles}),
};

export const uploads = {
  create: (body: UploadCreateRequest, signal?: AbortSignal) => post<UploadCreated>('/api/uploads', body, {signal}),
  /** Raw bytes at `offset` (at most 64 MiB). 409 carries `{received}` on an offset mismatch. */
  putChunk: (id: string, offset: number, chunk: Blob, signal?: AbortSignal) =>
    request<UploadReceived>(`/api/uploads/${enc(id)}`, {
      method: 'PUT',
      query: {offset},
      body: chunk,
      headers: {'Content-Type': 'application/octet-stream'},
      signal,
    }),
  status: (id: string, signal?: AbortSignal) =>
    get<UploadStatus>(`/api/uploads/${enc(id)}`, undefined, signal),
  complete: (id: string) => post<Download>(`/api/uploads/${enc(id)}/complete`, {}),
  cancel: (id: string) => del(`/api/uploads/${enc(id)}`),
};

export const vpn = {
  status: (signal?: AbortSignal) => get<VpnStatus>('/api/vpn/status', undefined, signal),
  nordvpnCountries: (signal?: AbortSignal) =>
    get<NordVpnCountry[]>('/api/vpn/nordvpn/countries', undefined, signal),
  /** The access token is used once and never stored by the server. */
  setupNordvpn: (body: NordVpnSetupRequest) => post<VpnStatus>('/api/vpn/nordvpn', body),
  setupWireguardText: (config: string) => post<VpnStatus>('/api/vpn/wireguard', {config}),
  setupWireguardFile: (file: File) => {
    const form = new FormData();
    form.append('file', file, file.name);
    return request<VpnStatus>('/api/vpn/wireguard', {method: 'POST', body: form});
  },
  setupOpenvpn: (file: File, username: string, password: string) => {
    const form = new FormData();
    form.append('file', file, file.name);
    form.append('username', username);
    form.append('password', password);
    return request<VpnStatus>('/api/vpn/openvpn', {method: 'POST', body: form});
  },
  reconnect: () => post<VpnStatus>('/api/vpn/reconnect', {}),
  remove: () => del<VpnStatus>('/api/vpn'),
};

export const settings = {
  get: (signal?: AbortSignal) => get<Settings>('/api/settings', undefined, signal),
  /** Partial update; the server merges. */
  update: (body: SettingsPatch) => put<Settings>('/api/settings', body),
};

export const system = {
  get: (signal?: AbortSignal) => get<SystemInfo>('/api/system', undefined, signal),
};

export const users = {
  list: (signal?: AbortSignal) => get<User[]>('/api/users', undefined, signal),
  setRole: (id: string, role: Role) => patch<User>(`/api/users/${enc(id)}`, {role}),
  remove: (id: string) => del(`/api/users/${enc(id)}`),
};

export const audit = {
  list: (params?: PageParams, signal?: AbortSignal) =>
    get<Page<AuditEntry>>('/api/audit', pageQuery(params), signal),
};

/** Everything above under one namespace: `api.downloads.create(...)`. */
export const api = {
  health,
  auth,
  invites,
  pairing,
  me,
  tokens,
  library,
  home,
  libraries,
  items,
  stream,
  inbox,
  trash,
  downloads,
  uploads,
  vpn,
  settings,
  system,
  users,
  audit,
};

export type {AuthenticationCredentialJSON, RegistrationCredentialJSON};
