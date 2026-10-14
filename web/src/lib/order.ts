/**
 * Client-side ordering that matches the server's, for merging live arrivals into
 * a list the server paged (docs/architecture.md section 4, Rules for clients).
 */
import type {Item} from '../api/types';
import {takenSortKey} from './format';

type Ordered = Pick<Item, 'id' | 'taken_local' | 'added_at'>;

function compareIds(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/** `sort=added&order=desc`: newest first, then the larger id. */
export function newestAddedFirst(a: Ordered, b: Ordered): number {
  return b.added_at - a.added_at || compareIds(b.id, a.id);
}

/**
 * `sort=taken&order=desc`: the camera's clock (else when it was added, read in UTC)
 * newest first, then the larger id. Never by `taken_at`, which can put a photo in a
 * different order from the clocks around it.
 */
export function newestTakenFirst(a: Ordered, b: Ordered): number {
  const ka = takenSortKey(a);
  const kb = takenSortKey(b);
  return kb < ka ? -1 : kb > ka ? 1 : compareIds(b.id, a.id);
}
