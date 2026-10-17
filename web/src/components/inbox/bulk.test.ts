import {describe, expect, it} from 'vitest';
import type {ItemDetail} from '../../api/types';
import {bulkReady, inboxFileType, matchingInbox} from './bulk';
const file = (id: string, kind: ItemDetail['kind'], path: string, download: string | null): ItemDetail => ({id, kind, rel_path: path, source_download_id: download, suggestion: null} as ItemDetail);
describe('Inbox bulk selection', () => {
  const video = file('v', 'video', 'Inbox/film.mkv', 'a');
  const subtitle = file('s', 'other', 'Inbox/film.SRT', 'a');
  const photo = file('p', 'photo', 'Inbox/photo.jpg', 'b');
  it('combines a download and file type without including neighbouring files', () => {
    expect(matchingInbox([video, subtitle, photo], 'a', 'video')).toEqual([video]);
    expect(matchingInbox([video, subtitle, photo], 'a', 'all')).toEqual([video, subtitle]);
    expect(inboxFileType(subtitle)).toBe('subtitle');
  });
  it('distinguishes loose uploads and does not import without a destination', () => {
    const loose = file('l', 'video', 'Inbox/loose.mp4', null);
    expect(matchingInbox([video, loose], 'none', 'all')).toEqual([loose]);
    expect(bulkReady([video, loose], [], true, () => null)).toEqual([]);
    expect(bulkReady([video], [], true, () => 'other_files')).toEqual([{item: video, value: 'other_files'}]);
  });
});
