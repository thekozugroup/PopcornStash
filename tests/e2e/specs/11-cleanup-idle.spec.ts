/** Real-file cleanup, filtered bulk import, and the browser's privacy gate. */
import fs from 'node:fs';
import {execFileSync} from 'node:child_process';
import path from 'node:path';
import {expect, test} from '@playwright/test';
import {env, signInWithPasskey} from '../helpers/env';

test.use({storageState: async ({}, use) => use(env.ownerState)});
test.describe.configure({mode: 'serial'});

test('cleanup removes only selected companions and whole-folder removal is opt in', async ({page}) => {
  const directory = path.join(env.stash, 'Cleanup fixture');
  fs.mkdirSync(directory);
  fs.writeFileSync(path.join(directory, 'video.mkv'), 'synthetic test video');
  fs.writeFileSync(path.join(directory, 'captions.srt'), '1\n00:00:00,000 --> 00:00:01,000\nExample');
  fs.writeFileSync(path.join(directory, 'instructions.txt'), 'Synthetic test instructions');
  await page.goto('/clean?folder=Cleanup%20fixture');
  await expect(page.getByRole('heading', {name: 'Clean files', exact: true})).toBeVisible();
  await page.getByRole('button', {name: 'Keep videos only', exact: true}).click();
  await expect(page.getByRole('checkbox', {name: 'video.mkv', exact: true})).not.toBeChecked();
  await expect(page.getByRole('checkbox', {name: 'captions.srt', exact: true})).toBeChecked();
  await page.screenshot({path: path.join(env.work, 'cleanup-desktop.png'), fullPage: true});
  await page.setViewportSize({width: 390, height: 844});
  await page.screenshot({path: path.join(env.work, 'cleanup-phone.png'), fullPage: true});
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.getByRole('button', {name: 'Move selected to Trash', exact: true}).click();
  await page.getByRole('alertdialog').getByRole('button', {name: 'Move to Trash', exact: true}).click();
  await expect.poll(() => fs.readdirSync(directory)).toEqual(['video.mkv']);
  const trash = await (await page.request.get('/api/files/trash')).json() as {id: string; original_rel_path: string}[];
  const subtitle = trash.find(entry => entry.original_rel_path.endsWith('captions.srt'))!;
  expect((await page.request.post(`/api/files/trash/${subtitle.id}/restore`, {data: {}, headers: {Origin: env.publicUrl}})).status()).toBe(204);
  expect(fs.existsSync(path.join(directory, 'captions.srt'))).toBe(true);
  const created = await page.request.post('/api/libraries', {headers: {Origin: env.publicUrl}, data: {
    name: 'Cleanup fixture', icon: 'film', content_type: 'video', structure: 'one_offs', claims: [],
    folder: {mode: 'existing', name: 'Cleanup fixture'}, visibility: 'selected', member_ids: [],
  }});
  expect(created.status(), await created.text()).toBe(201);
  const library = await created.json() as {id: string};
  await page.goto(`/settings/libraries/${library.id}`);
  await page.getByRole('button', {name: 'Delete library', exact: true}).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog.getByRole('checkbox')).not.toBeChecked();
  await dialog.getByRole('checkbox').check();
  await expect(dialog.getByRole('button', {name: 'Delete library and folder'})).toBeDisabled();
  await dialog.getByLabel('Type Cleanup fixture to confirm').fill('Cleanup fixture');
  await dialog.getByRole('button', {name: 'Delete library and folder'}).click();
  await expect.poll(() => fs.existsSync(directory)).toBe(false);
});

test('Inbox imports a selected file type without importing other matching uploads', async ({page}) => {
  await page.goto('/');
  await page.request.patch('/api/settings', {headers: {Origin: env.publicUrl}, data: {auto_sort: false}});
  for (const name of ['bulk-one.txt', 'bulk-two.txt']) {
    const file = path.join(env.stash, 'Inbox', name);
    fs.writeFileSync(file, 'Synthetic bulk import fixture');
    const old = new Date(Date.now() - 3_600_000); fs.utimesSync(file, old, old);
  }
  await page.request.post('/api/library/rescan', {headers: {Origin: env.publicUrl}});
  await page.goto('/inbox');
  await expect(page.getByRole('checkbox', {name: 'Select bulk-one.txt', exact: true})).toBeVisible();
  await page.getByRole('checkbox', {name: 'Select bulk-one.txt', exact: true}).check();
  await page.getByRole('button', {name: 'Import selected (1)', exact: true}).click();
  await page.getByRole('alertdialog').getByRole('button', {name: 'Import selected', exact: true}).click();
  await expect(page.getByRole('checkbox', {name: 'Select bulk-one.txt', exact: true})).toHaveCount(0);
  await expect(page.getByRole('checkbox', {name: 'Select bulk-two.txt', exact: true})).toBeVisible();
  expect(fs.existsSync(path.join(env.stash, 'Inbox', 'bulk-two.txt'))).toBe(true);
});

test('background traffic cannot keep private pages open after fifteen idle minutes', async ({page}) => {
  // Age only this isolated fixture's cookies to three seconds before the real
  // fifteen-minute deadline. No fake browser timers or shortened product policy.
  execFileSync('sqlite3', [path.join(env.config, 'popcornstash.db'), "UPDATE sessions SET last_activity_at = unixepoch() - 897 WHERE kind = 'cookie'"]);
  await page.goto('/');
  await expect(page.getByRole('heading', {name: 'Home', exact: true})).toBeVisible();
  expect((await page.request.get('/api/me')).status()).toBe(200);
  await expect(page).toHaveURL(/\/login/, {timeout: 10000});
  await expect(page.getByRole('button', {name: 'Sign in with a passkey', exact: true})).toBeVisible();
  await expect(page.getByRole('heading', {name: 'Home', exact: true})).toHaveCount(0);
  expect((await page.request.get('/api/me')).status()).toBe(401);
  await signInWithPasskey(page);
  await expect(page.getByRole('heading', {name: 'Home', exact: true})).toBeVisible();
});
