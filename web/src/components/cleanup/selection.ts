import type {CleanupFile, CleanupKind} from '../../api/types';

export const cleanupKinds: {value: CleanupKind; label: string}[] = [
  {value: 'video', label: 'Videos'}, {value: 'image', label: 'Photos'},
  {value: 'subtitle', label: 'Subtitles'}, {value: 'audio', label: 'Audio'}, {value: 'other', label: 'Other files'},
];
export function matchingFiles(files: CleanupFile[], kind: string, review: string, search: string): CleanupFile[] {
  const query = search.trim().toLocaleLowerCase();
  return files.filter(f => (kind === 'all' || f.kind === kind) && (review === 'all' || f.review === review) && f.path.toLocaleLowerCase().includes(query));
}
export function selectNonVideos(files: CleanupFile[]): Set<string> {
  return new Set(files.filter(f => f.kind !== 'video').map(f => f.path));
}
export function groupedFiles(files: CleanupFile[]): [string, CleanupFile[]][] {
  const groups = new Map<string, CleanupFile[]>();
  for (const file of files) {
    const folder = file.path.split('/').slice(0, -1).join('/') || 'Stash root';
    const rows = groups.get(folder) ?? []; rows.push(file); groups.set(folder, rows);
  }
  return [...groups];
}
