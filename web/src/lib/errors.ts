/**
 * One user-facing sentence for any failure: server errors carry their own
 * message; passkey and network failures get plain explanations.
 */
import {ApiError} from '../api/client';
import {PasskeyError} from '../api/webauthn';

export function errorMessage(error: unknown, fallback = 'Something went wrong. Try again.'): string {
  if (error instanceof PasskeyError) return error.message;
  if (error instanceof ApiError) return error.message || fallback;
  return fallback;
}

/** True for a passkey prompt the person closed on purpose (no error to show). */
export function isCancelled(error: unknown): boolean {
  return error instanceof PasskeyError && error.kind === 'cancelled';
}
