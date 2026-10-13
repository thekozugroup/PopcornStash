/**
 * Providers: the Popcorn Astryx theme (system light/dark), the layer host for
 * dialogs and toasts, React Router links
 * for every Astryx link, session state, and the router.
 */
import {Theme} from '@astryxdesign/core/theme';
import {LayerProvider} from '@astryxdesign/core/Layer';
import {LinkProvider} from '@astryxdesign/core/Link';
import {RouterProvider} from 'react-router/dom';
import {defaultThemeSlug, themes} from '../astryx-themes';
import {AuthProvider} from '../auth/AuthProvider';
import {RouterLink} from '../components/RouterLink';
import {createAppRouter} from './router';

const router = createAppRouter();

export function App() {
  return (
    <Theme theme={themes[defaultThemeSlug]} mode="system">
      <LayerProvider>
        <LinkProvider component={RouterLink}>
          <AuthProvider>
            <RouterProvider router={router} />
          </AuthProvider>
        </LinkProvider>
      </LayerProvider>
    </Theme>
  );
}
