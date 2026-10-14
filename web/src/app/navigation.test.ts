import {describe, expect, it} from 'vitest';
import type {Library} from '../api/types';
import {navPath} from './navPlace';
import {DOWNLOADS, HOME, INBOX, OTHER_FILES, isActive, itemNavPath, libraryDestination, navGroups} from './navigation';

const shows = {id: 'lib-shows', name: 'Shows', icon: 'tv'} as Library;
const movies = {id: 'lib-movies', name: 'Movies', icon: 'film'} as Library;
const libraries = [movies, shows];

/** The destinations marked current for an address, with the place a page names for itself. */
function current(pathname: string, path: string | null = null): string[] {
  const place = path ? {pathname, path} : null;
  return navGroups(libraries)
    .flatMap(group => group.items)
    .filter(destination => isActive(destination, navPath(pathname, place)))
    .map(destination => destination.label);
}

describe('the current destination', () => {
  it("is the item's library on its page", () => {
    const path = itemNavPath({place: 'library', library_id: 'lib-shows'}, libraries);
    expect(path).toBe('/libraries/lib-shows');
    expect(current('/item/ep-1', path)).toEqual(['Shows']);
  });

  it("is the show's library on the show's page", () => {
    expect(current('/libraries/lib-shows/shows/The%20Lantern%20Keepers')).toEqual(['Shows']);
    expect(isActive(libraryDestination(shows), '/libraries/lib-shows-2')).toBe(false);
  });

  it('is Other files, or Inbox for a file waiting there', () => {
    expect(current('/item/doc-1', itemNavPath({place: 'other_files', library_id: null}, libraries))).toEqual(['Other files']);
    expect(current('/item/new-1', itemNavPath({place: 'incoming', library_id: null}, libraries))).toEqual(['Inbox']);
  });

  it('is nothing for a file outside every place the sidebar lists', () => {
    expect(itemNavPath({place: 'folder', library_id: null}, libraries)).toBeNull();
    expect(itemNavPath({place: 'trash', library_id: 'lib-shows'}, libraries)).toBeNull();
    // A library the person no longer sees.
    expect(itemNavPath({place: 'library', library_id: 'lib-hidden'}, libraries)).toBeNull();
    expect(current('/item/x')).toEqual([]);
  });

  it("follows the address again once the item's page has gone", () => {
    // The place was named on another address: a stale place never sticks to the next page.
    expect(current('/downloads', null)).toEqual([DOWNLOADS.label]);
    expect(isActive(HOME, navPath('/', {pathname: '/item/ep-1', path: '/libraries/lib-shows'}))).toBe(true);
  });

  it('keeps Home exact and the fixed places whole', () => {
    expect(current('/')).toEqual(['Home']);
    expect(isActive(OTHER_FILES, '/other')).toBe(true);
    expect(isActive(INBOX, '/inboxes')).toBe(false);
  });
});
