import {describe, expect, it} from 'vitest';
import type {TorrentFile} from '../../api/types';
import {defaultFileKinds, groupValue, selectGroup} from './fileSelection';
const files: TorrentFile[] = [
  {index: 0, path: 'Series/S01/E01.mkv', kind: 'video', size: 100, selected: true},
  {index: 1, path: 'Series/S01/E02.mkv', kind: 'video', size: 100, selected: false},
  {index: 2, path: 'Series/Subs/E01.srt', kind: 'subtitle', size: 10, selected: true},
  {index: 3, path: 'Series/readme.txt', kind: 'other', size: 10, selected: false},
];
describe('torrent file choices', () => {
  it('defaults to videos and optional subtitles', () => expect(defaultFileKinds).toEqual(['video', 'subtitle']));
  it('shows partially selected types and only changes that type', () => {
    const selected = new Set([0, 2]);
    expect(groupValue(files, selected, 'video')).toBe('indeterminate');
    expect([...selectGroup(files, selected, 'video', true)]).toEqual([0, 2, 1]);
    expect([...selectGroup(files, selected, 'subtitle', false)]).toEqual([0]);
    expect([...selected]).toEqual([0, 2]);
  });
  it('does not select instruction files when videos are selected', () => {
    expect(selectGroup(files, new Set(), 'video', true).has(3)).toBe(false);
    expect(groupValue(files, new Set(), 'image')).toBe(false);
  });
});
