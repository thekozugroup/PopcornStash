/**
 * Old addresses from before libraries (`/movies`, `/shows`, `/shows/:name`,
 * `/home-videos`, `/photos`) go to the library that takes that kind of file
 * now, when the person can see it, else to Home. The redirect replaces the
 * history entry, so Back skips it; the query (a search, a sort) comes along.
 */
import {Navigate, useLocation, useParams} from 'react-router';
import type {ClaimKind} from '../api/types';
import {useLibraries} from '../app/LibrariesProvider';
import {libraryHref, libraryShowHref} from '../lib/libraries';

export function LegacyLibraryRedirect({kind}: {kind: ClaimKind}) {
  const {name} = useParams();
  const {search} = useLocation();
  const {status, claimedBy} = useLibraries();
  if (status === 'loading') return null;
  const library = claimedBy(kind);
  if (!library) return <Navigate to="/" replace />;
  const target = name ? libraryShowHref(library.id, name) : `${libraryHref(library.id)}${search}`;
  return <Navigate to={target} replace />;
}

export function MoviesRedirect() {
  return <LegacyLibraryRedirect kind="movie" />;
}

export function ShowsRedirect() {
  return <LegacyLibraryRedirect kind="episode" />;
}

export function HomeVideosRedirect() {
  return <LegacyLibraryRedirect kind="home_video" />;
}

export function PhotosRedirect() {
  return <LegacyLibraryRedirect kind="photo" />;
}
