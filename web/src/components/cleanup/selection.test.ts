import {describe, expect, it} from 'vitest';
import type {CleanupFile} from '../../api/types';
import {groupedFiles, matchingFiles, selectNonVideos} from './selection';
const file = (path: string, kind: CleanupFile['kind'], review: CleanupFile['review'] = 'none'): CleanupFile => ({path, kind, review, size: 10, fingerprint: 'snapshot', source_download_id: null});
const files = [file('Inbox/Film/film.mkv', 'video'), file('Inbox/Film/film.en.srt', 'subtitle'), file('Inbox/Film/cover.JPG', 'image', 'ignored'), file('Other/readme.txt', 'other', 'waiting')];
describe('explicit file cleanup selection', () => {
  it('keeps all videos while selecting subtitles, artwork and instructions', () => {
    expect([...selectNonVideos(files)]).toEqual(files.slice(1).map(f => f.path));
  });
  it('selects within the current type, review and path filters', () => {
    expect(matchingFiles(files, 'image', 'ignored', 'film/')).toEqual([files[2]]);
    expect(matchingFiles(files, 'all', 'waiting', '')).toEqual([files[3]]);
    expect(matchingFiles(files, 'video', 'ignored', '')).toEqual([]);
  });
  it('groups nested files by their exact parent, retaining paths for removal', () => {
    const groups = groupedFiles(files);
    expect(groups.map(([folder, rows]) => [folder, rows.length])).toEqual([['Inbox/Film', 3], ['Other', 1]]);
    expect(groups[0]?.[1][1]?.path).toBe('Inbox/Film/film.en.srt');
  });
});
