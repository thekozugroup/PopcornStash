/**
 * A friendly default name for a new passkey, so people can tell their
 * passkeys apart later in Settings ("iPhone", "Mac", "Windows PC").
 */
export function guessDeviceName(): string {
  const ua = navigator.userAgent;
  if (/iPhone/i.test(ua)) return 'iPhone';
  if (/iPad/i.test(ua)) return 'iPad';
  // iPadOS reports a Mac user agent with touch support.
  if (/Macintosh/i.test(ua) && navigator.maxTouchPoints > 1) return 'iPad';
  if (/Android/i.test(ua)) return /Mobile/i.test(ua) ? 'Android phone' : 'Android tablet';
  if (/Macintosh|Mac OS X/i.test(ua)) return 'Mac';
  if (/Windows/i.test(ua)) return 'Windows PC';
  if (/CrOS/i.test(ua)) return 'Chromebook';
  if (/Linux/i.test(ua)) return 'Linux computer';
  return 'This device';
}

/** Only same-app paths are allowed as a post-sign-in destination. */
export function safeNextPath(value: string | null | undefined): string {
  if (!value || !value.startsWith('/') || value.startsWith('//') || value.startsWith('/\\')) return '/';
  if (value.startsWith('/login') || value.startsWith('/setup')) return '/';
  return value;
}
