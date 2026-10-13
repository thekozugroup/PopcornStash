/**
 * Primary destinations, grouped as in DESIGN.md:
 * Home, Movies, Shows, Home Videos, Photos, Other · Inbox, Downloads · Settings.
 *
 * Each destination is defined once with its icon, so the sidebar, the phone
 * menu and the quick links always draw the same glyph for the same place. The
 * category icons are the ones the library already uses for that category's
 * tile placeholder and empty state; the Android toolbar uses the same set.
 */
import {
  ArrowDownToLine,
  Clapperboard,
  FileText,
  Film,
  House,
  ImageIcon,
  Inbox,
  Settings,
  Tv,
  type LucideIcon,
} from 'lucide-react';

export interface NavDestination {
  path: string;
  label: string;
  /** Line icon drawn before the label (sidebar) or above it (quick links). */
  icon: LucideIcon;
  /** Match only the exact path (Home). */
  exact?: boolean;
}

export interface NavGroup {
  /** Screen-reader name for the group. */
  title: string;
  items: NavDestination[];
}

/** Navigation icons: 18px line glyphs at stroke 1.5, in the label's colour. */
export const NAV_ICON_SIZE = 18;
export const NAV_ICON_STROKE = 1.5;

const HOME: NavDestination = {path: '/', label: 'Home', icon: House, exact: true};
const MOVIES: NavDestination = {path: '/movies', label: 'Movies', icon: Film};
const SHOWS: NavDestination = {path: '/shows', label: 'Shows', icon: Tv};
const HOME_VIDEOS: NavDestination = {path: '/home-videos', label: 'Home Videos', icon: Clapperboard};
const PHOTOS: NavDestination = {path: '/photos', label: 'Photos', icon: ImageIcon};
const OTHER: NavDestination = {path: '/other', label: 'Other', icon: FileText};
const INBOX: NavDestination = {path: '/inbox', label: 'Inbox', icon: Inbox};
const DOWNLOADS: NavDestination = {path: '/downloads', label: 'Downloads', icon: ArrowDownToLine};
const SETTINGS: NavDestination = {path: '/settings', label: 'Settings', icon: Settings};

export const NAV_GROUPS: NavGroup[] = [
  {title: 'Library', items: [HOME, MOVIES, SHOWS, HOME_VIDEOS, PHOTOS, OTHER]},
  {title: 'Incoming', items: [INBOX, DOWNLOADS]},
  {title: 'Manage', items: [SETTINGS]},
];

/** The five destinations kept one tap away on narrow screens. */
export const QUICK_DESTINATIONS: NavDestination[] = [HOME, MOVIES, SHOWS, PHOTOS, DOWNLOADS];

export function isActive(destination: NavDestination, pathname: string): boolean {
  if (destination.exact) return pathname === destination.path;
  return pathname === destination.path || pathname.startsWith(`${destination.path}/`);
}
