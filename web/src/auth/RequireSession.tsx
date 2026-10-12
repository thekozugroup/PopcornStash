/**
 * Auth gate for the signed-in app. Routes a fresh install to /setup, a
 * signed-out visitor to /login (remembering where they were going), and shows
 * a plain retry screen when the server cannot be reached.
 */
import * as stylex from '@stylexjs/stylex';
import {Button} from '@astryxdesign/core/Button';
import {Spinner} from '@astryxdesign/core/Spinner';
import {Navigate, Outlet, useLocation} from 'react-router';
import {AuthLayout} from './AuthLayout';
import {useAuth} from './AuthProvider';

const styles = stylex.create({
  center: {
    display: 'grid',
    placeItems: 'center',
    minHeight: '100dvh',
  },
});

export function FullPageSpinner() {
  return (
    <div {...stylex.props(styles.center)}>
      <Spinner size="lg" aria-label="Loading PopcornStash" />
    </div>
  );
}

export function RequireSession() {
  const {status, state, error, refresh} = useAuth();
  const location = useLocation();

  if (status === 'loading' && !state) return <FullPageSpinner />;

  if (status === 'error' && !state) {
    return (
      <AuthLayout
        title="PopcornStash is not answering"
        subtitle={error?.message ?? 'The server could not be reached. Check that the container is running, then try again.'}>
        <Button variant="primary" label="Try again" onClick={() => void refresh()} width="100%" />
      </AuthLayout>
    );
  }

  if (state?.setup_required) return <Navigate to="/setup" replace />;

  if (!state?.authenticated) {
    const next = `${location.pathname}${location.search}`;
    const target = next && next !== '/' ? `/login?next=${encodeURIComponent(next)}` : '/login';
    return <Navigate to={target} replace />;
  }

  return <Outlet />;
}
