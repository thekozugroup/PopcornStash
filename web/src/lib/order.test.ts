import {describe, expect, it} from 'vitest';
import type {Item, PhotoMonth} from '../api/types';
import {newestAddedFirst, newestTakenFirst, placeInOrder, placeInTimeline} from './order';
import {sameInEveryZone} from './timeZones.testing';

// Two clocks around the end of June whose moments go the other way: Kiritimati
// (+14:00) is already in July while Los Angeles (-07:00) is still in June, but the
// Los Angeles photo was taken later. The timeline and sort=taken go by the clock.
const kiritimati = {id: 'b', taken_local: '2024-07-01T00:20:00', taken_at: 1_719_742_800, added_at: 100};
const losAngeles = {id: 'a', taken_local: '2024-06-30T20:00:00', taken_at: 1_719_802_800, added_at: 100};
// No capture time: ordered by when it was added, read in UTC (2024-06-30 23:20:00).
const undated = {id: 'c', taken_local: null, taken_at: null, added_at: 1_719_789_600};

describe('merging pages of sort=taken', () => {
  it("orders by the camera's clock, newest first, as the server does", () => {
    expect(losAngeles.taken_at).toBeGreaterThan(kiritimati.taken_at);
    const order = sameInEveryZone(() => [losAngeles, undated, kiritimati].sort(newestTakenFirst).map(item => item.id));
    expect(order).toEqual(['b', 'c', 'a']);
  });

  it('breaks a tie on the larger id first', () => {
    const twin = {...kiritimati, id: 'a2'};
    expect([twin, kiritimati].sort(newestTakenFirst).map(item => item.id)).toEqual(['b', 'a2']);
  });
});

describe('merging pages of sort=added', () => {
  it('orders by when it was added, newest first, then the larger id', () => {
    const items = [
      {id: 'x', taken_local: null, added_at: 5},
      {id: 'y', taken_local: null, added_at: 9},
      {id: 'z', taken_local: null, added_at: 5},
    ];
    expect(items.sort(newestAddedFirst).map(item => item.id)).toEqual(['y', 'z', 'x']);
  });
});

/** A photo row with only what ordering reads filled in meaningfully. */
function photo(id: string, takenLocal: string | null, addedAt = 1_700_000_000): Item {
  return {
    id,
    kind: 'photo',
    category: 'photo',
    status: 'ready',
    title: id,
    year: null,
    show_name: null,
    season: null,
    episode: null,
    episode_title: null,
    rel_path: `Photos/${id}.jpg`,
    size: 1,
    duration_s: null,
    width: null,
    height: null,
    taken_at: null,
    taken_local: takenLocal,
    taken_offset: null,
    camera: null,
    thumb: null,
    favorite: false,
    added_at: addedAt,
    progress: null,
    library_id: 'photos',
    place: 'library',
  } as Item;
}

const ids = (months: PhotoMonth[]) => months.map(m => `${m.month} ${m.count}: ${m.items.map(i => i.id).join(' ')}`);

describe('a photo whose capture time changed', () => {
  // As migration 0003 left them: the old moment read in UTC. The redate moves jan1 back
  // into the new year and the July evening photos back to June 30.
  const loaded: PhotoMonth[] = [
    {month: '2025-01', count: 1, items: [photo('new-year', '2025-01-02T10:00:00')]},
    {
      month: '2024-12',
      count: 2,
      items: [photo('jan1', '2024-12-31T23:30:00'), photo('dec', '2024-12-20T09:00:00')],
    },
    {
      month: '2024-07',
      count: 2,
      items: [photo('sunset', '2024-07-01T02:30:00'), photo('beach', '2024-07-01T01:00:00')],
    },
    {month: '2024-06', count: 40, items: [photo('june', '2024-06-20T12:00:00')]},
  ];

  it('moves to the month of its camera clock, in order, with both counts', () => {
    const jan1 = photo('jan1', '2025-01-01T00:30:00');
    expect(ids(placeInTimeline(loaded, jan1, true))).toEqual([
      '2025-01 2: new-year jan1',
      '2024-12 1: dec',
      '2024-07 2: sunset beach',
      '2024-06 40: june',
    ]);
    const sunset = photo('sunset', '2024-06-30T22:30:00');
    expect(ids(placeInTimeline(loaded, sunset, true))).toEqual([
      '2025-01 1: new-year',
      '2024-12 2: jan1 dec',
      '2024-07 1: beach',
      '2024-06 41: sunset june',
    ]);
  });

  it('drops a month it emptied and starts one it is the first of', () => {
    const months = placeInTimeline(loaded, photo('dec', '2024-09-05T08:00:00'), true);
    expect(ids(months)).toEqual([
      '2025-01 1: new-year',
      '2024-12 1: jan1',
      '2024-09 1: dec',
      '2024-07 2: sunset beach',
      '2024-06 40: june',
    ]);
    const emptied = placeInTimeline(loaded, photo('new-year', '2024-12-25T08:00:00'), true);
    expect(ids(emptied)).toEqual(['2024-12 3: jan1 new-year dec', '2024-07 2: sunset beach', '2024-06 40: june']);
  });

  it('waits for a later page when it now sorts after the last photo shown', () => {
    const older = photo('beach', '2024-06-01T08:00:00');
    expect(ids(placeInTimeline(loaded, older, true))).toEqual([
      '2025-01 1: new-year',
      '2024-12 2: jan1 dec',
      '2024-07 1: sunset',
      '2024-06 41: june',
    ]);
    // With nothing more to load it goes at the end.
    expect(ids(placeInTimeline(loaded, older, false)).at(-1)).toBe('2024-06 41: june beach');
    // An older month that is not loaded yet: it simply leaves.
    expect(ids(placeInTimeline(loaded, photo('dec', '2023-03-01T08:00:00'), true))).toEqual([
      '2025-01 1: new-year',
      '2024-12 1: jan1',
      '2024-07 2: sunset beach',
      '2024-06 40: june',
    ]);
  });

  it('uses when it was added, read in UTC, without a capture time', () => {
    // 2024-12-15T00:00:00Z
    const undated = photo('dec', null, 1_734_220_800);
    expect(ids(placeInTimeline(loaded, undated, true))[1]).toBe('2024-12 2: jan1 dec');
    expect(ids(placeInTimeline(loaded, photo('dec', null, 1_720_000_000), true))[2]).toBe(
      '2024-07 3: dec sunset beach',
    );
  });

  it('keeps a sort=taken grid in order', () => {
    const list = [photo('a', '2024-07-03T00:00:00'), photo('b', '2024-07-02T00:00:00'), photo('c', '2024-07-01T00:00:00')];
    const moved = placeInOrder(list, photo('c', '2024-07-05T00:00:00'), newestTakenFirst, true);
    expect(moved.map(i => i.id)).toEqual(['c', 'a', 'b']);
    const later = placeInOrder(list, photo('a', '2024-06-01T00:00:00'), newestTakenFirst, true);
    expect(later.map(i => i.id)).toEqual(['b', 'c']);
    const last = placeInOrder(list, photo('a', '2024-06-01T00:00:00'), newestTakenFirst, false);
    expect(last.map(i => i.id)).toEqual(['b', 'c', 'a']);
  });
});
