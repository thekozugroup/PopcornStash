/** Real folders, bulk moves, Inbox deletion and picture gestures; isolated fixture only. */
import fs from 'node:fs';
import path from 'node:path';
import {expect, test} from '@playwright/test';
import {env, findItem, LIBRARY} from '../helpers/env';

test.use({storageState: async ({}, use) => use(env.ownerState)});
test.describe.configure({mode: 'serial'});

test('create a subfolder and move a searched selection without changing watch history', async ({page}) => {
  await page.goto('/');
  const tin = await findItem(page.request, 'Tin Lighthouse');
  const original = path.join(env.stash, tin.rel_path);
  const parent = path.dirname(original);
  // Both files have valid media bytes, independent of the fixture's existing titles.
  fs.copyFileSync(original, path.join(env.stash, 'Movies', 'Collection One (2020).mp4'));
  fs.copyFileSync(original, path.join(env.stash, 'Movies', 'Collection Two (2021).mp4'));
  const headers = {Origin: env.publicUrl};
  expect((await page.request.post('/api/library/rescan', {headers})).status()).toBe(202);
  await page.goto(`/libraries/${LIBRARY.movies}/folders`);
  await page.getByLabel('Search this folder and its subfolders', {exact: true}).fill('Collection');
  await expect(page.getByRole('checkbox')).toHaveCount(2);
  const result = await (await page.request.get(`/api/items?library_id=${LIBRARY.movies}&q=Collection`)).json() as {items: {id: string}[]};
  const id = result.items[0]!.id;
  expect((await page.request.put(`/api/items/${id}/progress`, {headers, data: {position_s: 20, duration_s: 45}})).status()).toBe(204);
  await page.getByLabel('New subfolder in this destination').fill('Favorites');
  await page.getByRole('button', {name: 'Create folder', exact: true}).click();
  await expect.poll(() => fs.existsSync(path.join(env.stash, 'Movies', 'Favorites'))).toBe(true);
  await expect(page.getByRole('checkbox')).toHaveCount(2);
  await page.getByRole('button', {name: 'Select shown (2)', exact: true}).click();
  // Folder appears twice: source and destination. Choose it in the destination browser.
  await page.getByRole('button', {name: 'Favorites', exact: true}).last().click();
  await page.getByRole('button', {name: 'Move selected (2)', exact: true}).click();
  await page.getByRole('alertdialog').getByRole('button', {name: 'Move files', exact: true}).click();
  await expect.poll(() => fs.existsSync(path.join(env.stash, 'Movies', 'Favorites', 'Collection One (2020).mp4'))).toBe(true);
  const fresh = await (await page.request.get(`/api/items/${id}`)).json() as {progress: {position_s: number}; rel_path: string};
  expect(fresh.progress.position_s).toBe(20);
  expect(fresh.rel_path).toContain('Movies/Favorites/');
  expect(fs.existsSync(parent)).toBe(true);
  await page.screenshot({path: path.join(env.work, 'folders-desktop.png'), fullPage: true});
  await page.setViewportSize({width: 390, height: 844});
  await page.screenshot({path: path.join(env.work, 'folders-phone.png'), fullPage: true});
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});

test('Search navigation searches globally and inside one library', async ({page}) => {
  await page.goto('/search?q=Collection');
  await expect(page.getByRole('heading', {name: 'Search', exact: true})).toBeVisible();
  await expect(page.getByRole('link', {name: /Collection One/})).toBeVisible();
  await page.goto(`/search?library=${LIBRARY.photos}&q=Collection`);
  await expect(page.getByText('No files match “Collection”', {exact: true})).toBeVisible();
  await page.goto(`/search?library=${LIBRARY.movies}&q=Collection`);
  await expect(page.getByRole('link', {name: /Collection Two/})).toBeVisible();
});

test('bulk image deletion removes waiting and ignored files and an older read cannot restore rows', async ({page}) => {
  await page.goto('/');
  const headers = {Origin: env.publicUrl};
  expect((await page.request.put('/api/settings', {headers, data: {auto_sort: false}})).status()).toBe(200);
  for (const name of ['bulk-cover-a.jpg', 'bulk-cover-b.jpg', 'bulk-keep.txt']) {
    const file = path.join(env.stash, 'Inbox', name);
    fs.writeFileSync(file, `Synthetic fixture ${name}`);
    const old = new Date(Date.now() - 3_600_000); fs.utimesSync(file, old, old);
  }
  await page.request.post('/api/library/rescan', {headers});
  await page.goto('/inbox');
  await expect(page.getByRole('checkbox', {name: 'Select bulk-cover-a.jpg', exact: true})).toBeVisible();
  await page.getByRole('combobox', {name: 'File type', exact: true}).click();
  await page.getByRole('option', {name: 'Images', exact: true}).click();
  const matching = page.getByRole('checkbox', {name: /Select bulk-cover-/});
  await expect(matching).toHaveCount(2);
  await matching.nth(0).check(); await matching.nth(1).check();
  let releaseRead: () => void = () => {};
  let readCaptured: () => void = () => {};
  const captured = new Promise<void>(resolve => { readCaptured = resolve; });
  const held = new Promise<void>(resolve => { releaseRead = resolve; });
  await page.route('**/api/inbox', async route => {
    const response = await route.fetch();
    readCaptured();
    await held;
    await route.fulfill({response});
  }, {times: 1});
  const current = await (await page.request.get('/api/inbox')).json() as {items: {id: string}[]; ignored: {id: string}[]};
  const row = [...current.items, ...current.ignored][0]!;
  // An item event starts a background read. Hold its pre-deletion snapshot.
  await page.request.patch(`/api/items/${row.id}`, {headers, data: {favorite: true}});
  await Promise.race([captured, new Promise((_, reject) => setTimeout(() => reject(new Error("Inbox refresh was not captured")), 8000))]);
  try {
  await page.getByRole('button', {name: 'Delete selected (2)', exact: true}).click();
  await page.getByRole('alertdialog').getByRole('button', {name: 'Move to Trash', exact: true}).click();
  await expect(matching).toHaveCount(0);
  } finally { releaseRead(); }
  await page.waitForTimeout(700);
  await expect(matching).toHaveCount(0);
  await page.reload();
  await expect(page.getByRole('checkbox', {name: /Select bulk-cover-/})).toHaveCount(0);
  expect(fs.existsSync(path.join(env.stash, 'Inbox', 'bulk-keep.txt'))).toBe(true);
  await page.request.post('/api/library/rescan', {headers});
  const inbox = await (await page.request.get('/api/inbox')).json() as {items: {title: string}[]; ignored: {title: string}[]};
  expect([...inbox.items, ...inbox.ignored].some(i => i.title.startsWith('bulk-cover-'))).toBe(false);
});

test('picture double taps seek fifteen seconds and clamp at media boundaries', async ({page}) => {
  const tin = await findItem(page.request, 'Tin Lighthouse');
  await page.goto(`/item/${tin.id}`);
  await page.getByRole('button', {name: /^(Play|Resume from)/}).first().click();
  const video = page.locator('video');
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime)).toBeGreaterThan(.2);
  await video.evaluate((v: HTMLVideoElement) => { v.pause(); v.currentTime = 20; });
  const rect = (await video.boundingBox())!;
  await video.dblclick({position: {x: rect.width * .1, y: rect.height * .4}});
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime)).toBeCloseTo(5, 0);
  await video.dblclick({position: {x: rect.width * .9, y: rect.height * .4}});
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime)).toBeCloseTo(20, 0);
  await video.evaluate((v: HTMLVideoElement) => { v.currentTime = 2; });
  await page.getByRole('button', {name: 'Back 15 seconds', exact: true}).click();
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime)).toBe(0);
  await video.evaluate((v: HTMLVideoElement) => { v.currentTime = 40; });
  await video.dblclick({position: {x: rect.width * .9, y: rect.height * .4}});
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => Math.abs(v.currentTime - v.duration))).toBeLessThan(.2);
});
