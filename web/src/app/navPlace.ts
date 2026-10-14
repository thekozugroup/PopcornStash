/**
 * Which navigation destination a page belongs to when its address does not say so.
 *
 * Most pages live under their destination's address (`/libraries/:id/shows/:name`
 * is inside its library), so the sidebar and quick links work it out from the
 * path. An item's page is `/item/:id` wherever the item is; it names its place
 * with `useNavPlace(path)` once it knows it (its library, Other files or Inbox),
 * and the shell marks that destination current for as long as the page is open.
 */
import {createContext, useContext, useEffect} from 'react';
import {useLocation} from 'react-router';

/** A page's place: the address it was set on and the destination path it belongs to. */
export interface NavPlace {
  pathname: string;
  path: string;
}

export const NavPlaceContext = createContext<(place: NavPlace | null) => void>(() => undefined);

/** Marks `path` (a destination's address, or null for none) current while this page is open. */
export function useNavPlace(path: string | null): void {
  const setPlace = useContext(NavPlaceContext);
  const {pathname} = useLocation();
  useEffect(() => {
    setPlace(path ? {pathname, path} : null);
    return () => setPlace(null);
  }, [setPlace, pathname, path]);
}

/** The path the navigation matches against: the page's own place while it is open, else the address. */
export function navPath(pathname: string, place: NavPlace | null): string {
  return place && place.pathname === pathname ? place.path : pathname;
}
