import {describe, expect, it} from 'vitest';
import type {HomeShow, Item} from '../../api/types';
import {sameInEveryZone} from '../../lib/timeZones.testing';
import {displayTitle, hasFileNameTitle, itemMeta, showTileMeta, takenOrAddedDate} from './libraryFormat';

const english = new Intl.DateTimeFormat().resolvedOptions().locale.startsWith('en-US');

const photo = {
  id: 'p1',
  kind: 'photo',
  category: 'photo',
  title: 'DSCF0501',
  rel_path: 'Photos/2024/07/DSCF0501.JPG',
  episode_title: null,
  camera: 'FUJIFILM X-T30',
  // Just after midnight on July 1 on the camera; the moment is June 30 in London.
  taken_local: '2024-07-01T00:20:00',
  taken_offset: null,
  taken_at: 1_719_789_600,
  added_at: 1_790_000_000,
  duration_s: null,
  size: 1,
} as unknown as Item;

describe('photo and home video labels', () => {
  it("name the day on the camera's clock wherever the viewer is", () => {
    const labels = sameInEveryZone(() => [displayTitle(photo), takenOrAddedDate(photo)]);
    if (english) expect(labels).toEqual(['Photo from Jul 1, 2024', 'Jul 1, 2024']);
    expect(hasFileNameTitle(photo)).toBe(true);
    // A camera-named photo already shows its date as the title, so its line is the camera.
    expect(itemMeta(photo)).toBe('FUJIFILM X-T30');
  });

  it('use the day it was added when there is no capture time', () => {
    const undated = {...photo, taken_local: null, taken_at: null, taken_offset: null} as Item;
    expect(hasFileNameTitle(undated)).toBe(false);
    expect(displayTitle(undated)).toBe('DSCF0501');
    expect(takenOrAddedDate(undated)).not.toBe('');
  });

  it("put a home video's camera day and length on its tile", () => {
    const clip = {...photo, kind: 'video', category: 'home_video', title: 'IMG_4410', duration_s: 8, taken_local: '2024-06-13T17:42:05'} as Item;
    const meta = sameInEveryZone(() => itemMeta(clip));
    if (english) expect(meta).toBe('Jun 13, 2024 · 8 s');
  });
});

describe("a show's tile on Home", () => {
  const latest = {season: 2, episode: 3} as Item;
  const show = (item_count: number, new_count: number): Pick<HomeShow, 'item_count' | 'new_count' | 'latest'> => ({
    item_count,
    new_count,
    latest,
  });

  it('names the one new episode, counts several, or counts them all when none is new', () => {
    expect(showTileMeta(show(7, 1))).toBe('New: S02E03');
    expect(showTileMeta(show(7, 6))).toBe('6 new episodes');
    expect(showTileMeta(show(5, 0))).toBe('5 episodes');
    expect(showTileMeta(show(1, 0))).toBe('1 episode');
  });
});
