import {describe, expect, it} from 'vitest';
import {draftFromRules, emptyFileRules, matchesFileRules, parseFileRules} from './fileRules';
import type {TorrentFile} from '../../api/types';
const file = (path: string, size: number, kind: TorrentFile['kind'] = 'video'): TorrentFile => ({index: 0, path, size, kind, selected: false});
describe('torrent file rules', () => {
  it('combines types, alternative names, exclusions and inclusive sizes', () => {
    const {rules, error} = parseFileRules({...emptyFileRules, enabled: true, include: 'Jane Doe, Álex Smith', exclude: 'trailer', minimum: '.1', maximum: '.2'}, ['video']);
    expect(error).toBeUndefined();
    expect(matchesFileRules(file('CAST/JANE.DOE/film.mkv', 100000), rules!)).toBe(true);
    expect(matchesFileRules(file('Álex_Smith/film.mp4', 200000), rules!)).toBe(true);
    for (const f of [file('Jane Doe/trailer.mkv', 150000), file('Other/film.mkv', 150000), file('Jane Doe/pic.jpg', 150000, 'image'), file('Jane Doe/film.mkv', 99999), file('Jane Doe/film.mkv', 200001)]) expect(matchesFileRules(f, rules!)).toBe(false);
    expect(parseFileRules(draftFromRules(rules), ['video']).rules).toEqual(rules);
  });
  it('rejects invalid rules instead of accidentally selecting files', () => {
    for (const draft of [{minimum: '-1'}, {minimum: 'NaN'}, {minimum: '1e5'}, {minimum: '2', maximum: '1'}, {include: '...'}, {include: 'a\nb'}, {maximum: '99999999999999999999'}]) expect(parseFileRules({...emptyFileRules, ...draft, enabled: true}, ['video']).error).toBeTruthy();
    expect(parseFileRules({...emptyFileRules, enabled: true}, []).error).toBeTruthy();
    expect(parseFileRules({...emptyFileRules, include: 'Jane'}, [])).toEqual({});
  });
  it('uses decimal GB and permits zero matches', () => {
    const {rules} = parseFileRules({...emptyFileRules, enabled: true, minimum: '1.25', unit: 'GB'}, ['video']);
    expect(rules?.min_size).toBe(1250000000);
    expect(matchesFileRules(file('film.mp4', 10), rules!)).toBe(false);
  });
});

describe('live torrent selection', () => {
  it('updates exact manifest indices without an enable or apply step', async () => {
    const {selectionFromFilters} = await import('./fileRules');
    const files = [
      {...file('Film/feature.mkv', 8_000_000_000), index: 2},
      {...file('Film/sample.mp4', 50_000_000), index: 4},
      {...file('Film/English.srt', 54_000, 'subtitle'), index: 5},
    ];
    expect(selectionFromFilters(files, {...emptyFileRules, minimum: '1', unit: 'GB'}, ['video', 'subtitle']).selected).toEqual(new Set([2]));
    expect(selectionFromFilters(files, {...emptyFileRules, minimum: '1', maximum: '.5', unit: 'GB'}, ['video']).selected).toBeUndefined();
    expect(selectionFromFilters(files, emptyFileRules, [])).toEqual({selected: new Set(), error: 'Choose at least one file type.'});
    expect(selectionFromFilters(files, {...emptyFileRules, include: 'missing'}, ['video']).selected).toEqual(new Set());
    expect(selectionFromFilters(files, emptyFileRules, ['subtitle']).selected).toEqual(new Set([5]));
  });
});
