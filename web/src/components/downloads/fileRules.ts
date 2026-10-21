import type {TorrentFile, TorrentFileKind, TorrentFileRules} from '../../api/types';

export interface FileRuleDraft {enabled: boolean; include: string; exclude: string; minimum: string; maximum: string; unit: 'MB' | 'GB'}
export const emptyFileRules: FileRuleDraft = {enabled: false, include: '', exclude: '', minimum: '', maximum: '', unit: 'MB'};
const normalize = (value: string) => value.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter(Boolean).join(' ');
const names = (value: string) => value.split(',').map(s => s.trim()).filter(Boolean);

export function parseFileRules(draft: FileRuleDraft, fileTypes: TorrentFileKind[]): {rules?: TorrentFileRules; error?: string} {
  if (!draft.enabled) return {};
  const include = names(draft.include), exclude = names(draft.exclude);
  if (!fileTypes.length) return {error: 'Choose at least one file type for the rules.'};
  if ([include, exclude].some(list => list.length > 32 || list.some(s => !normalize(s) || new TextEncoder().encode(s).length > 128 || /\p{Cc}/u.test(s)))) {
    return {error: 'Use up to 32 names per field, each up to 128 bytes and containing letters or numbers.'};
  }
  const factor = draft.unit === 'GB' ? 1_000_000_000 : 1_000_000;
  const size = (text: string) => text.trim() === '' ? undefined : /^(?:\d+(?:\.\d*)?|\.\d+)$/.test(text.trim()) ? Math.round(Number(text) * factor) : NaN;
  const min = size(draft.minimum), max = size(draft.maximum);
  if ([min,max].some(n => n !== undefined && (!Number.isSafeInteger(n) || n < 0))) return {error: 'Enter file sizes as positive numbers, or leave them blank.'};
  if (min !== undefined && max !== undefined && min > max) return {error: 'Minimum size must be no greater than maximum size.'};
  return {rules: {file_types: fileTypes, name_contains: include, name_excludes: exclude, min_size: min, max_size: max}};
}

export function draftFromRules(rules?: TorrentFileRules | null): FileRuleDraft {
  return rules ? {enabled: true, include: (rules.name_contains ?? []).join(', '), exclude: (rules.name_excludes ?? []).join(', '), minimum: rules.min_size == null ? '' : String(rules.min_size / 1_000_000), maximum: rules.max_size == null ? '' : String(rules.max_size / 1_000_000), unit: 'MB'} : {...emptyFileRules};
}

export function matchesFileRules(file: TorrentFile, rules: TorrentFileRules): boolean {
  const path = normalize(file.path);
  return (!rules.file_types || rules.file_types.includes(file.kind))
    && (rules.min_size == null || file.size >= rules.min_size) && (rules.max_size == null || file.size <= rules.max_size)
    && (!rules.name_contains?.length || rules.name_contains.some(s => path.includes(normalize(s))))
    && !rules.name_excludes?.some(s => path.includes(normalize(s)));
}

/** Opening preserves server choices. Only an explicit filter edit calls this. */
export function selectionFromFilters(files: TorrentFile[], draft: FileRuleDraft, types: TorrentFileKind[]): {selected?: Set<number>; error?: string} {
  if (types.length === 0) return {selected: new Set(), error: 'Choose at least one file type.'};
  const parsed = parseFileRules({...draft, enabled: true}, types);
  const rules = parsed.rules;
  if (parsed.error || !rules) return {error: parsed.error};
  return {selected: new Set(files.filter(file => matchesFileRules(file, rules)).map(file => file.index))};
}
