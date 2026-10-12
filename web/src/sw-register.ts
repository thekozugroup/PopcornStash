/**
 * Registers the minimal service worker (public/sw.js) in production builds.
 * The worker never caches API or media responses; it exists so browsers
 * offer "Install app" and the Android share target.
 */
export function registerServiceWorker(): void {
  if (!import.meta.env.PROD || !('serviceWorker' in navigator) || !window.isSecureContext) return;
  window.addEventListener('load', () => {
    navigator.serviceWorker.register('/sw.js', {scope: '/'}).catch(() => {
      // Installing is optional; the app works without it.
    });
  });
}
