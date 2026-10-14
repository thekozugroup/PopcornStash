import {describe, expect, it} from 'vitest';
import {newestAddedFirst, newestTakenFirst} from './order';
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
