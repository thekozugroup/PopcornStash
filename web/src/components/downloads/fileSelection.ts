import type {TorrentFileKind, TorrentFile} from '../../api/types';

export const fileKinds: {value: TorrentFileKind; label: string}[] = [
  {value: 'video', label: 'Videos'}, {value: 'subtitle', label: 'Subtitles'},
  {value: 'image', label: 'Images'}, {value: 'other', label: 'Other files'},
];
export const defaultFileKinds: TorrentFileKind[] = ['video', 'subtitle'];

export function groupValue(files: TorrentFile[], selected: ReadonlySet<number>, kind: TorrentFileKind): boolean | 'indeterminate' {
  const group = files.filter(file => file.kind === kind);
  const count = group.filter(file => selected.has(file.index)).length;
  return count === 0 ? false : count === group.length ? true : 'indeterminate';
}

export function selectGroup(files: TorrentFile[], selected: ReadonlySet<number>, kind: TorrentFileKind, value: boolean): Set<number> {
  const next = new Set(selected);
  for (const file of files) if (file.kind === kind) {
    if (value) next.add(file.index); else next.delete(file.index);
  }
  return next;
}
