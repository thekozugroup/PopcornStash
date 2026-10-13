/*
 * PopcornStash service worker.
 *
 * It makes the app installable and receives what Android's share sheet sends
 * (the manifest's share_target: a POST of multipart form data to /share). It
 * never caches anything private: API responses, media, thumbnails and streams
 * always go to the network, and the browser's own HTTP cache handles the
 * hashed files under /assets/.
 *
 * A share with files: the files are kept in Cache Storage on this device
 * (SHARE_CACHE) and the share page opens with ?files=<id>. Keeping them lets
 * the person check them first, sign in first if needed, and lets an upload
 * cut off by a reload continue. The page uploads them through the normal
 * upload queue and deletes each kept copy once it is uploaded, or all of them
 * on Cancel (web/src/components/uploads/sharedFiles.ts reads the same cache).
 * A share without files (a link or text) opens /share?title&text&url as
 * before. Shares nobody acted on are deleted after SHARE_MAX_AGE_MS.
 */
const VERSION = 'popcornstash-sw-2';

/** Keep in step with web/src/components/uploads/sharedFiles.ts. */
const SHARE_CACHE = 'popcornstash-shared-files-v1';
const SHARE_PREFIX = '/shared-files/';
const SHARE_MAX_AGE_MS = 7 * 24 * 3600 * 1000;

self.addEventListener('install', event => {
  self.skipWaiting();
  // Requests this worker never answers skip it entirely where the browser
  // supports static routes: API calls, the event stream, uploads, media and
  // the hashed assets go straight to the network.
  if (typeof event.addRoutes === 'function' && typeof self.URLPattern === 'function') {
    const routes = ['/api/*', '/assets/*'].map(pathname => ({
      condition: {urlPattern: new self.URLPattern({pathname})},
      source: 'network',
    }));
    event.waitUntil(event.addRoutes(routes).catch(() => undefined));
  }
});

self.addEventListener('activate', event => {
  event.waitUntil(
    (async () => {
      // Remove any cache an earlier worker might have created; shares waiting for upload stay.
      const keys = await caches.keys();
      await Promise.all(keys.filter(key => key !== SHARE_CACHE).map(key => caches.delete(key)));
      await self.clients.claim();
    })(),
  );
});

self.addEventListener('message', event => {
  if (event.data === 'version' && event.source) {
    event.source.postMessage(VERSION);
  }
});

// Only a share (POST /share) is answered here. Every other request, including
// /api/* and media, gets no response from this worker and goes to the network
// with the browser's normal behaviour.
self.addEventListener('fetch', event => {
  const {request} = event;
  if (request.method !== 'POST') return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin || url.pathname !== '/share') return;
  event.respondWith(receiveShare(request));
});

/** A 303 to a page of the app, so the browser opens it with GET. */
function openPage(path) {
  return Response.redirect(new URL(path, self.location.origin).href, 303);
}

async function receiveShare(request) {
  let form;
  try {
    form = await request.formData();
  } catch {
    return openPage('/share?failed=files');
  }
  // A form without a chosen file still sends an empty, nameless file part.
  const files = form.getAll('files').filter(value => typeof value !== 'string' && (value.size > 0 || value.name));
  if (files.length === 0) {
    const query = new URLSearchParams();
    for (const name of ['title', 'text', 'url']) {
      const value = form.get(name);
      if (typeof value === 'string' && value.trim()) query.set(name, value.trim());
    }
    const search = query.toString();
    return openPage(search ? `/share?${search}` : '/share');
  }
  try {
    const id = await keepFiles(files);
    return openPage(`/share?files=${id}`);
  } catch {
    // Usually no room left on the device.
    return openPage('/share?failed=files');
  }
}

/** Stores the files and a note listing them; returns the share's id. */
async function keepFiles(files) {
  const cache = await caches.open(SHARE_CACHE);
  await deleteOldShares(cache);
  const id = crypto.randomUUID();
  const listKey = `${SHARE_PREFIX}${id}/share.json`;
  const records = [];
  try {
    for (const [index, file] of files.entries()) {
      const key = `${SHARE_PREFIX}${id}/${index}`;
      const type = file.type || 'application/octet-stream';
      await cache.put(key, new Response(file, {headers: {'Content-Type': type}}));
      records.push({
        key,
        name: file.name || `Shared file ${index + 1}`,
        type: file.type,
        size: file.size,
        lastModified: file.lastModified || Date.now(),
      });
    }
    await cache.put(listKey, Response.json({id, savedAt: Date.now(), files: records}));
  } catch (error) {
    await Promise.all([listKey, ...records.map(record => record.key)].map(key => cache.delete(key)));
    throw error;
  }
  return id;
}

/** Deletes shares older than SHARE_MAX_AGE_MS that were never uploaded or cancelled. */
async function deleteOldShares(cache) {
  const now = Date.now();
  const requests = await cache.keys();
  const expired = new Set();
  for (const request of requests) {
    const path = new URL(request.url).pathname;
    if (!path.startsWith(SHARE_PREFIX) || !path.endsWith('/share.json')) continue;
    try {
      const share = await (await cache.match(request)).json();
      if (!(now - share.savedAt < SHARE_MAX_AGE_MS)) expired.add(path.slice(0, -'share.json'.length));
    } catch {
      expired.add(path.slice(0, -'share.json'.length));
    }
  }
  if (expired.size === 0) return;
  await Promise.all(
    requests
      .filter(request => [...expired].some(prefix => new URL(request.url).pathname.startsWith(prefix)))
      .map(request => cache.delete(request)),
  );
}
