/**
 * Last-resort error screen for a route that failed to load or render
 * (for example a stale chunk after an update).
 */
import {Button} from '@astryxdesign/core/Button';
import {isRouteErrorResponse, useRouteError} from 'react-router';
import {AuthLayout} from '../auth/AuthLayout';

export function RouteError() {
  const error = useRouteError();
  const notFound = isRouteErrorResponse(error) && error.status === 404;
  return (
    <AuthLayout
      title={notFound ? 'Page not found' : 'This page did not load'}
      subtitle={
        notFound
          ? 'That address does not match anything in PopcornStash.'
          : 'PopcornStash may have been updated. Reload to get the latest version.'
      }>
      <Button variant="primary" label="Reload" onClick={() => window.location.reload()} width="100%" />
    </AuthLayout>
  );
}
