/**
 * Settings-page views over the shared API client: the rows the People and
 * Libraries pages list, and "sign out everywhere else".
 */
import {invites, me, trash} from '../../api/client';
import type {InviteListEntry, ItemCategory, ItemKind, Session, UnixSeconds} from '../../api/types';

export type {InviteListEntry};

// ---------------------------------------------------------------------------
// Invites: the ones that can still be used.
// ---------------------------------------------------------------------------

export async function listOpenInvites(signal?: AbortSignal): Promise<InviteListEntry[]> {
  const rows = await invites.list(signal);
  return rows.filter(invite => invite.status === 'active');
}

// ---------------------------------------------------------------------------
// Trash: one row per item, newest first.
// ---------------------------------------------------------------------------

export interface TrashRow {
  id: string;
  fileEntry?: boolean;
  detail?: string;
  title: string;
  kind: ItemKind | null;
  category: ItemCategory | null;
  thumb: string | null;
  originalPath: string;
  trashedAt: UnixSeconds;
  /** Why a restore would be refused now (its library no longer shows this kind of file), else null. */
  restoreRefusal: string | null;
}

function fileName(path: string): string {
  const parts = path.split('/').filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export async function listTrash(signal?: AbortSignal): Promise<TrashRow[]> {
  const rows = await trash.list(signal);
  return rows
    .map(row => ({
      id: row.item.id,
      title: row.item.title || fileName(row.original_rel_path),
      kind: row.item.kind,
      category: row.item.category,
      thumb: row.item.thumb,
      originalPath: row.original_rel_path,
      trashedAt: row.trashed_at,
      restoreRefusal: row.restore_refusal ?? null,
    }))
    .sort((a, b) => b.trashedAt - a.trashedAt);
}

// ---------------------------------------------------------------------------
// Sessions. "Sign out everywhere else" revokes the other sessions one by one.
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
