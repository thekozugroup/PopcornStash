/**
 * Session state for the whole app, from `GET /api/auth/state`.
 *
 * The provider loads the state once, re-checks it whenever any API call
 * answers 401 (registered with the client), and exposes `refresh` for the
 * sign-in, setup and sign-out flows. It also hands the upload queue to the
 * person signed in, and signing out clears what they left on the device.
 */
import {createContext, use, useCallback, useEffect, useMemo, useRef, useState, type ReactNode} from 'react';
import {ApiError, auth, setUnauthorizedHandler} from '../api/client';
import type {AuthState, User} from '../api/types';
import {clearUploadsForSignOut, setUploadUser} from '../components/uploads/uploadQueue';
import {BrowserIdle, type IdleDeadline} from './browserIdle';

/** Signing out waits this long at most for the device's uploads to be cleaned up. */
const SIGN_OUT_CLEANUP_MS = 3000;

export type AuthStatus = 'loading' | 'ready' | 'error';

export interface AuthContextValue {
  status: AuthStatus;
  /** Last state from the server; null until the first answer. */
  state: AuthState | null;
  /** Set when the server could not be reached on the first load. */
  error: ApiError | null;
  user: User | null;
  isOwner: boolean;
  idleExpired: boolean;
  refresh: (afterPasskey?: boolean) => Promise<AuthState | null>;
  signOut: () => Promise<void>;
}

const AuthContext = createContext<AuthContextValue | null>(null);

export function AuthProvider({children}: {children: ReactNode}) {
  const [status, setStatus] = useState<AuthStatus>('loading');
  const [state, setState] = useState<AuthState | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [idleExpired, setIdleExpired] = useState(false);
  const generation = useRef(0);
  const inflight = useRef<Promise<AuthState | null> | null>(null);
  const idleLocked = useRef(false);

  const refresh = useCallback((afterPasskey = false): Promise<AuthState | null> => {
    if (afterPasskey) { idleLocked.current = false; generation.current += 1; inflight.current = null; }
    if (inflight.current) return inflight.current;
    const epoch = generation.current;
    const pending = auth
      .state()
      .then(next => {
        if (idleLocked.current || epoch !== generation.current) return null;
        if (afterPasskey) setIdleExpired(false);
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
        if (inflight.current === pending) inflight.current = null;
      });
    inflight.current = pending;
    return pending;
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Any 401 from an authenticated call means the session ended: re-check.
  useEffect(() => setUnauthorizedHandler(() => void refresh()), [refresh]);

  // Uploads belong to the person signed in: another person, or nobody, never continues them.
  const userId = state?.authenticated ? (state.user?.id ?? null) : null;
  useEffect(() => setUploadUser(userId), [userId]);

  const idleExpiry = state?.idle_expires_at;
  const serverTime = state?.server_time;
  useEffect(() => {
    if (!userId) return;
    const idle = new BrowserIdle(Date.now(), idleExpiry != null && serverTime != null
      ? {idle_expires_at: idleExpiry, server_time: serverTime} : undefined);
    const channel = typeof BroadcastChannel === 'undefined' ? null : new BroadcastChannel('popcornstash-browser-activity');
    let disposed = false;
    let pending = false;
    let timer: ReturnType<typeof setTimeout>;
    const lock = () => {
      if (disposed || idleLocked.current) return;
      idleLocked.current = true;
      setIdleExpired(true);
      setState(current => current ? {...current, authenticated: false, user: null} : current);
      void clearUploadsForSignOut().catch(() => undefined);
      // Older servers have no idle enforcement. Keep the local gate closed even
      // when this best-effort revocation cannot reach one.
      if (idleExpiry == null) void auth.logout().catch(() => undefined);
    };
    const check = () => {
      clearTimeout(timer);
      if (idle.check(Date.now())) { lock(); return; }
      timer = setTimeout(check, Math.min(idle.remaining(Date.now()), 30_000));
    };
    const acknowledge = (next: IdleDeadline) => {
      if (disposed) return;
      idle.acknowledge(Date.now(), next);
      check();
    };
    if (channel) channel.onmessage = (event: MessageEvent<{user: string; deadline: IdleDeadline; sent: number}>) => {
      const data = event.data;
      if (data?.user !== userId || !Number.isFinite(data.deadline?.server_time) || !Number.isFinite(data.deadline?.idle_expires_at) || !Number.isFinite(data.sent)) return;
      // Account for delivery delay instead of giving a suspended tab a fresh 15 minutes.
      acknowledge({...data.deadline, server_time: data.deadline.server_time + Math.max(0, Date.now() - data.sent) / 1000});
    };
    const input = (event: Event) => {
      check();
      if (!event.isTrusted || pending || !idle.input(Date.now())) return;
      pending = true;
      void auth.activity().then(next => {
        acknowledge(next);
        channel?.postMessage({user: userId, deadline: next, sent: Date.now()});
      }).catch(cause => {
        if (cause instanceof ApiError && cause.status === 401) lock();
      }).finally(() => { pending = false; });
    };
    const wake = () => { check(); if (!idleLocked.current) void refresh(); };
    const events = ['pointerdown', 'pointermove', 'keydown', 'wheel', 'touchstart'] as const;
    events.forEach(name => window.addEventListener(name, input, {capture: true, passive: true}));
    window.addEventListener('focus', wake);
    document.addEventListener('visibilitychange', check);
    check();
    return () => {
      disposed = true;
      clearTimeout(timer);
      channel?.close();
      events.forEach(name => window.removeEventListener(name, input, true));
      window.removeEventListener('focus', wake);
      document.removeEventListener('visibilitychange', check);
    };
  }, [userId, idleExpiry, serverTime, refresh]);

  const signOut = useCallback(async () => {
    try {
      // Nothing this person uploads or shares on the device is left for the next one.
      await Promise.race([
        clearUploadsForSignOut(),
        new Promise(resolve => setTimeout(resolve, SIGN_OUT_CLEANUP_MS)),
      ]).catch(() => undefined);
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
      idleExpired,
      refresh,
      signOut,
    };
  }, [status, state, error, refresh, signOut, idleExpired]);

  return <AuthContext value={value}>{children}</AuthContext>;
}

export function useAuth(): AuthContextValue {
  const value = use(AuthContext);
  if (!value) throw new Error('useAuth must be used inside <AuthProvider>.');
  return value;
}
