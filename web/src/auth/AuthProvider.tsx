/**
 * Session state for the whole app, from `GET /api/auth/state`.
 *
 * The provider loads the state once, re-checks it whenever any API call
 * answers 401 (registered with the client), and exposes `refresh` for the
 * sign-in, setup and sign-out flows.
 */
import {createContext, use, useCallback, useEffect, useMemo, useRef, useState, type ReactNode} from 'react';
import {ApiError, auth, setUnauthorizedHandler} from '../api/client';
import type {AuthState, User} from '../api/types';

export type AuthStatus = 'loading' | 'ready' | 'error';

export interface AuthContextValue {
  status: AuthStatus;
  /** Last state from the server; null until the first answer. */
  state: AuthState | null;
  /** Set when the server could not be reached on the first load. */
  error: ApiError | null;
  user: User | null;
  isOwner: boolean;
  refresh: () => Promise<AuthState | null>;
  signOut: () => Promise<void>;
}

const AuthContext = createContext<AuthContextValue | null>(null);

export function AuthProvider({children}: {children: ReactNode}) {
  const [status, setStatus] = useState<AuthStatus>('loading');
  const [state, setState] = useState<AuthState | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const inflight = useRef<Promise<AuthState | null> | null>(null);

  const refresh = useCallback((): Promise<AuthState | null> => {
    if (inflight.current) return inflight.current;
    const pending = auth
      .state()
      .then(next => {
        setState(next);
        setError(null);
        setStatus('ready');
        return next;
      })
      .catch((cause: unknown) => {
        const apiError =
          cause instanceof ApiError ? cause : new ApiError('unknown', 'Something went wrong.', 0);
        setError(apiError);
        setStatus(current => (current === 'ready' ? 'ready' : 'error'));
        return null;
      })
      .finally(() => {
        inflight.current = null;
      });
    inflight.current = pending;
    return pending;
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Any 401 from an authenticated call means the session ended: re-check.
  useEffect(() => setUnauthorizedHandler(() => void refresh()), [refresh]);

  const signOut = useCallback(async () => {
    try {
      await auth.logout();
    } finally {
      await refresh();
    }
  }, [refresh]);

  const value = useMemo<AuthContextValue>(() => {
    const user = state?.authenticated ? state.user : null;
    return {
      status,
      state,
      error,
      user,
      isOwner: user?.role === 'owner',
      refresh,
      signOut,
    };
  }, [status, state, error, refresh, signOut]);

  return <AuthContext value={value}>{children}</AuthContext>;
}

export function useAuth(): AuthContextValue {
  const value = use(AuthContext);
  if (!value) throw new Error('useAuth must be used inside <AuthProvider>.');
  return value;
}
