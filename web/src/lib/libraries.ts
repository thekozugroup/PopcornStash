/**
 * Libraries in plain words: their addresses, what each kind of new file is
 * called, which kinds fit which library, and the one-line count under a
 * library's name (docs/architecture.md section 8, Libraries; DESIGN.md,
 * Library pages and Library settings).
 */
import type {
  ClaimKind,
  ContentType,
  Id,
  ItemCategory,
  ItemKind,
  Library,
  LibraryVisibility,
  Structure,
  User,
} from '../api/types';
import type {MediaShape} from '../components/MediaTile';
import {formatCount} from './format';

export function libraryHref(id: string): string {
  return `/libraries/${encodeURIComponent(id)}`;
}

export function libraryShowHref(libraryId: string, showName: string): string {
  return `${libraryHref(libraryId)}/shows/${encodeURIComponent(showName)}`;
}

export const CLAIM_KINDS: ClaimKind[] = ['movie', 'episode', 'home_video', 'photo'];

/** The owner's words for each kind of new file, as a plural. */
export const CLAIM_WORDS: Record<ClaimKind, string> = {
  movie: 'movies',
  episode: 'TV episodes',
  home_video: 'camera videos',
  photo: 'photos',
};

/** Checkbox labels in the editor. */
export const CLAIM_LABELS: Record<ClaimKind, string> = {
  movie: 'Movies',
  episode: 'TV episodes',
  home_video: 'Camera videos',
  photo: 'Photos',
};

/** Whether a kind of new file fits a library of this type and structure. */
export function claimFits(kind: ClaimKind, contentType: ContentType, structure: Structure | null): boolean {
  if (kind === 'photo') return contentType === 'photos';
  if (contentType !== 'video') return false;
  return kind === 'episode' ? structure === 'episodes' : structure === 'one_offs';
}

/** The kinds that fit, in list order. */
export function fittingClaims(contentType: ContentType, structure: Structure | null): ClaimKind[] {
  return CLAIM_KINDS.filter(kind => claimFits(kind, contentType, structure));
}

/** "movies and camera videos", "TV episodes". */
export function joinWords(words: string[]): string {
  if (words.length <= 1) return words[0] ?? '';
  return `${words.slice(0, -1).join(', ')} and ${words[words.length - 1]}`;
}

/** "Takes TV episodes", "Takes nothing new". */
export function takesLine(claims: ClaimKind[]): string {
  return claims.length === 0 ? 'Takes nothing new' : `Takes ${joinWords(claims.map(kind => CLAIM_WORDS[kind]))}`;
}

/** "Videos, arranged as series": what a library holding files is, said as a fact (it cannot change). */
export function holdsLine(contentType: Library['content_type'], structure: Library['structure']): string {
  if (contentType === 'photos') return 'Photos';
  return structure === 'episodes' ? 'Videos, arranged as series' : 'Videos, arranged as one-offs';
}

export function kindLine(library: Pick<Library, 'content_type' | 'structure'>): string {
  if (library.content_type === 'photos') return 'Photos';
  return library.structure === 'episodes' ? 'Series' : 'One-offs';
}

/** The tile shape a library's layout draws. */
export function layoutShape(layout: Library['layout']): MediaShape {
  switch (layout) {
    case 'timeline':
      return 'square';
    case 'frames':
      return 'frame';
    default:
      return 'poster';
  }
}

/** The sentence under a library's name: "214 videos", "12 shows, 140 episodes", "3,214 photos". */
export function libraryCountLine(library: Pick<Library, 'layout' | 'item_count' | 'show_count'>): string {
  if (library.layout === 'series') {
    return `${formatCount(library.show_count ?? 0, 'show')}, ${formatCount(library.item_count, 'episode')}`;
  }
  if (library.layout === 'timeline') return formatCount(library.item_count, 'photo');
  return formatCount(library.item_count, 'video');
}

/** How an empty library fills, for its empty state. */
export function emptyLibraryLine(library: Pick<Library, 'name' | 'claims'>): string {
  if (library.claims.length === 0) return `Files you accept from Inbox into ${library.name} show up here.`;
  const words = joinWords(library.claims.map(kind => CLAIM_WORDS[kind]));
  return `New ${words} land here on their own.`;
}

/**
 * Whether a file of this kind can go into the library at all (section 8, Inbox
 * accept and Organize): photos and videos into a Photos library, videos into a
 * Video library. Songs and documents go to Other files only.
 */
export function fileFits(kind: ItemKind, library: Pick<Library, 'content_type'>): boolean {
  if (library.content_type === 'photos') return kind === 'photo' || kind === 'video';
  return kind === 'video';
}

/** "Everyone", "Only Ana and Sam", "Only owners" (the owner's list and editor). */
export function whoSeesLine(visibility: LibraryVisibility | null, memberIds: Id[] | null, people: User[]): string {
  if (visibility !== 'selected') return 'Everyone';
  const names = (memberIds ?? [])
    .map(id => people.find(person => person.id === id && person.role === 'member')?.display_name)
    .filter((name): name is string => Boolean(name));
  return names.length === 0 ? 'Only owners' : `Only ${joinWords(names)}`;
}

/** The category a file becomes in a library (what the server will make of it). */
export function categoryIn(
  library: Pick<Library, 'content_type' | 'structure' | 'claims'>,
  wanted: ItemCategory,
): ItemCategory {
  if (library.content_type === 'photos') return 'photo';
  if (library.structure === 'episodes') return 'episode';
  if (wanted === 'movie' || wanted === 'home_video') return wanted;
  return library.claims.includes('home_video') && !library.claims.includes('movie') ? 'home_video' : 'movie';
}
