/**
 * Sorting: the startup scan files Inbox media into the library on disk, leaves
 * the owner's own folders alone, and holds an ambiguous file in Inbox until
 * someone accepts (or edits) the suggestion.
 */
import fs from 'node:fs';
import path from 'node:path';
import {expect, test} from '@playwright/test';
import {apiJson, env, exists, expectFile, findItem, stashPath, type ItemSummary} from '../helpers/env';

test.use({storageState: async ({}, use) => use(env.ownerState)});

test('the startup scan moved the Inbox files into the library with tidy names', async ({page}) => {
  const moves: [string, string][] = [
    ['Inbox/Harbor.Lights.2019.1080p.WEB-DL.x264-NOGRP.mp4', 'Movies/Harbor Lights (2019)/Harbor Lights (2019).mp4'],
    ['Inbox/Quiet.Meadow.S01E03.720p.WEB.h264.mp4', 'Shows/Quiet Meadow/Season 01/Quiet Meadow - S01E03.mp4'],
    ['Inbox/IMG_1234.MOV', 'Home Videos/2024/06/IMG_1234.MOV'],
  ];
  for (const [from, to] of moves) {
    await expectFile(to);
    expect(exists(from), `${from} should have left the Inbox`).toBe(false);
  }

  // Already organized files stay exactly where they were.
  for (const rel of [
    'Movies/Paper Boats (2011)/Paper Boats (2011).mp4',
    'Shows/The Lantern Keepers/Season 02/The Lantern Keepers - S02E01.mp4',
    'Home Videos/2023/07/IMG_0042.MOV',
    'Photos/2022/05/DSC_0101.jpg',
    'Other/Morning Notes.mp3',
  ]) {
    expect(exists(rel), rel).toBe(true);
  }

  // The owner's own folder is indexed in place and never moved.
  expect(exists('Family 2019/beach.mp4')).toBe(true);
  const beach = await findItem(page.request, 'beach');
  expect(beach.rel_path).toBe('Family 2019/beach.mp4');
  expect(beach.status).toBe('ready');

  const movie = await findItem(page.request, 'Harbor Lights', '&category=movie');
  expect(movie.rel_path).toBe('Movies/Harbor Lights (2019)/Harbor Lights (2019).mp4');
  const phone = await findItem(page.request, 'IMG_1234', '&category=home_video');
  expect(phone.camera).toBe('Apple iPhone 15');
  expect(new Date((phone.taken_at ?? 0) * 1000).toISOString()).toBe('2024-06-13T10:21:00.000Z');
});

test('the ambiguous file waits in Inbox, and Accept with an edited title moves it', async ({page}) => {
  expect(exists('Inbox/clip_final_v2.mkv')).toBe(true);
  const inbox = await apiJson<{items: ItemSummary[]}>(page.request, '/api/inbox');
  expect(inbox.items.map(item => item.rel_path)).toEqual(['Inbox/clip_final_v2.mkv']);
  expect(inbox.items[0]?.suggestion).toMatchObject({category: 'other', confidence: 'low', destination: 'Other/clip_final_v2.mkv'});

  await page.goto('/inbox');
  await expect(page.getByRole('heading', {level: 1, name: 'Inbox'})).toBeVisible();
  const row = page.getByRole('article', {name: 'clip_final_v2.mkv'});
  await expect(row).toBeVisible();
  await expect(row.getByText('Now in Inbox')).toBeVisible();
  await expect(row.getByText('We could not tell what this video is')).toBeVisible();
  await expect(row.getByText('Will move to Other')).toBeVisible();

  await row.getByRole('button', {name: 'Edit'}).click();
  const form = page.getByRole('form', {name: /^Edit where clip_final_v2 goes/});
  await expect(form).toBeVisible();
  await form.getByRole('combobox', {name: 'What is this file?'}).or(form.getByRole('button', {name: /What is this file\?/})).first().click();
  await page.getByRole('option', {name: 'Movie'}).click();
  await form.getByRole('textbox', {name: 'Title'}).fill('Final Cut');
  await form.getByRole('textbox', {name: 'Year'}).fill('2020');
  await expect(form.getByText('Will move to Movies / Final Cut (2020)')).toBeVisible();
  await form.getByRole('button', {name: 'Move it there'}).click();

  await expect(row).toBeHidden();
  await expectFile('Movies/Final Cut (2020)/Final Cut (2020).mkv');
  expect(exists('Inbox/clip_final_v2.mkv')).toBe(false);
  await expect(page.getByRole('heading', {name: 'Nothing needs a decision'})).toBeVisible();
  await expect(page.getByRole('navigation', {name: 'Main navigation'}).getByRole('link', {name: 'Inbox', exact: true})).toBeVisible();

  const item = await findItem(page.request, 'Final Cut', '&category=movie');
  expect(item).toMatchObject({status: 'ready', rel_path: 'Movies/Final Cut (2020)/Final Cut (2020).mkv'});
  expect((await apiJson<{items: unknown[]}>(page.request, '/api/inbox')).items).toHaveLength(0);
});

test('a video copied into Inbox while the server runs is sorted once stable, and Movies updates by itself', async ({page}) => {
  test.setTimeout(90_000);
  await page.goto('/movies');
  const grid = page.getByRole('list', {name: 'Movies'});
  await expect(grid.getByRole('link', {name: /^Paper Boats/})).toBeVisible();
  await expect(grid.getByRole('link', {name: /^Copper Trail/})).toHaveCount(0);

  const name = 'Copper.Trail.2018.720p.WEB.mp4';
  fs.copyFileSync(path.join(env.files, name), stashPath(`Inbox/${name}`));

  // The watcher waits until the file has stopped changing (10 s), then sorts it.
  await expect(grid.getByRole('link', {name: /^Copper Trail 2018/})).toBeVisible({timeout: 60_000});
  await expectFile('Movies/Copper Trail (2018)/Copper Trail (2018).mp4');
  expect(exists(`Inbox/${name}`)).toBe(false);
});

test('a file in an owner folder moves only when the owner organizes it', async ({page, baseURL}) => {
  // Rescans never move it.
  const rescan = await page.request.post('/api/library/rescan', {headers: {Origin: baseURL ?? ''}});
  expect(rescan.status()).toBe(202);
  await page.waitForTimeout(1000);
  expect(exists('Family 2019/beach.mp4')).toBe(true);

  const beach = await findItem(page.request, 'beach');
  await page.goto(`/item/${beach.id}`);
  await expect(page.getByRole('heading', {level: 1, name: 'beach'})).toBeVisible();
  await page.getByRole('button', {name: 'More actions'}).click();
  await page.getByRole('menuitem', {name: 'Organize into library'}).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.getByRole('combobox', {name: 'What is it?'}).or(dialog.getByRole('button', {name: /What is it\?/})).first().click();
  await page.getByRole('option', {name: 'Home video'}).click();
  await dialog.getByRole('button', {name: 'Move to Home Videos'}).click();
  await expect(dialog).toBeHidden();

  await expect.poll(() => exists('Family 2019/beach.mp4')).toBe(false);
  const moved = await findItem(page.request, 'beach');
  expect(moved.id).toBe(beach.id);
  expect(moved.category).toBe('home_video');
  expect(moved.rel_path).toMatch(/^Home Videos\/\d{4}\/\d{2}\/beach\.mp4$/);
  expect(exists(moved.rel_path)).toBe(true);
});

test('an item moved to Trash leaves the library and can be restored to its place', async ({page}) => {
  const rel = 'Movies/Copper Trail (2018)/Copper Trail (2018).mp4';
  const item = await findItem(page.request, 'Copper Trail', '&category=movie');
  expect(item.rel_path).toBe(rel);

  await page.goto(`/item/${item.id}`);
  await page.getByRole('button', {name: 'More actions'}).click();
  await page.getByRole('menuitem', {name: 'Move to Trash'}).click();
  const confirm = page.getByRole('alertdialog', {name: /Move “Copper Trail” to Trash\?/});
  await confirm.getByRole('button', {name: 'Move to Trash'}).click();
  await expect(confirm).toBeHidden();

  // Out of the library folder and into the stash's own trash, not deleted.
  await expect.poll(() => exists(rel)).toBe(false);
  const trashDir = stashPath('.popcornstash/Trash');
  const trashed = fs.readdirSync(trashDir, {recursive: true}).map(String).filter(name => name.endsWith('.mp4'));
  expect(trashed.some(name => name.includes('Copper Trail'))).toBe(true);
  await page.goto('/movies');
  await expect(page.getByRole('heading', {level: 1, name: 'Movies'})).toBeVisible();
  await expect(page.getByRole('list', {name: 'Movies'}).getByRole('link', {name: /^Copper Trail/})).toHaveCount(0);

  await page.goto('/settings/library');
  const row = page.getByRole('listitem').filter({hasText: 'Copper Trail'});
  await expect(row).toBeVisible();
  await row.getByRole('button', {name: 'Restore'}).click();
  await expect(row).toBeHidden();
  await expectFile(rel);
  const restored = await findItem(page.request, 'Copper Trail', '&category=movie');
  expect(restored).toMatchObject({id: item.id, status: 'ready', rel_path: rel});
});
