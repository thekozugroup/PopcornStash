import {describe, expect, it} from 'vitest';
import type {ItemDetail, Library} from '../../api/types';
import {sameInEveryZone} from '../../lib/timeZones.testing';
import {draftFromItem, hintBeside, isClaimOnly, monthFolders, previewMove, targetChoices, type Target} from './places';

const shows = {
  id: 'lib-shows',
  name: 'Shows',
  icon: 'tv',
  content_type: 'video',
  structure: 'episodes',
  claims: ['episode'],
  layout: 'series',
  folder: 'Shows',
} as unknown as Library;

const kids = {...shows, id: 'lib-kids', name: 'Kids', icon: 'baby', claims: [], folder: 'Kids'} as Library;

const photos = {
  ...shows,
  id: 'lib-photos',
  name: 'Photos',
  icon: 'image',
  content_type: 'photos',
  structure: null,
  claims: ['photo'],
  layout: 'timeline',
  folder: 'Photos',
} as Library;

const episode = {
  id: 'i-1',
  kind: 'video',
  rel_path: 'Inbox/Puddle.Club.mkv',
  category: 'episode',
  title: null,
  year: null,
  show_name: 'Puddle Club',
  season: 1,
  episode: null,
  taken_at: null,
  taken_local: null,
  taken_offset: null,
  suggestion: null,
} as unknown as ItemDetail;

const toShows: Target = {place: 'library', library: shows};

describe('the move preview', () => {
  it('says what is missing until the field itself says so', () => {
    const preview = previewMove(toShows, draftFromItem(episode), episode, true);
    expect(preview.hint).toBe('Add the episode number to see where it goes.');
    expect(hintBeside(preview, {})).toBe(preview.hint);
    // Saving shows "Enter the episode number." under the field: the preview does not repeat it.
    expect(hintBeside(preview, {episode: 'Enter the episode number.'})).toBeNull();
    // Another field's error leaves the hint.
    expect(hintBeside(preview, {season: 'Enter the season as a number.'})).toBe(preview.hint);
  });

  it('names the folder and the new file name once the details are there', () => {
    const preview = previewMove(toShows, {...draftFromItem(episode), episode: 3}, episode, true);
    expect(preview).toMatchObject({
      title: 'Will move to Shows',
      folder: 'Shows/Puddle Club/Season 01/',
      fileName: 'Puddle Club - S01E03.mkv',
      hint: null,
    });
    expect(previewMove(toShows, {...draftFromItem(episode), episode: 3}, episode, false).folder).toBeNull();
  });
});

describe('where a waiting file may go', () => {
  const claimed = {
    ...episode,
    suggestion: {place: 'library', library_id: 'lib-shows', category: 'episode'},
  } as unknown as ItemDetail;

  it('offers a member only the claiming library for a file someone else may have added', () => {
    expect(isClaimOnly(claimed, false)).toBe(true);
    expect(targetChoices(claimed, [shows, kids, photos], false).map(target => target.library?.name)).toEqual(['Shows']);
  });

  it('offers owners every library that fits (a photos library takes clips too), then Other files', () => {
    expect(isClaimOnly(claimed, true)).toBe(false);
    expect(targetChoices(claimed, [shows, kids, photos], true).map(target => target.library?.name ?? 'Other files')).toEqual([
      'Shows',
      'Kids',
      'Photos',
      'Other files',
    ]);
  });
});

describe("a photo's month folder", () => {
  // A camera clock just after midnight on July 1 with no offset. Its moment is June 30 in
  // London (the server) and in Los Angeles, but it is filed by the clock, under 2024/07.
  const photo = {
    id: 'i-2',
    kind: 'photo',
    rel_path: 'Inbox/DSCF0501.JPG',
    category: 'photo',
    title: 'DSCF0501',
    year: null,
    show_name: null,
    season: null,
    episode: null,
    taken_at: 1_719_789_600,
    taken_local: '2024-06-30T23:20:00',
    taken_offset: null,
    suggestion: {category: 'photo', taken_at: 1_719_789_600, taken_local: '2024-07-01T00:20:00', taken_offset: null},
  } as unknown as ItemDetail;
  const toPhotos: Target = {place: 'library', library: photos};

  it("is the year and month on the suggestion's clock, in every time zone", () => {
    const folder = sameInEveryZone(() => previewMove(toPhotos, draftFromItem(photo), photo, true).folder);
    expect(folder).toBe('Photos/2024/07/');
  });

  it("falls back to the item's own clock, and says how it is filed when there is none", () => {
    const own = {...photo, suggestion: null} as unknown as ItemDetail;
    expect(sameInEveryZone(() => previewMove(toPhotos, draftFromItem(own), own, true).folder)).toBe('Photos/2024/06/');
    const undated = {...own, taken_at: null, taken_local: null} as unknown as ItemDetail;
    expect(previewMove(toPhotos, draftFromItem(undated), undated, true)).toMatchObject({
      folder: null,
      hint: 'Filed by the month it was taken.',
    });
  });

  it('reads the month straight from the text', () => {
    expect(monthFolders('2023-12-31T23:59:59')).toEqual(['2023', '12']);
    expect(monthFolders(null)).toBeNull();
    expect(monthFolders('Undated')).toBeNull();
  });
});
