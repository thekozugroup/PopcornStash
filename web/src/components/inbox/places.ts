/**
 * Where a file can go and how it will be laid out there, shared by Inbox
 * (accept, edit), Organize and Downloads.
 *
 * The destination is always what the person sees on screen: a library they
 * can see that fits the file, or Other files (docs/architecture.md section 8,
 * Inbox accept and Organize). The preview mirrors the sorter's layout
 * (section 4) so people see where a file goes before it moves; the server's
 * answer is still the final word.
 */
import type {ClaimKind, Classification, ItemCategory, ItemDetail, Library} from '../../api/types';
import {formatEpisodeCode} from '../../lib/format';
import {CLAIM_WORDS, categoryIn, fileFits} from '../../lib/libraries';

/** Last part of a relative path: "Downloads/Completed/Heat.1995.mkv" -> "Heat.1995.mkv". */
export function baseName(relPath: string): string {
  const parts = relPath.split('/').filter(Boolean);
  return parts[parts.length - 1] ?? relPath;
}

/** Folder of a relative path: "Downloads/Completed/Heat.1995.mkv" -> "Downloads/Completed". */
export function folderOf(relPath: string): string {
  const parts = relPath.split('/').filter(Boolean);
  return parts.slice(0, -1).join('/');
}

// ---------------------------------------------------------------------------
// Targets
// ---------------------------------------------------------------------------

/** A Selector value: a library (`lib:<id>`) or Other files. */
export type TargetValue = `lib:${string}` | 'other_files';

export const OTHER_FILES_TARGET: TargetValue = 'other_files';

export function libraryTarget(id: string): TargetValue {
  return `lib:${id}`;
}

export interface Target {
  place: 'library' | 'other_files';
  library: Library | null;
}

export function resolveTarget(value: TargetValue | null, libraries: Library[]): Target | null {
  if (value === null) return null;
  if (value === OTHER_FILES_TARGET) return {place: 'other_files', library: null};
  const library = libraries.find(candidate => `lib:${candidate.id}` === value);
  return library ? {place: 'library', library} : null;
}

/** The target the row shows first: the suggestion, when the person can see it. */
export function suggestedTarget(item: ItemDetail, libraries: Library[]): TargetValue | null {
  const suggestion = item.suggestion;
  if (!suggestion || suggestion.place === null) return null;
  if (suggestion.place === 'other_files') return OTHER_FILES_TARGET;
  const library = suggestion.library_id ? libraries.find(candidate => candidate.id === suggestion.library_id) : undefined;
  return library ? libraryTarget(library.id) : null;
}

/**
 * Whether the person may send the file only to the library that claims its
 * kind. The person who added a file and owners may choose any library they
 * see that fits, or Other files; anyone else sees a waiting file only because
 * a library they see takes its kind, and may accept it only there.
 *
 * The API does not say who added a file, so for a member the row decides: a
 * suggestion that names a library may be someone else's file, and offers that
 * library alone; with no claiming library (or for Other files) the member can
 * only be seeing it because they added it, and gets the whole choice.
 */
export function isClaimOnly(item: ItemDetail, isOwner: boolean): boolean {
  return !isOwner && item.suggestion?.place === 'library' && item.suggestion.library_id !== null;
}

/** The libraries (and Other files) a file can be sent to by this person, in the owner's order. */
export function targetChoices(item: ItemDetail, libraries: Library[], isOwner: boolean): Target[] {
  if (isClaimOnly(item, isOwner)) {
    const only = libraries.find(library => library.id === item.suggestion?.library_id);
    return only ? [{place: 'library', library: only}] : [];
  }
  return [
    ...libraries.filter(library => fileFits(item.kind, library)).map(library => ({place: 'library' as const, library})),
    {place: 'other_files' as const, library: null},
  ];
}

export function targetValue(target: Target): TargetValue {
  return target.library ? libraryTarget(target.library.id) : OTHER_FILES_TARGET;
}

export function targetName(target: Target): string {
  return target.library?.name ?? 'Other files';
}

// ---------------------------------------------------------------------------
// Why a file waits
// ---------------------------------------------------------------------------

const CLAIMABLE = new Set<ItemCategory>(['movie', 'episode', 'home_video', 'photo']);

/** The row's reason line from the server's `waiting`, or null when there is nothing to add. */
export function waitingReason(suggestion: Classification | null, isOwner: boolean): string | null {
  if (!suggestion) return 'PopcornStash could not tell what this file is.';
  switch (suggestion.waiting) {
    case 'choose_library':
      return isOwner && CLAIMABLE.has(suggestion.category)
        ? `No library takes ${CLAIM_WORDS[suggestion.category as ClaimKind]} yet.`
        : 'Choose a library for this file.';
    case 'folder_unavailable':
      return 'The library’s folder is not available.';
    case 'low_confidence':
      return suggestion.reason;
    default:
      return null;
  }
}

// ---------------------------------------------------------------------------
// The edit form's draft and the live preview
// ---------------------------------------------------------------------------

export interface MoveDraft {
  /** What the file is inside a one-offs library: a movie or a camera video. */
  category: ItemCategory;
  title: string;
  year: number | null;
  showName: string;
  season: number | null;
  episode: number | null;
}

/** Starting values for the form, from the suggestion or the item itself. */
export function draftFromItem(item: ItemDetail): MoveDraft {
  const source: Pick<Classification, 'category' | 'title' | 'year' | 'show_name' | 'season' | 'episode'> =
    item.suggestion ?? item;
  return {
    category: source.category,
    title: source.title ?? '',
    year: source.year ?? null,
    showName: source.show_name ?? '',
    season: source.season ?? null,
    episode: source.episode ?? null,
  };
}

/** The kind the file becomes at a target (photos, episode, movie or camera video, other). */
export function categoryAt(target: Target, draft: MoveDraft): ItemCategory {
  return target.library ? categoryIn(target.library, draft.category) : 'other';
}

/** Same characters the sorter replaces in folder names, so the preview matches. */
function cleanName(value: string): string {
  return value
    .replace(/[\\/:*?"<>|]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}

/**
 * The `YYYY/MM/` folder a photo or camera video is filed in: the year and month on the
 * camera's clock, as written ("2024-07-01T00:20:00" files under 2024/07 in every time zone).
 */
export function monthFolders(takenLocal: string | null | undefined): string[] | null {
  const match = takenLocal ? /^(\d{4})-(\d{2})-/.exec(takenLocal) : null;
  return match ? [match[1] as string, match[2] as string] : null;
}

export interface MovePreview {
  /** "Will move to Kids" */
  title: string;
  /** For owners: the folder it lands in ("Kids/Puddle Club/Season 01/"). */
  folder: string | null;
  /** The new file name when the sorter renames it, else null. */
  fileName: string | null;
  /** What is still missing, when something is. */
  hint: string | null;
  /** The form field `hint` asks for; while that field shows its own error, the hint is left out. */
  asksFor?: 'title' | 'show' | 'episode';
}

/** The preview's hint, unless the field it asks for already says so under it (said once). */
export function hintBeside(preview: MovePreview, errors: Partial<Record<string, string>>): string | null {
  if (!preview.hint) return null;
  return preview.asksFor && errors[preview.asksFor] ? null : preview.hint;
}

/** Live preview of where a file goes at a target with these details. */
export function previewMove(target: Target, draft: MoveDraft, item: ItemDetail, showFolder: boolean): MovePreview {
  const name = baseName(item.rel_path);
  const dot = name.lastIndexOf('.');
  const extension = dot > 0 ? name.slice(dot) : '';
  const title = `Will move to ${targetName(target)}`;
  const root = target.library ? target.library.folder : 'Other';
  const folderOfParts = (parts: string[]) => (showFolder && root ? `${[root, ...parts].join('/')}/` : null);
  const category = categoryAt(target, draft);

  switch (category) {
    case 'movie': {
      const clean = cleanName(draft.title);
      if (!clean) return {title, folder: null, fileName: null, hint: 'Add a title to see where it goes.', asksFor: 'title'};
      const folder = draft.year ? `${clean} (${draft.year})` : clean;
      return {title, folder: folderOfParts([folder]), fileName: `${folder}${extension}`, hint: null};
    }
    case 'episode': {
      const show = cleanName(draft.showName);
      if (!show) return {title, folder: null, fileName: null, hint: 'Add the show name to see where it goes.', asksFor: 'show'};
      if (draft.episode === null) {
        return {title, folder: null, fileName: null, hint: 'Add the episode number to see where it goes.', asksFor: 'episode'};
      }
      const season = draft.season ?? 1;
      return {
        title,
        folder: folderOfParts([show, `Season ${String(season).padStart(2, '0')}`]),
        fileName: `${show} - ${formatEpisodeCode(season, draft.episode)}${extension}`,
        hint: null,
      };
    }
    case 'home_video':
    case 'photo': {
      const month = monthFolders(item.suggestion?.taken_local ?? item.taken_local);
      return {
        title,
        folder: month ? folderOfParts(month) : null,
        fileName: null,
        hint: month ? null : category === 'photo' ? 'Filed by the month it was taken.' : 'Filed by the month it was recorded.',
      };
    }
    default:
      return {title, folder: folderOfParts([]), fileName: null, hint: null};
  }
}

/** A readable title for a library item: "Heat (1995)", "The Bear S01E02". */
export function itemTitle(item: Pick<ItemDetail, 'category' | 'title' | 'year' | 'show_name' | 'season' | 'episode'>): string {
  if (item.category === 'episode' && item.show_name) {
    const code = formatEpisodeCode(item.season, item.episode);
    return code ? `${item.show_name} ${code}` : item.show_name;
  }
  if (item.category === 'movie' && item.year) return `${item.title} (${item.year})`;
  return item.title;
}

/**
 * Whether a plain Accept to this target would be refused for missing details,
 * so the row opens the form instead: a series needs a show and an episode.
 */
export function needsDetails(target: Target, item: ItemDetail): boolean {
  if (target.library?.structure !== 'episodes') return false;
  const draft = draftFromItem(item);
  return !draft.showName.trim() || draft.episode === null;
}
