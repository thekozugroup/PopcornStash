/**
 * Primary destinations, grouped as in DESIGN.md: Home, every library the
 * person can see (in the owner's order, each with its own icon), Other files ·
 * Inbox, Downloads · Settings.
 *
 * The fixed places are defined once here with their fixed glyphs; a library
 * draws the icon its owner chose through `libraryIcon()`. So the sidebar, the
 * phone menu and the quick links always draw the same glyph for the same
 * place, and no library can look like Home or Inbox (their glyphs are not in
 * the curated list).
 */
import {ArrowDownToLine, FileText, House, Inbox, Settings, type LucideIcon} from 'lucide-react';
import type {Item, Library} from '../api/types';
import {libraryHref} from '../lib/libraries';
import {libraryIcon} from '../lib/libraryIcons';

export interface NavDestination {
  path: string;
  label: string;
  /** Line icon drawn before the label (sidebar) or above it (quick links). */
  icon: LucideIcon;
  /** Match only the exact path (Home). */
  exact?: boolean;
  /** Set for a library: its id. */
  libraryId?: string;
}

export interface NavGroup {
  /** Screen-reader name for the group. */
  title: string;
  items: NavDestination[];
}

/** Navigation icons: 18px line glyphs at stroke 1.5, in the label's colour. */
export const NAV_ICON_SIZE = 18;
export const NAV_ICON_STROKE = 1.5;

export const HOME: NavDestination = {path: '/', label: 'Home', icon: House, exact: true};
export const OTHER_FILES: NavDestination = {path: '/other', label: 'Other files', icon: FileText};
export const INBOX: NavDestination = {path: '/inbox', label: 'Inbox', icon: Inbox};
export const DOWNLOADS: NavDestination = {path: '/downloads', label: 'Downloads', icon: ArrowDownToLine};
export const SETTINGS: NavDestination = {path: '/settings', label: 'Settings', icon: Settings};

/** How many libraries the phone quick links hold, after Home. */
const QUICK_LIBRARIES = 3;

export function libraryDestination(library: Pick<Library, 'id' | 'name' | 'icon'>): NavDestination {
  return {path: libraryHref(library.id), label: library.name, icon: libraryIcon(library.icon), libraryId: library.id};
}

export function navGroups(libraries: Library[]): NavGroup[] {
  return [
    {title: 'Library', items: [HOME, ...libraries.map(libraryDestination), OTHER_FILES]},
    {title: 'Incoming', items: [INBOX, DOWNLOADS]},
    {title: 'Manage', items: [SETTINGS]},
  ];
}

/** Kept one tap away on narrow screens: Home, the first three libraries, Downloads. */
export function quickDestinations(libraries: Library[]): NavDestination[] {
  return [HOME, ...libraries.slice(0, QUICK_LIBRARIES).map(libraryDestination), DOWNLOADS];
}

/**
 * Whether `destination` is current for `pathname`: the address itself or anything under
 * it (a show's page is inside its library). An item's page passes its place instead of
 * its own address (`navPath`, app/navPlace.ts), so its library stays marked.
 */
export function isActive(destination: NavDestination, pathname: string): boolean {
  if (destination.exact) return pathname === destination.path;
  return pathname === destination.path || pathname.startsWith(`${destination.path}/`);
}

/**
 * The destination an item belongs to, marked current on the item's page: its library when
 * the person sees it, Other files, or Inbox for a file waiting there. A file in a folder
 * that is not a library (or in the trash) belongs to none of them.
 */
export function itemNavPath(item: Pick<Item, 'library_id' | 'place'>, libraries: Pick<Library, 'id'>[]): string | null {
  if (item.place === 'library' && item.library_id && libraries.some(library => library.id === item.library_id)) {
    return libraryHref(item.library_id);
  }
  if (item.place === 'other_files') return OTHER_FILES.path;
  if (item.place === 'incoming') return INBOX.path;
  return null;
}
