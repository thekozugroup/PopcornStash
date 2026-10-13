/**
 * Library places in plain words, shared by Inbox and Downloads.
 *
 * Folder names follow docs/architecture.md section 3. The destination preview
 * mirrors the sorter's rules (section 4) so people see where a file goes
 * before it moves; the server's answer is still the final word.
 */
import type {Classification, ItemCategory, ItemDetail} from '../../api/types';
import {formatEpisodeCode} from '../../lib/format';

export const CATEGORY_FOLDER: Record<ItemCategory, string> = {
  movie: 'Movies',
  episode: 'Shows',
  home_video: 'Home Videos',
  photo: 'Photos',
  other: 'Other',
};

export const CATEGORY_LABEL: Record<ItemCategory, string> = {
  movie: 'Movie',
  episode: 'Show episode',
  home_video: 'Home video',
  photo: 'Photo',
  other: 'Other',
};

/** Choices for "What is this file?". Where each one goes is the live preview's job. */
export const CATEGORY_OPTIONS: {value: ItemCategory; label: string}[] = [
  {value: 'movie', label: 'Movie'},
  {value: 'episode', label: 'Show episode'},
  {value: 'home_video', label: 'Home video'},
  {value: 'photo', label: 'Photo'},
  {value: 'other', label: 'Other'},
];

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

/** "Movies/Heat (1995)/Heat (1995).mkv" -> "Movies / Heat (1995)". */
export function describeDestination(destination: string): string {
  const parts = destination.split('/').filter(Boolean);
  const folders = parts.length > 1 ? parts.slice(0, -1) : parts;
  return folders.join(' / ');
}

/** Same characters the sorter replaces in folder names, so the preview matches. */
function cleanName(value: string): string {
  return value
    .replace(/[\\/:*?"<>|]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}

function monthPath(takenAt: number | null | undefined): string | null {
  if (takenAt === null || takenAt === undefined) return null;
  const date = new Date(takenAt * 1000);
  return `${date.getFullYear()} / ${String(date.getMonth() + 1).padStart(2, '0')}`;
}

export interface DestinationDraft {
  category: ItemCategory;
  title: string;
  year: number | null;
  showName: string;
  season: number | null;
  episode: number | null;
}

export interface DestinationPreview {
  /** "Movies / Heat (1995)" */
  place: string;
  /** The new file name when the sorter renames it, else null. */
  fileName: string | null;
  /** One line under the destination: what is still missing, or that the name is kept. */
  hint: string | null;
}

/** Live preview of where an edited inbox item will go. */
export function previewDestination(draft: DestinationDraft, item: ItemDetail): DestinationPreview {
  const extension = (() => {
    const name = baseName(item.rel_path);
    const dot = name.lastIndexOf('.');
    return dot > 0 ? name.slice(dot) : '';
  })();
  const takenAt = item.suggestion?.taken_at ?? item.taken_at;

  switch (draft.category) {
    case 'movie': {
      const title = cleanName(draft.title);
      if (!title) return {place: 'Movies', fileName: null, hint: 'Add a title to see where it goes.'};
      const folder = draft.year ? `${title} (${draft.year})` : title;
      return {place: `Movies / ${folder}`, fileName: `${folder}${extension}`, hint: null};
    }
    case 'episode': {
      const show = cleanName(draft.showName);
      if (!show) return {place: 'Shows', fileName: null, hint: 'Add the show name to see where it goes.'};
      if (draft.season === null || draft.episode === null) {
        return {place: `Shows / ${show}`, fileName: null, hint: 'Add the season and episode numbers.'};
      }
      const season = `Season ${String(draft.season).padStart(2, '0')}`;
      return {
        place: `Shows / ${show} / ${season}`,
        fileName: `${show} - ${formatEpisodeCode(draft.season, draft.episode)}${extension}`,
        hint: null,
      };
    }
    case 'home_video': {
      const month = monthPath(takenAt);
      return {
        place: month ? `Home Videos / ${month}` : 'Home Videos, by the month it was recorded',
        fileName: null,
        hint: null,
      };
    }
    case 'photo': {
      const month = monthPath(takenAt);
      return {
        place: month ? `Photos / ${month}` : 'Photos, by the month it was taken',
        fileName: null,
        hint: null,
      };
    }
    case 'other':
    default:
      return {place: 'Other', fileName: null, hint: 'It keeps its file name.'};
  }
}

/** Starting values for the edit form, from the suggestion or the item itself. */
export function draftFromItem(item: ItemDetail): DestinationDraft {
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

/** A readable title for a library item: "Heat (1995)", "The Bear S01E02". */
export function itemTitle(item: Pick<ItemDetail, 'category' | 'title' | 'year' | 'show_name' | 'season' | 'episode'>): string {
  if (item.category === 'episode' && item.show_name) {
    const code = formatEpisodeCode(item.season, item.episode);
    return code ? `${item.show_name} ${code}` : item.show_name;
  }
  if (item.category === 'movie' && item.year) return `${item.title} (${item.year})`;
  return item.title;
}
