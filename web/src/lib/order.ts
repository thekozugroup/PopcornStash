/**
 * Client-side ordering that matches the server's, for merging live arrivals into
 * a list the server paged (docs/architecture.md section 4, Rules for clients).
 */
import type {Item, PhotoMonth} from '../api/types';
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

/**
 * A list item whose details changed, put where the server would list it now. It leaves
 * its old place; when more pages are still to load and it now sorts after the last item
 * shown, it waits for the page that brings it (the cursor is past the loaded items, so
 * that page will), otherwise it goes in order among the loaded items.
 */
export function placeInOrder<T extends Ordered>(
  list: readonly T[],
  fresh: T,
  compare: (a: T, b: T) => number,
  hasMore: boolean,
): T[] {
  const rest = list.filter(item => item.id !== fresh.id);
  const last = rest.at(-1);
  if (hasMore && last && compare(fresh, last) > 0) return rest;
  const at = rest.findIndex(item => compare(fresh, item) < 0);
  return at < 0 ? [...rest, fresh] : [...rest.slice(0, at), fresh, ...rest.slice(at)];
}

/**
 * A photo whose capture time changed (a probe that read its EXIF, the 0003 redate), put
 * under the month of its camera clock and in order there, as the timeline lists it
 * (`month` is the first seven characters of `taken_local`, else of `added_at` read in
 * UTC). Each month's `count` stays the month's full total: the month it left loses one,
 * the month it joins gains one, even when the photo waits for a later page because it
 * now sorts after the last photo shown.
 */
export function placeInTimeline(months: readonly PhotoMonth[], fresh: Item, hasMore: boolean): PhotoMonth[] {
  const target = takenSortKey(fresh).slice(0, 7);
  const result = months.map(month => {
    const items = month.items.filter(item => item.id !== fresh.id);
    return {...month, items, count: month.count - (month.items.length - items.length)};
  });
  const flat = result.flatMap(month => month.items);
  const placed = placeInOrder(flat, fresh, newestTakenFirst, hasMore).some(item => item.id === fresh.id);
  let month = result.find(candidate => candidate.month === target);
  if (!month) {
    // A month with no photos loaded: shown only when the photo is.
    if (!placed) return result.filter(candidate => candidate.items.length > 0);
    month = {month: target, count: 0, items: []};
    const at = result.findIndex(candidate => candidate.month < target);
    result.splice(at < 0 ? result.length : at, 0, month);
  }
  month.count += 1;
  if (placed) month.items = placeInOrder(month.items, fresh, newestTakenFirst, false);
  return result.filter(candidate => candidate.items.length > 0);
}
