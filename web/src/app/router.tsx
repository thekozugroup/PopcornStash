/**
 * Route table. Auth pages render outside the shell; everything else sits
 * behind the session gate inside the app shell. Content pages load lazily so
 * the player (hls.js) and other heavy pages do not weigh down sign-in.
 */
import type {ComponentType} from 'react';
import {createBrowserRouter, type RouteObject} from 'react-router';
import {FullPageSpinner, RequireSession} from '../auth/RequireSession';
import LoginPage from '../pages/LoginPage';
import SetupPage from '../pages/SetupPage';
import {AppLayout} from './AppLayout';
import {RouteError} from './RouteError';

type PageModule = {default: ComponentType};

/** Lazy route: `page(() => import('../pages/MoviesPage'))`. */
function page(load: () => Promise<PageModule>): Pick<RouteObject, 'lazy'> {
  return {lazy: async () => ({Component: (await load()).default})};
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
              {path: 'movies', ...page(() => import('../pages/MoviesPage'))},
              {path: 'shows', ...page(() => import('../pages/ShowsPage'))},
              {path: 'shows/:name', ...page(() => import('../pages/ShowPage'))},
              {path: 'home-videos', ...page(() => import('../pages/HomeVideosPage'))},
              {path: 'photos', ...page(() => import('../pages/PhotosPage'))},
              {path: 'other', ...page(() => import('../pages/OtherPage'))},
              {path: 'item/:id', ...page(() => import('../pages/ItemPage'))},
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
                  {path: 'library', ...page(() => import('../pages/settings/LibrarySettingsPage'))},
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
