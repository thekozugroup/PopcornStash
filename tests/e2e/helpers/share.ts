/**
 * Helpers for the Android share sheet (the PWA share target): small media
 * files made with ffmpeg, the app's service worker in control of a page, and
 * shares posted the way Chrome posts them (multipart form data to the
 * manifest's `share_target` action).
 */
import {execFileSync} from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import type {Page} from '@playwright/test';
import {env} from './env';

/** Cache Storage name the service worker keeps shared files in (web/public/sw.js). */
export const SHARE_CACHE = 'popcornstash-shared-files-v1';

const FFMPEG = ['-nostdin', '-hide_banner', '-loglevel', 'error', '-y'];

/** A short H.264/AAC clip in the test's files folder. */
export function makeClip(name: string, seconds = 3, hue = 200): string {
  const out = path.join(env.files, name);
  execFileSync('ffmpeg', [
    ...FFMPEG,
    '-f', 'lavfi', '-i', `testsrc2=size=320x180:rate=24:duration=${seconds},hue=h=${hue}`,
    '-f', 'lavfi', '-i', `sine=frequency=440:sample_rate=48000:duration=${seconds}`,
    '-map', '0:v', '-map', '1:a',
    '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '30', '-pix_fmt', 'yuv420p',
    '-c:a', 'aac', '-b:a', '64k', '-shortest', '-movflags', '+faststart', out,
  ]);
  return out;
}

/** A small JPEG without EXIF data in the test's files folder. */
export function makePhoto(name: string): string {
  const out = path.join(env.files, name);
  execFileSync('ffmpeg', [...FFMPEG, '-f', 'lavfi', '-i', 'testsrc2=size=480x360', '-frames:v', '1', '-q:v', '5', out]);
  return out;
}

/** True once the server log shows the sorter took `name` from Downloads/Completed. */
export function sortedFromCompleted(name: string): boolean {
  const log = fs.readFileSync(env.serverLog, 'utf8');
  return log.split('\n').some(line => line.includes('sorted a file') && line.includes(`from=Downloads/Completed/${name}`));
}

/** Opens `url` and waits until the app's service worker controls the page. */
export async function withServiceWorker(page: Page, url = '/'): Promise<void> {
  await page.goto(url);
  await page.waitForFunction(() => Boolean(navigator.serviceWorker?.controller), undefined, {timeout: 20_000});
}

export interface SharedFile {
  path: string;
  type: string;
  /** Defaults to the file's own name. */
  name?: string;
}

export interface ShareLanding {
  status: number;
  /** True when the share was answered with a redirect (by the service worker). */
  redirected: boolean;
  /** Where the share landed after the redirect. */
  url: string;
}

/**
 * Posts a share from the page with `fetch`, so the page's service worker
 * receives it exactly as it receives Android's share: a POST of multipart
 * form data to /share with title, text, url and files.
 */
export async function postShare(
  page: Page,
  share: {fields?: Record<string, string>; files?: SharedFile[]},
): Promise<ShareLanding> {
  const files = (share.files ?? []).map(file => ({
    name: file.name ?? path.basename(file.path),
    type: file.type,
    bytes: fs.readFileSync(file.path).toString('base64'),
    lastModified: Math.floor(fs.statSync(file.path).mtimeMs),
  }));
  return page.evaluate(
    async ({fields, files}) => {
      const form = new FormData();
      for (const [name, value] of Object.entries(fields)) form.append(name, value);
      for (const file of files) {
        const data = Uint8Array.from(atob(file.bytes), char => char.charCodeAt(0));
        form.append('files', new File([data], file.name, {type: file.type, lastModified: file.lastModified}));
      }
      const response = await fetch('/share', {method: 'POST', body: form});
      return {status: response.status, redirected: response.redirected, url: response.url};
    },
    {fields: share.fields ?? {}, files},
  );
}

/**
 * Shares text the way a real share sheet does: a form navigation that posts
 * multipart form data to /share, with the empty file part a browser sends
 * when no file was chosen.
 */
export async function submitShareForm(page: Page, fields: Record<string, string>): Promise<void> {
  await page.evaluate(values => {
    const form = document.createElement('form');
    form.method = 'post';
    form.enctype = 'multipart/form-data';
    form.action = '/share';
    for (const [name, value] of Object.entries(values)) {
      const input = document.createElement('input');
      input.type = 'hidden';
      input.name = name;
      input.value = value;
      form.append(input);
    }
    const files = document.createElement('input');
    files.type = 'file';
    files.name = 'files';
    form.append(files);
    document.body.append(form);
    form.submit();
  }, fields);
}

/** Paths of everything the service worker keeps for shares on this device. */
export async function storedShareKeys(page: Page): Promise<string[]> {
  return page.evaluate(async cacheName => {
    if (!(await caches.has(cacheName))) return [];
    const cache = await caches.open(cacheName);
    return (await cache.keys()).map(request => new URL(request.url).pathname).sort();
  }, SHARE_CACHE);
}
