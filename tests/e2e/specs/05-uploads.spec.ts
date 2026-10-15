/**
 * Browser uploads: from the Downloads page and by dropping files anywhere.
 * Each upload shows progress, passes through Downloads/Completed, is sorted,
 * and shows up in the library.
 */
import fs from 'node:fs';
import path from 'node:path';
import {expect, test, type Page} from '@playwright/test';
import {LIBRARY, env, exists, expectFile, expectNotClipped, findItem, libraryPath} from '../helpers/env';

test.use({storageState: async ({}, use) => use(env.ownerState)});

/** Holds each upload chunk for a moment so the progress row can be seen. */
async function slowUploads(page: Page) {
  await page.route(/\/api\/uploads\/[^/]+\?offset=\d+$/, async route => {
    await new Promise(resolve => setTimeout(resolve, 1200));
    await route.continue();
  });
}

function sortedFromCompleted(name: string): boolean {
  const log = fs.readFileSync(env.serverLog, 'utf8');
  return log.split('\n').some(line => line.includes('sorted a file') && line.includes(`from=Downloads/Completed/${name}`));
}

test('a video uploaded from the Downloads page is sorted into Movies', async ({page}) => {
  const name = 'Sunset.Drive.2020.720p.WEB.mp4';
  await slowUploads(page);
  await page.goto('/downloads');
  await expect(page.getByRole('heading', {level: 1, name: 'Downloads'})).toBeVisible();

  const chooser = page.waitForEvent('filechooser');
  await page.getByRole('button', {name: 'Upload files'}).click();
  await (await chooser).setFiles(path.join(env.files, name));

  // While it uploads: a row with the file name, the state in words and a real progress bar.
  await expect(page.getByText(name).first()).toBeVisible();
  await expect(page.getByText(/^Upload · Uploading/).first()).toBeVisible();
  await expect(page.getByRole('progressbar', {name: `${name} progress`})).toBeVisible();

  // Then it is finished, moved through Downloads/Completed and sorted: the row names the library.
  const done = page.getByText(/^In Movies$/).first();
  await expect(done).toBeVisible({timeout: 30_000});
  await expect(done.getByRole('link', {name: 'Movies'})).toHaveAttribute('href', libraryPath(LIBRARY.movies));
  await expectFile('Movies/Sunset Drive (2020)/Sunset Drive (2020).mp4');
  expect(sortedFromCompleted(name), 'the sorter took the upload from Downloads/Completed').toBe(true);
  expect(exists(`Downloads/Completed/${name}`)).toBe(false);
  expect(fs.readdirSync(path.join(env.stash, '.popcornstash', 'uploads'))).toEqual([]);

  const downloads = await (await page.request.get('/api/downloads?state=all')).json();
  const download = downloads.items.find((d: {name: string}) => d.name === name);
  expect(download).toMatchObject({kind: 'upload', state: 'sorted', progress: 1});
  expect(download.item_ids).toHaveLength(1);
  expect(download.destinations).toEqual([{place: 'library', library_id: LIBRARY.movies, count: 1}]);

  // Nothing is in progress any more, and the page says so in a padded panel.
  await expectNotClipped(page.getByText('Nothing is downloading right now.', {exact: false}));

  const item = await findItem(page.request, 'Sunset Drive', '&category=movie');
  expect(download.item_ids[0]).toBe(item.id);
  await page.goto(libraryPath(LIBRARY.movies));
  await expect(page.getByRole('list', {name: 'Movies'}).getByRole('link', {name: /^Sunset Drive 2020/})).toBeVisible();
});

test('files dropped anywhere in the app are uploaded and sorted', async ({page}) => {
  const name = 'Kite.Festival.2021.1080p.WEB.mp4';
  await slowUploads(page);
  await page.goto(libraryPath(LIBRARY.movies));
  await expect(page.getByRole('heading', {level: 1, name: 'Movies'})).toBeVisible();

  const bytes = fs.readFileSync(path.join(env.files, name)).toString('base64');
  const transfer = await page.evaluateHandle(
    ({bytes, name}) => {
      const data = Uint8Array.from(atob(bytes), char => char.charCodeAt(0));
      const dt = new DataTransfer();
      dt.items.add(new File([data], name, {type: 'video/mp4', lastModified: Date.now()}));
      return dt;
    },
    {bytes, name},
  );
  // Dragging files over the window shows the drop overlay.
  await page.getByRole('main').dispatchEvent('dragenter', {dataTransfer: transfer});
  await page.getByRole('main').dispatchEvent('dragover', {dataTransfer: transfer});
  await expect(page.getByRole('heading', {name: 'Drop to upload'})).toBeVisible();
  await page.getByRole('main').dispatchEvent('drop', {dataTransfer: transfer});

  // The drop goes to Downloads, where the upload starts.
  await expect(page).toHaveURL(/\/downloads$/);
  await expect(page.getByRole('heading', {name: 'Drop to upload'})).toBeHidden();
  await expect(page.getByRole('progressbar', {name: `${name} progress`})).toBeVisible();
  await expect(page.getByText(/^In Movies$/).first()).toBeVisible({timeout: 30_000});
  await expectFile('Movies/Kite Festival (2021)/Kite Festival (2021).mp4');
  expect(sortedFromCompleted(name)).toBe(true);

  await page.goto('/');
  await expect(page.getByRole('region', {name: 'Movies', exact: true}).getByRole('link', {name: /^Kite Festival 2021/})).toBeVisible();
});

test('a paused upload keeps showing what the server already has', async ({page}) => {
  // Big enough that the first 16 MiB chunk is still on its way when it is paused.
  const name = 'Family archive.zip';
  const file = path.join(env.files, name);
  fs.writeFileSync(file, Buffer.alloc(40_000_000, 7));
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Network.enable');
  await cdp.send('Network.emulateNetworkConditions', {
    offline: false,
    latency: 20,
    downloadThroughput: -1,
    uploadThroughput: 2 * 1024 * 1024,
  });

  await page.goto('/downloads');
  const chooser = page.waitForEvent('filechooser');
  await page.getByRole('button', {name: 'Upload files'}).click();
  await (await chooser).setFiles(file);
  const row = page.getByRole('listitem').filter({hasText: name}).first();
  const sentMb = async () => {
    const match = /([\d.]+) MB of 40 MB/.exec(await row.innerText());
    return match ? Number(match[1]) : 0;
  };
  await expect.poll(sentMb, {timeout: 20_000}).toBeGreaterThan(3);

  await page.getByRole('button', {name: `Pause ${name}`}).click();
  await expect(row.getByText(/^Upload · Paused/)).toBeVisible();
  // Not back to 0 B: the part of the chunk that arrived is kept and counted.
  await expect.poll(sentMb).toBeGreaterThan(2);
  expect(await row.innerText()).not.toContain('0 B of 40 MB');

  await cdp.send('Network.emulateNetworkConditions', {offline: false, latency: 0, downloadThroughput: -1, uploadThroughput: -1});
  await page.getByRole('button', {name: `More for ${name}`}).click();
  await page.getByRole('menuitem', {name: 'Cancel upload…'}).click();
  await page.getByRole('alertdialog').getByRole('button', {name: 'Cancel upload'}).click();
  await expect(row).toBeHidden();
  await expect.poll(() => fs.readdirSync(path.join(env.stash, '.popcornstash', 'uploads'))).toEqual([]);
  fs.rmSync(file);
});


test('an uncertain chosen upload moves immediately after interruption while automatic sorting remains available', async ({page}) => {
  await page.goto('/downloads');
  const made = await page.request.post('/api/libraries', {headers: {Origin: new URL(env.publicUrl).origin}, data: {
    name: 'Test movies', icon: 'film', content_type: 'video', structure: 'one_offs', claims: [],
    folder: {mode: 'new', name: 'Test movies'},
  }});
  expect(made.status()).toBe(201);
  const library = await made.json();
  await page.reload();
  // The existing library claim is unchanged; the selection applies only to this upload.
  const picker = page.getByRole('combobox', {name: 'Where it goes'});
  await picker.click();
  await page.getByRole('option', {name: 'Test movies', exact: true}).click();
  const bytes = fs.readFileSync(path.join(env.files, 'Sunset.Drive.2020.720p.WEB.mp4'));
  const chosenFile = path.join(env.files, 'Chosen.Dawn.1080p.WEB.mp4');
  fs.writeFileSync(chosenFile, bytes);
  let uploadId = '';
  await page.route(/\/api\/uploads\/[^/]+\?offset=\d+$/, route => route.abort());
  const chooser = page.waitForEvent('filechooser');
  await page.getByRole('button', {name: 'Upload files'}).click();
  await (await chooser).setFiles(chosenFile);
  await expect.poll(async () => {
    const stored = await page.evaluate(() => JSON.parse(localStorage.getItem('popcornstash.uploads.v1') ?? '[]'));
    uploadId = stored.find((v: {name: string}) => v.name === 'Chosen.Dawn.1080p.WEB.mp4')?.uploadId ?? '';
    return uploadId;
  }).not.toBe('');
  await page.reload();
  await page.unroute(/\/api\/uploads\/[^/]+\?offset=\d+$/);
  // After a reload the form returns to automatic, while the interrupted file keeps Test movies.
  const resume = page.waitForEvent('filechooser');
  await page.getByRole('button', {name: 'Upload files'}).click();
  await (await resume).setFiles(chosenFile);
  await expectFile('Test movies/Chosen Dawn/Chosen Dawn.mp4');
  const downloads = await (await page.request.get('/api/downloads?state=all')).json();
  expect(downloads.items.find((d: {name: string}) => d.name === 'Chosen.Dawn.1080p.WEB.mp4')).toMatchObject({library_id: library.id});
  // A fresh upload follows the default movie claim.
  const auto = page.waitForEvent('filechooser');
  await page.getByRole('button', {name: 'Upload files'}).click();
  await (await auto).setFiles({name: 'Automatic.Dawn.2021.mp4', mimeType: 'video/mp4', buffer: bytes});
  await expectFile('Movies/Automatic Dawn (2021)/Automatic Dawn (2021).mp4');
  await picker.click();
  await page.getByRole('option', {name: 'Test movies', exact: true}).click();
  await page.screenshot({path: path.join(env.files, 'destination-desktop.png')});
  await page.setViewportSize({width: 390, height: 844});
  await expectNotClipped(picker);
  await page.screenshot({path: path.join(env.files, 'destination-phone.png')});
});
