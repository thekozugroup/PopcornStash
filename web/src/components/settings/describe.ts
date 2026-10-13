/**
 * Plain-language names for things the settings pages list: device kinds,
 * token access, browsers from a user agent, and audit events.
 */
import type {AuditEntry, ClientKind, TokenScope} from '../../api/types';

export function clientKindName(kind: ClientKind | string): string {
  switch (kind) {
    case 'chrome-extension':
      return 'Chrome extension';
    case 'android':
      return 'Android app';
    case 'other':
      return 'Other app';
    default:
      return 'App';
  }
}

/** One sentence: what a token with these scopes can do. */
export function scopesInWords(scopes: readonly (TokenScope | string)[]): string {
  const set = new Set(scopes);
  if (set.has('full')) return 'Full access to your account';
  const parts: string[] = [];
  if (set.has('downloads:create') && set.has('downloads:read')) parts.push('add downloads and see their progress');
  else if (set.has('downloads:create')) parts.push('add downloads');
  else if (set.has('downloads:read')) parts.push('see downloads');
  if (set.has('library:read')) parts.push('browse and play the library');
  if (parts.length === 0) return 'No access';
  const sentence = parts.join(', and ');
  return `Can ${sentence}`;
}

/** What a device of this kind will be able to do once paired. */
export function pairingAccess(kind: ClientKind | string): {can: string[]; cannot: string | null} {
  switch (kind) {
    case 'chrome-extension':
      return {
        can: ['Send links, magnets and files to Downloads', 'See how your downloads are doing'],
        cannot: 'It cannot browse or play your library, or change any settings.',
      };
    case 'android':
      return {
        can: ['Everything you can do when you sign in: browse, play, upload and add downloads'],
        cannot: null,
      };
    default:
      return {
        can: ['Send links and magnets to Downloads'],
        cannot: 'It cannot see your library or downloads, or change any settings.',
      };
  }
}

function browserName(ua: string): string | null {
  if (/Edg\//.test(ua)) return 'Edge';
  if (/OPR\/|Opera/.test(ua)) return 'Opera';
  if (/Firefox\/|FxiOS/.test(ua)) return 'Firefox';
  if (/Chrome\/|CriOS/.test(ua)) return 'Chrome';
  if (/Safari\//.test(ua)) return 'Safari';
  return null;
}

function systemName(ua: string): string | null {
  if (/iPhone/.test(ua)) return 'iPhone';
  if (/iPad/.test(ua)) return 'iPad';
  if (/Android/.test(ua)) return 'Android';
  if (/CrOS/.test(ua)) return 'ChromeOS';
  if (/Macintosh|Mac OS X/.test(ua)) return 'Mac';
  if (/Windows/.test(ua)) return 'Windows';
  if (/Linux/.test(ua)) return 'Linux';
  return null;
}

/** "Chrome on Mac", "Safari on iPhone", "App sign-in". */
export function sessionName(kind: string, userAgent: string | null): string {
  if (kind === 'bearer') return 'App sign-in';
  if (!userAgent) return 'Unknown browser';
  const browser = browserName(userAgent);
  const system = systemName(userAgent);
  if (browser && system) return `${browser} on ${system}`;
  return browser ?? system ?? 'Unknown browser';
}

const AUDIT_EVENTS: Record<string, string> = {
  setup_completed: 'Set up PopcornStash',
  login: 'Signed in',
  logout: 'Signed out',
  auth_failed: 'Sign-in failed',
  passkey_added: 'Added a passkey',
  passkey_removed: 'Removed a passkey',
  session_revoked: 'Signed out a browser',
  token_created: 'Created a device token',
  token_revoked: 'Revoked a device token',
  device_paired: 'Paired a device',
  pairing_denied: 'Denied a device',
  invite_created: 'Created an invite',
  invite_revoked: 'Revoked an invite',
  invite_accepted: 'Joined with an invite',
  user_role_changed: 'Changed a role',
  user_removed: 'Removed a person',
  owner_reset_link_created: 'Printed an owner recovery link',
  owner_recovered: 'Recovered owner access',
  settings_changed: 'Changed settings',
  vpn_configured: 'Set up the VPN',
  vpn_removed: 'Removed the VPN',
  item_trashed: 'Moved to trash',
  item_restored: 'Restored from trash',
  trash_emptied: 'Emptied the trash',
};

export function auditEventName(event: string): string {
  const known = AUDIT_EVENTS[event];
  if (known) return known;
  const words = event.replace(/[_.]+/g, ' ').trim();
  return words ? words.charAt(0).toUpperCase() + words.slice(1) : 'Event';
}

const SETTING_NAMES: Record<string, string> = {
  auto_sort: 'auto-sort',
  http_via_vpn: 'web downloads and the VPN',
  max_concurrent_downloads: 'downloads at once',
  trash_retention_days: 'trash retention',
  'torrent.seed_after_complete': 'seeding',
  'torrent.seed_ratio': 'seed ratio',
  'torrent.seed_hours': 'seed time',
  'torrent.max_active': 'active torrents',
  'torrent.down_limit_kbps': 'download limit',
  'torrent.up_limit_kbps': 'upload limit',
  'transcode.hwaccel': 'hardware acceleration',
  'transcode.max_height': 'streaming resolution',
  'transcode.crf': 'streaming quality',
  'transcode.max_sessions': 'streams at once',
};

const AUTH_FAILURE_REASONS: Record<string, string> = {
  wrong_setup_code: 'Wrong setup code',
  passkey_rejected: 'Passkey not accepted',
  unknown_user: 'Passkey not recognised',
  no_user_handle: 'Passkey not recognised',
  invalid_invite: 'Invite link not valid',
  invalid_code: 'Recovery link not valid',
};

const AUTH_FAILURE_FLOWS: Record<string, string> = {
  setup: 'during setup',
  invite: 'while joining',
  recover: 'during owner recovery',
  add_passkey: 'while adding a passkey',
};

/** `auth_failed` detail `{flow, reason}` in words: "Passkey not accepted while joining". */
function authFailure(flow: unknown, reason: unknown): string | null {
  const why = typeof reason === 'string' ? (AUTH_FAILURE_REASONS[reason] ?? null) : null;
  if (!why) return null;
  const where = typeof flow === 'string' && reason === 'passkey_rejected' ? AUTH_FAILURE_FLOWS[flow] : undefined;
  return where ? `${why} ${where}` : why;
}

function text(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value : null;
}

/** A short detail for an audit row, or null when there is nothing useful to add. */
export function auditDetail(entry: Pick<AuditEntry, 'event' | 'detail'>): string | null {
  const detail = entry.detail ?? {};
  switch (entry.event) {
    case 'item_trashed':
    case 'item_restored':
      return text(detail.title);
    case 'device_paired':
    case 'pairing_denied':
      return text(detail.client_name);
    case 'token_created':
      return text(detail.name);
    case 'user_removed':
      return text(detail.display_name);
    case 'user_role_changed':
      return detail.role === 'owner' ? 'Now an owner' : detail.role === 'member' ? 'Now a member' : null;
    case 'invite_created':
      return detail.role === 'owner' ? 'Owner invite' : detail.role === 'member' ? 'Member invite' : null;
    case 'auth_failed':
      return authFailure(detail.flow, detail.reason);
    case 'trash_emptied':
      return typeof detail.deleted === 'number' ? `${detail.deleted} ${detail.deleted === 1 ? 'item' : 'items'}` : null;
    case 'vpn_configured':
      return (
        [text(detail.provider) === 'nordvpn' ? 'NordVPN' : text(detail.provider), text(detail.country)]
          .filter(Boolean)
          .join(', ') || null
      );
    case 'settings_changed': {
      const changed = Array.isArray(detail.changed)
        ? detail.changed.filter((key): key is string => typeof key === 'string')
        : [];
      if (changed.length === 0) return null;
      const names = changed.map(key => SETTING_NAMES[key] ?? key.replace(/[_.]+/g, ' '));
      const sentence = names.join(', ');
      return sentence.charAt(0).toUpperCase() + sentence.slice(1);
    }
    default:
      return null;
  }
}
