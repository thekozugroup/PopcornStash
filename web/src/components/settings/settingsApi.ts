/**
 * Small typed wrappers over `request()` for the places where the shared
 * client's types do not match what the server sends. Each one says which gap
 * it covers so it can be removed once `src/api` is corrected.
 */
import {me, request} from '../../api/client';
import type {Item, ItemCategory, ItemKind, Role, Session, TrashEntry, UnixSeconds} from '../../api/types';

// ---------------------------------------------------------------------------
// Invites. `GET /api/invites` lists invites without their code or link (the
// server only keeps a hash), so the shared `Invite` type (with `url`, `code`)
// does not describe the list. This is the list row as the server sends it.
// ---------------------------------------------------------------------------

export interface InviteListEntry {
  id: string;
  role: Role;
  created_by?: string | null;
  created_by_name?: string | null;
  created_at?: UnixSeconds;
  expires_at: UnixSeconds;
  used_at?: UnixSeconds | null;
  used_by_name?: string | null;
  /** `active`, `used` or `expired`. Older servers may leave it out. */
  status?: 'active' | 'used' | 'expired';
}

export async function listOpenInvites(signal?: AbortSignal): Promise<InviteListEntry[]> {
  const rows = await request<InviteListEntry[]>('/api/invites', {signal});
  const nowSeconds = Date.now() / 1000;
  return rows.filter(invite =>
    invite.status ? invite.status === 'active' : !invite.used_at && invite.expires_at > nowSeconds,
  );
}

// ---------------------------------------------------------------------------
// Trash. The server sends `{item, original_rel_path, trashed_at, trashed_by}`
// while the shared `TrashEntry` type has `item_id` and `trashed_rel_path`.
// Accept both and give the page one shape.
// ---------------------------------------------------------------------------

export interface TrashRow {
  id: string;
  title: string;
  kind: ItemKind | null;
  category: ItemCategory | null;
  thumb: string | null;
  originalPath: string;
  trashedAt: UnixSeconds;
}

type ServerTrashEntry = {
  item?: Partial<Item> & {id: string};
  original_rel_path: string;
  trashed_at: UnixSeconds;
} & Partial<TrashEntry>;

function fileName(path: string): string {
  const parts = path.split('/').filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export async function listTrash(signal?: AbortSignal): Promise<TrashRow[]> {
  const rows = await request<ServerTrashEntry[]>('/api/trash', {signal});
  return rows
    .map(row => {
      const id = row.item?.id ?? row.item_id ?? '';
      return {
        id,
        title: row.item?.title || fileName(row.original_rel_path),
        kind: row.item?.kind ?? null,
        category: row.item?.category ?? null,
        thumb: row.item?.thumb ?? null,
        originalPath: row.original_rel_path,
        trashedAt: row.trashed_at,
      };
    })
    .filter(row => row.id !== '')
    .sort((a, b) => b.trashedAt - a.trashedAt);
}

// ---------------------------------------------------------------------------
// Sessions. The shared client has `me.revokeAllSessions()` (DELETE
// /api/me/sessions), but the server has no such route; revoke the others one
// by one instead.
// ---------------------------------------------------------------------------

export interface RevokeOthersResult {
  revoked: string[];
  /** The first failure, when some sessions stayed signed in. */
  failure: unknown;
}

export async function revokeOtherSessions(sessions: Session[]): Promise<RevokeOthersResult> {
  const others = sessions.filter(session => !session.current);
  const results = await Promise.allSettled(others.map(session => me.revokeSession(session.id)));
  const revoked: string[] = [];
  let failure: unknown = undefined;
  results.forEach((result, index) => {
    const session = others[index];
    if (!session) return;
    if (result.status === 'fulfilled') revoked.push(session.id);
    else failure ??= result.reason;
  });
  return {revoked, failure};
}
