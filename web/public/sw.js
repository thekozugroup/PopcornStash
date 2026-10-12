/*
 * PopcornStash service worker.
 *
 * Deliberately minimal: it makes the app installable (and an Android share
 * target) without caching anything private. API responses, media, thumbnails
 * and streams always go to the network; the browser's own HTTP cache handles
 * the hashed files under /assets/. Nothing is stored by this worker.
 */
const VERSION = 'popcornstash-sw-1';

self.addEventListener('install', () => {
  self.skipWaiting();
});

self.addEventListener('activate', event => {
  event.waitUntil(
    (async () => {
      // Remove any cache an earlier worker might have created.
      const keys = await caches.keys();
      await Promise.all(keys.map(key => caches.delete(key)));
      await self.clients.claim();
    })(),
  );
});

self.addEventListener('message', event => {
  if (event.data === 'version' && event.source) {
    event.source.postMessage(VERSION);
  }
});

// No fetch handler: every request, including /api/* and media, goes straight
// to the network with the browser's normal behaviour.
