/**
 * Route table. Auth pages render outside the shell; everything else sits
 * behind the session gate inside the app shell. Content pages load lazily so
 * the player (hls.js) and other heavy pages do not weigh down sign-in.
 */
import type {ComponentType} from 'react';
import {Navigate, createBrowserRouter, type RouteObject} from 'react-router';
import type * as Redirects from '../pages/LegacyLibraryRedirect';
import {FullPageSpinner, RequireSession} from '../auth/RequireSession';
import LoginPage from '../pages/LoginPage';
import SetupPage from '../pages/SetupPage';
import {AppLayout} from './AppLayout';
import {RouteError} from './RouteError';

type PageModule = {default: ComponentType};

/** Lazy route: `page(() => import('../pages/LibraryPage'))`. */
function page(load: () => Promise<PageModule>): Pick<RouteObject, 'lazy'> {
  return {lazy: async () => ({Component: (await load()).default})};
}

/** An old fixed-category address: redirects to the library that takes that kind now. */
function legacy(pick: (module: typeof Redirects) => ComponentType): Pick<RouteObject, 'lazy'> {
  return {lazy: async () => ({Component: pick(await import('../pages/LegacyLibraryRedirect'))})};
}

export const routes: RouteObject[] = [
  {
    errorElement: <RouteError />,
    hydrateFallbackElement: <FullPageSpinner />,
    children: [
      {path: '/setup', element: <SetupPage />},
      {path: '/login', element: <LoginPage />},
      {path: '/invite/:code', ...page(() => import('../pages/InvitePage'))},
      {path: '/recover/:code', ...page(() => import('../pages/RecoverPage'))},
      {
        element: <RequireSession />,
        children: [
          {
            element: <AppLayout />,
            children: [
              {index: true, ...page(() => import('../pages/HomePage'))},
              {path: 'search', ...page(() => import('../pages/SearchPage'))},
              {path: 'libraries/:id/folders', ...page(() => import('../pages/LibraryFoldersPage'))},
              {path: 'libraries/:id', ...page(() => import('../pages/LibraryPage'))},
              {path: 'libraries/:id/shows/:name', ...page(() => import('../pages/ShowPage'))},
              {path: 'movies', ...legacy(module => module.MoviesRedirect)},
              {path: 'shows', ...legacy(module => module.ShowsRedirect)},
              {path: 'shows/:name', ...legacy(module => module.ShowsRedirect)},
              {path: 'home-videos', ...legacy(module => module.HomeVideosRedirect)},
              {path: 'photos', ...legacy(module => module.PhotosRedirect)},
              {path: 'other', ...page(() => import('../pages/OtherPage'))},
              {path: 'item/:id', ...page(() => import('../pages/ItemPage'))},
              {path: 'clean', ...page(() => import('../pages/CleanFilesPage'))},
              {path: 'inbox', ...page(() => import('../pages/InboxPage'))},
              {path: 'downloads', ...page(() => import('../pages/DownloadsPage'))},
              {path: 'share', ...page(() => import('../pages/SharePage'))},
              {path: 'pair', ...page(() => import('../pages/PairPage'))},
              {
                path: 'settings',
                lazy: async () => {
                  const module = await import('../pages/SettingsPage');
                  return {Component: module.default};
                },
                children: [
                  {
                    index: true,
                    lazy: async () => ({Component: (await import('../pages/SettingsPage')).SettingsIndex}),
                  },
                  {path: 'vpn', ...page(() => import('../pages/settings/VpnSettingsPage'))},
                  {path: 'devices', ...page(() => import('../pages/settings/DevicesSettingsPage'))},
                  {path: 'people', ...page(() => import('../pages/settings/PeopleSettingsPage'))},
                  {path: 'security', ...page(() => import('../pages/settings/SecuritySettingsPage'))},
                  {path: 'libraries', ...page(() => import('../pages/settings/LibrariesSettingsPage'))},
                  {path: 'libraries/new', ...page(() => import('../pages/settings/LibraryEditorPage'))},
                  {path: 'libraries/:id', ...page(() => import('../pages/settings/LibraryEditorPage'))},
                  {path: 'library', element: <Navigate to="/settings/libraries" replace />},
                  {path: 'system', ...page(() => import('../pages/settings/SystemSettingsPage'))},
                ],
              },
              {path: '*', ...page(() => import('../pages/NotFoundPage'))},
            ],
          },
        ],
      },
    ],
  },
];

export function createAppRouter() {
  return createBrowserRouter(routes);
}
