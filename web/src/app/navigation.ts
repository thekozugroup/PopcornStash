/**
 * Primary destinations, grouped as in DESIGN.md:
 * Home, Movies, Shows, Home Videos, Photos, Other · Inbox, Downloads · Settings.
 */
export interface NavDestination {
  path: string;
  label: string;
  /** Match only the exact path (Home). */
  exact?: boolean;
}

export interface NavGroup {
  /** Screen-reader name for the group. */
  title: string;
  items: NavDestination[];
}

export const NAV_GROUPS: NavGroup[] = [
  {
    title: 'Library',
    items: [
      {path: '/', label: 'Home', exact: true},
      {path: '/movies', label: 'Movies'},
      {path: '/shows', label: 'Shows'},
      {path: '/home-videos', label: 'Home Videos'},
      {path: '/photos', label: 'Photos'},
      {path: '/other', label: 'Other'},
    ],
  },
  {
    title: 'Incoming',
    items: [
      {path: '/inbox', label: 'Inbox'},
      {path: '/downloads', label: 'Downloads'},
    ],
  },
  {
    title: 'Manage',
    items: [{path: '/settings', label: 'Settings'}],
  },
];

/** The five destinations kept one tap away on narrow screens. */
export const QUICK_DESTINATIONS: NavDestination[] = [
  {path: '/', label: 'Home', exact: true},
  {path: '/movies', label: 'Movies'},
  {path: '/shows', label: 'Shows'},
  {path: '/photos', label: 'Photos'},
  {path: '/downloads', label: 'Downloads'},
];

export function isActive(destination: NavDestination, pathname: string): boolean {
  if (destination.exact) return pathname === destination.path;
  return pathname === destination.path || pathname.startsWith(`${destination.path}/`);
}
