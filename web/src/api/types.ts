/**
 * Wire types for the PopcornStash HTTP API.
 *
 * Mirrors docs/architecture.md section 8 exactly: JSON, snake_case, times in
 * unix seconds, ids as UUIDv7 strings. When the contract changes, update the
 * document and this file in the same commit.
 */

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

/** UUIDv7 string. */
export type Id = string;
/** Unix time in seconds. */
export type UnixSeconds = number;

export type ErrorCode =
  | 'bad_request'
  | 'unauthorized'
  | 'forbidden'
  | 'not_found'
  | 'conflict'
  | 'rate_limited'
  | 'setup_required'
  | 'passkeys_unavailable'
  | 'vpn_unavailable'
  | 'internal';

/** `{"error": {"code": "not_found", "message": "Human readable sentence."}}` */
export interface ApiErrorBody {
  error: {code: ErrorCode; message: string};
}

/** Cursor pagination: `?limit=50&cursor=<opaque>`. */
export interface Page<T> {
  items: T[];
  next_cursor: string | null;
}

export interface PageParams {
  limit?: number;
  cursor?: string | null;
}

// ---------------------------------------------------------------------------
// Shared shapes
// ---------------------------------------------------------------------------

export type Role = 'owner' | 'member';

export interface User {
  id: Id;
  display_name: string;
  role: Role;
  created_at: UnixSeconds;
}

export interface Passkey {
  id: Id;
  name: string;
  created_at: UnixSeconds;
  last_used_at: UnixSeconds | null;
}

export type SessionKind = 'cookie' | 'bearer';

export interface Session {
  id: Id;
  kind: SessionKind;
  created_at: UnixSeconds;
  last_seen_at: UnixSeconds;
  user_agent: string | null;
  ip: string | null;
  current: boolean;
}

export type TokenScope =
  | 'full'
  | 'downloads:create'
  | 'downloads:read'
  | 'library:read';

export type ClientKind = 'chrome-extension' | 'android' | 'other';

export interface Token {
  id: Id;
  name: string;
  prefix: string;
  scopes: TokenScope[];
  client_kind: string;
  created_at: UnixSeconds;
  last_used_at: UnixSeconds | null;
}

/** Returned once, on creation. */
export type CreatedToken = Token & {token: string};

export type ItemKind = 'video' | 'photo' | 'audio' | 'other';
export type ItemCategory =
  | 'movie'
  | 'episode'
  | 'home_video'
  | 'photo'
  | 'other';
export type ItemStatus = 'ready' | 'needs_review' | 'missing' | 'trashed';

export interface ItemProgress {
  position_s: number;
  duration_s: number;
  finished: boolean;
}

export interface Item {
  id: Id;
  kind: ItemKind;
  category: ItemCategory;
  status: ItemStatus;
  title: string;
  year: number | null;
  show_name: string | null;
  season: number | null;
  episode: number | null;
  episode_title: string | null;
  rel_path: string;
  size: number;
  duration_s: number | null;
  width: number | null;
  height: number | null;
  taken_at: UnixSeconds | null;
  camera: string | null;
  /** Thumbnail URL, or null when not ready. */
  thumb: string | null;
  favorite: boolean;
  added_at: UnixSeconds;
  progress: ItemProgress | null;
}

/** `items.media_json` summary (architecture section 6). */
export interface MediaSummary {
  container: string;
  duration_s: number | null;
  bitrate: number | null;
  video: MediaVideoStream | null;
  audio: MediaAudioStream[];
  subtitles: MediaSubtitleStream[];
  tags: Record<string, string>;
}

export interface MediaVideoStream {
  codec: string;
  profile: string | null;
  pix_fmt: string | null;
  width: number;
  height: number;
  fps: number | null;
  hdr: string | null;
  rotation: number | null;
}

export interface MediaAudioStream {
  index: number;
  codec: string;
  channels: number | null;
  language: string | null;
  title: string | null;
}

export interface MediaSubtitleStream {
  index: number;
  codec: string;
  language: string | null;
  title: string | null;
  external: boolean;
}

export type Confidence = 'high' | 'low';

export interface Classification {
  category: ItemCategory;
  title: string;
  year: number | null;
  show_name: string | null;
  season: number | null;
  episode: number | null;
  episode_title: string | null;
  taken_at: UnixSeconds | null;
  confidence: Confidence;
  reason: string;
  /** Relative path the file would move to. */
  destination: string;
}

export type ItemDetail = Item & {
  media: MediaSummary | null;
  mime: string;
  suggestion: Classification | null;
  source_download_id: Id | null;
  next_episode_id: Id | null;
  previous_episode_id: Id | null;
};

export type DownloadKind = 'torrent' | 'http' | 'upload';
export type DownloadState =
  | 'queued'
  | 'downloading'
  | 'paused'
  | 'seeding'
  | 'completed'
  | 'sorting'
  | 'sorted'
  | 'failed'
  | 'cancelled';

export interface Download {
  id: Id;
  kind: DownloadKind;
  name: string;
  source: string;
  state: DownloadState;
  /** 0..1 */
  progress: number;
  total_bytes: number | null;
  done_bytes: number;
  down_bps: number;
  up_bps: number;
  peers: number | null;
  eta_s: number | null;
  error: string | null;
  created_at: UnixSeconds;
  completed_at: UnixSeconds | null;
  item_ids: Id[];
}

export type VpnState =
  | 'connected'
  | 'connecting'
  | 'disconnected'
  | 'error'
  | 'not_configured'
  | 'unreachable';
export type VpnProvider = 'nordvpn' | 'wireguard' | 'openvpn';
export type VpnProtocol = 'wireguard' | 'openvpn';

export interface VpnStatus {
  configured: boolean;
  /** torrentd answered */
  reachable: boolean;
  state: VpnState;
  provider: VpnProvider | null;
  protocol: VpnProtocol | null;
  label: string | null;
  server: string | null;
  country: string | null;
  public_ip: string | null;
  protected: boolean | null;
  since: UnixSeconds | null;
  error: string | null;
  kill_switch: boolean;
  allow_no_vpn: boolean;
}

// ---------------------------------------------------------------------------
// WebAuthn (webauthn-rs JSON shapes, base64url binary fields)
// ---------------------------------------------------------------------------

/** webauthn-rs `CreationChallengeResponse`. */
export interface CreationChallengeResponse {
  publicKey: PublicKeyCredentialCreationOptionsJSON;
  mediation?: CredentialMediationRequirement | null;
}

/** webauthn-rs `RequestChallengeResponse`. */
export interface RequestChallengeResponse {
  publicKey: PublicKeyCredentialRequestOptionsJSON;
  mediation?: CredentialMediationRequirement | null;
}

/** Serialized `PublicKeyCredential` from `navigator.credentials.create`. */
export interface RegistrationCredentialJSON {
  id: string;
  rawId: string;
  type: 'public-key';
  response: {
    clientDataJSON: string;
    attestationObject: string;
    transports?: string[];
  };
  authenticatorAttachment?: string | null;
  /** webauthn-rs reads this under its `clientExtensionResults` alias. */
  clientExtensionResults: Record<string, unknown>;
}

/** Serialized `PublicKeyCredential` from `navigator.credentials.get`. */
export interface AuthenticationCredentialJSON {
  id: string;
  rawId: string;
  type: 'public-key';
  response: {
    clientDataJSON: string;
    authenticatorData: string;
    signature: string;
    userHandle: string | null;
  };
  authenticatorAttachment?: string | null;
  /** webauthn-rs reads this under its `clientExtensionResults` alias. */
  clientExtensionResults: Record<string, unknown>;
}

export interface CreationFlow {
  flow_id: string;
  options: CreationChallengeResponse;
}

export interface RequestFlow {
  flow_id: string;
  options: RequestChallengeResponse;
}

// ---------------------------------------------------------------------------
// Public endpoints
// ---------------------------------------------------------------------------

export interface HealthResponse {
  status: 'ok';
  version: string;
}

export interface AuthState {
  setup_required: boolean;
  authenticated: boolean;
  user: User | null;
  public_url: string | null;
  rp_id: string | null;
  passkeys_ready: boolean;
  reason: string | null;
}

export interface SetupStartRequest {
  setup_code: string;
  display_name: string;
}

export interface RegistrationFinishRequest {
  flow_id: string;
  credential: RegistrationCredentialJSON;
  passkey_name?: string;
}

export interface UserResponse {
  user: User;
}

export interface LoginFinishRequest {
  flow_id: string;
  credential: AuthenticationCredentialJSON;
  want_token?: boolean;
  device_name?: string;
}

export interface LoginFinishResponse {
  user: User;
  token?: string;
}

export interface InviteStartRequest {
  code: string;
  display_name: string;
}

export interface RecoverStartRequest {
  code: string;
}

export interface InviteInfo {
  valid: boolean;
  /** Null when the invite is not valid. */
  role: Role | null;
  expires_at: UnixSeconds | null;
}

export interface PairingRequestBody {
  client_name: string;
  client_kind: ClientKind;
}

export interface PairingRequestResponse {
  pairing_id: string;
  user_code: string;
  poll_secret: string;
  expires_at: UnixSeconds;
  verify_url: string;
}

export interface PairingPollBody {
  pairing_id: string;
  poll_secret: string;
}

export type PairingPollResponse =
  | {status: 'pending'}
  | {status: 'approved'; token: string; scopes: TokenScope[]; server_name: string}
  | {status: 'denied' | 'expired'};

// ---------------------------------------------------------------------------
// Authenticated endpoints
// ---------------------------------------------------------------------------

export interface PasskeyAddStartRequest {
  passkey_name?: string;
}

export interface PasskeyRenameRequest {
  name: string;
}

export interface TokenCreateRequest {
  name: string;
  scopes: TokenScope[];
}

export interface PendingPairing {
  pairing_id: string;
  user_code: string;
  client_name: string;
  client_kind: ClientKind;
  ip: string | null;
  created_at: UnixSeconds;
}

export interface PairingDecisionRequest {
  user_code: string;
}

export interface OkResponse {
  ok: true;
}

export interface LibrarySummary {
  counts: {
    movies: number;
    shows: number;
    episodes: number;
    home_videos: number;
    photos: number;
    other: number;
  };
  total_bytes: number;
  needs_review: number;
  recently_added: Item[];
  continue_watching: Item[];
}

export type ItemSort = 'added' | 'title' | 'taken' | 'size';
export type SortOrder = 'asc' | 'desc';

export interface ItemQuery extends PageParams {
  category?: ItemCategory;
  q?: string;
  sort?: ItemSort;
  order?: SortOrder;
  favorite?: boolean;
  year?: number;
  status?: ItemStatus;
}

export interface ItemPatch {
  title?: string;
  year?: number | null;
  favorite?: boolean;
  show_name?: string | null;
  season?: number | null;
  episode?: number | null;
  episode_title?: string | null;
}

export interface OrganizeRequest {
  category: ItemCategory;
  title?: string;
  year?: number | null;
  show_name?: string | null;
  season?: number | null;
  episode?: number | null;
}

export type ThumbSize = 'sm' | 'lg';

export interface PlaybackRequest {
  video_codecs: string[];
  audio_codecs: string[];
  hdr: boolean;
  max_height?: number;
  start_s?: number;
  audio_index?: number;
}

export type PlaybackResponse =
  | {mode: 'direct'; url: string}
  | {mode: 'hls'; url: string; session_id: string};

export interface ProgressUpdate {
  position_s: number;
  duration_s: number;
}

export interface ShowSummary {
  show_name: string;
  item_count: number;
  season_count: number;
  poster_item_id: Id | null;
  last_added_at: UnixSeconds;
}

export interface ShowDetail {
  show_name: string;
  seasons: {season: number; episodes: Item[]}[];
}

export interface PhotoMonth {
  /** "2024-06" */
  month: string;
  count: number;
  items: Item[];
}

export interface PhotoTimeline {
  months: PhotoMonth[];
  next_cursor: string | null;
}

export interface InboxResponse {
  items: ItemDetail[];
}

export type InboxAction = 'accept' | 'ignore';

export interface InboxDecision {
  action: InboxAction;
  category?: ItemCategory;
  title?: string;
  year?: number | null;
  show_name?: string | null;
  season?: number | null;
  episode?: number | null;
}

export interface JobAccepted {
  job_id: number;
}

export interface TrashEntry {
  item: Item;
  original_rel_path: string;
  trashed_at: UnixSeconds;
  trashed_by: Id | null;
}

export type DownloadFilter = 'active' | 'finished' | 'all';

export interface DownloadList {
  items: Download[];
}

export interface DownloadCreateRequest {
  url: string;
  filename?: string;
  headers?: {cookie?: string; referer?: string; user_agent?: string};
}

export interface UploadCreateRequest {
  filename: string;
  size: number;
  mime?: string;
  last_modified?: UnixSeconds;
}

export interface UploadCreated {
  id: Id;
  chunk_size: number;
  received: number;
}

export interface UploadReceived {
  received: number;
}

export type UploadState = 'receiving' | 'complete' | 'cancelled';

export interface UploadStatus {
  id: Id;
  filename: string;
  size: number;
  received: number;
  state: UploadState;
}

export interface NordVpnCountry {
  id: number;
  name: string;
  code: string;
}

export interface NordVpnSetupRequest {
  access_token: string;
  country_id?: number;
  protocol: VpnProtocol;
}

export interface WireGuardConfigRequest {
  config: string;
}

export type HttpViaVpn = 'never' | 'when_available' | 'always';
export type HwAccel = 'none' | 'vaapi' | 'qsv' | 'nvenc';

/** Settings object (architecture section 6). Partial PUT merges. */
export interface Settings {
  auto_sort: boolean;
  http_via_vpn: HttpViaVpn;
  max_concurrent_downloads: number;
  torrent: {
    seed_after_complete: boolean;
    seed_ratio: number;
    seed_hours: number;
    max_active: number;
    down_limit_kbps: number;
    up_limit_kbps: number;
  };
  transcode: {
    hwaccel: HwAccel;
    max_height: number;
    crf: number;
    max_sessions: number;
  };
  trash_retention_days: number;
}

export type SettingsPatch = Partial<
  Omit<Settings, 'torrent' | 'transcode'> & {
    torrent: Partial<Settings['torrent']>;
    transcode: Partial<Settings['transcode']>;
  }
>;

export interface SystemInfo {
  version: string;
  stash_root: string;
  config_dir: string;
  disk: {total: number; free: number};
  ffmpeg_version: string | null;
  hwaccel: {configured: HwAccel; available: string[]};
  torrentd: {reachable: boolean};
  jobs: {queued: number; running: number; failed: number};
  public_url: string | null;
  passkeys_ready: boolean;
}

export interface UserRolePatch {
  role: Role;
}

/** `POST /api/invites` response. The code and link are only ever shown here. */
export interface Invite {
  id: Id;
  url: string;
  code: string;
  role: Role;
  expires_at: UnixSeconds;
}

export type InviteStatus = 'active' | 'used' | 'expired';

/** One row of `GET /api/invites`. */
export interface InviteListEntry {
  id: Id;
  role: Role;
  created_by: Id | null;
  created_by_name: string | null;
  created_at: UnixSeconds;
  expires_at: UnixSeconds;
  used_at: UnixSeconds | null;
  used_by: Id | null;
  used_by_name: string | null;
  status: InviteStatus;
}

export interface InviteCreateRequest {
  role: Role;
  expires_in_hours: number;
}

export interface AuditUser {
  id: Id;
  display_name: string | null;
}

export interface AuditEntry {
  id: number;
  at: UnixSeconds;
  user: AuditUser | null;
  event: string;
  detail: Record<string, unknown> | null;
  ip: string | null;
}

// ---------------------------------------------------------------------------
// Server-sent events (`GET /api/events`)
// ---------------------------------------------------------------------------

export interface LibraryEvent {
  type: 'added' | 'updated' | 'removed';
  item_id: Id;
}

export interface InboxEvent {
  count: number;
}

export interface JobEvent {
  kind: string;
  state: string;
  item_id: Id | null;
}

export interface StashEventMap {
  download: Download;
  library: LibraryEvent;
  vpn: VpnStatus;
  inbox: InboxEvent;
  job: JobEvent;
}

export type StashEventName = keyof StashEventMap;
