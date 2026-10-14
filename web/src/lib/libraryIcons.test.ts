import {Folder} from 'lucide-react';
import {describe, expect, it} from 'vitest';
import {KNOWN_LIBRARY_ICONS, libraryIcon, libraryIconLabel} from './libraryIcons';

describe('library icons', () => {
  it('never offers the folder glyph, which marks folders that are not a library', () => {
    expect(KNOWN_LIBRARY_ICONS).not.toContain('folder');
    expect(KNOWN_LIBRARY_ICONS).toHaveLength(31);
  });

  it('draws a folder for a name this build does not know', () => {
    expect(libraryIcon('rocket-from-a-newer-server')).toBe(Folder);
    expect(libraryIcon(null)).toBe(Folder);
    expect(libraryIconLabel('rocket-ship')).toBe('Rocket ship');
  });

  it('uses the plain names the Android app uses too', () => {
    // android/.../ui/icons/LibraryIconsTest compares the whole table with LibraryIcons.kt.
    expect(['tv', 'mountain-snow', 'book-open', 'bike'].map(libraryIconLabel)).toEqual(['TV', 'Mountain', 'Book', 'Bike']);
  });
});
