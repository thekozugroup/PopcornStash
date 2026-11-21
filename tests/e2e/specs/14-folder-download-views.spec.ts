/** Folder navigation and large download lists, with generated collections and transfers. */
import {expect, test} from '@playwright/test';
import {env, LIBRARY} from '../helpers/env';

test.use({storageState: async ({}, use) => use(env.ownerState)});

test('folder options show the hierarchy and bulk selection on demand', async ({page}) => {
  const folder = {key: 'download:example', name: 'Example collection', item_count: 1, size: 2048, added_at: 1, custom: false};
  const base = `/api/libraries/${LIBRARY.movies}/download-folders`;
  const queriedFolders: (string | null)[] = [];
  let physicalWrites = 0;
  await page.route(url => url.pathname.startsWith(base), route => {
    const pathname = new URL(route.request().url()).pathname;
    if (pathname === base) return route.fulfill({json: [folder]});
    if (pathname.endsWith('/contents')) return route.fulfill({json: {
      folder: '', folders: [{name: 'Clips', folder: 'Clips', item_count: 1, size: 2048}],
    }});
    if (pathname.endsWith('/flatten-preview')) return route.fulfill({json: {
      key: folder.key, folder: 'Movies/Example', files: [], blocked: [], token: 'synthetic-preview',
    }});
    physicalWrites++;
    return route.abort();
  });
  await page.route(url => url.pathname === '/api/items' && url.searchParams.has('download_group'), route => {
    queriedFolders.push(new URL(route.request().url()).searchParams.get('download_folder'));
    return route.fulfill({json: {items: [{
      id: 'example-video', kind: 'video', title: 'Example video', library_id: LIBRARY.movies, status: 'ready',
      rel_path: 'Movies/Example/Clips/video.mp4', size: 2048, added_at: 1, favorite: false,
    }], next_cursor: null}});
  });

  await page.goto(`/libraries/${LIBRARY.movies}?view=folders`);
  await expect(page.getByRole('searchbox', {name: 'Find a download folder'}).or(page.getByLabel('Find a download folder'))).toBeVisible();
  await page.getByText(folder.name, {exact: true}).click();
  await expect(page.getByText('Example video', {exact: true})).toBeVisible();
  expect(queriedFolders.at(-1)).toBeNull();
  await expect(page.getByRole('button', {name: 'Download folders', exact: true})).toHaveCount(0);
  await expect(page.getByText('Select videos', {exact: true})).not.toBeVisible();
  await expect(page.getByText('Clips', {exact: true})).toHaveCount(0);

  const options = page.getByRole('button', {name: 'Library options'});
  await options.click();
  await page.getByText('Show subfolders', {exact: true}).click();
  await page.getByText('Clips', {exact: true}).click();
  await expect.poll(() => queriedFolders.at(-1)).toBe('Clips');
  await options.click();
  await page.getByText('Collapse subfolders', {exact: true}).click();
  await expect.poll(() => queriedFolders.at(-1)).toBeNull();

  await options.click();
  await page.getByText('Select videos', {exact: true}).click();
  await page.getByRole('checkbox', {name: 'Select Example video'}).check();
  await expect(page.getByText('1 selected')).toBeVisible();
  await options.click();
  await page.getByText('Done selecting', {exact: true}).click();
  await expect(page.getByRole('checkbox', {name: 'Select Example video'})).toHaveCount(0);

  await options.click();
  await page.getByText('Flatten files…', {exact: true}).click();
  await expect(page.getByText('Files are already in this folder.')).toBeVisible();
  await page.getByRole('button', {name: 'Cancel', exact: true}).click();
  await page.getByRole('button', {name: 'Back to folders'}).click();
  await expect(page.getByLabel('Find a download folder')).toBeVisible();
  expect(physicalWrites).toBe(0);
});

test('grouping keeps both status sections and rapid file scrolling stays within the picker', async ({page}) => {
  await page.setViewportSize({width: 390, height: 760});
  const torrent = {
    id: 'fixture-torrent', name: 'Example torrent', kind: 'torrent', source: 'magnet:?xt=urn:btih:0000000000000000000000000000000000000000',
    library_id: LIBRARY.movies, state: 'paused', total_bytes: 300_000_000, done_bytes: 0, down_bps: 0,
    up_bps: 0, progress: 0, peers: 0, eta_s: null, error: 'Ready to choose files.', item_ids: [], destinations: [],
    created_at: 1, completed_at: null,
  };
  const finished = {...torrent, id: 'fixture-finished', name: 'Example history', state: 'sorted', error: null, completed_at: 2, library_id: null};
  const files = Array.from({length: 500}, (_, index) => ({
    index, path: `Example clip ${index.toString().padStart(3, '0')}.mp4`, size: 600_000, kind: 'video', selected: true,
  }));
  let loads = 0;
  await page.route(url => url.pathname === '/api/downloads', route => route.fulfill({json: {items: [torrent, finished]}}));
  await page.route('**/api/downloads/fixture-torrent/files', route => {
    loads++;
    return route.fulfill({json: {ready: true, review_required: true, files}});
  });
  await page.goto('/downloads');
  const group = page.getByRole('button', {name: 'Group by library'});
  await group.click();
  await expect(group).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('list', {name: 'Movies', exact: true})).toContainText(torrent.name);
  await expect(page.getByRole('list', {name: 'Automatic sorting'})).toContainText(finished.name);
  await group.click();
  await expect(page.getByRole('list', {name: 'In progress'})).toContainText(torrent.name);

  await page.getByRole('button', {name: `Review files for ${torrent.name}`}).click();
  const dialog = page.getByRole('dialog', {name: 'Review torrent'});
  const scroll = dialog.getByRole('region', {name: 'Torrent file review'});
  await expect(dialog.getByRole('button', {name: 'Start download'})).toBeVisible();
  const before = await page.evaluate(() => window.scrollY);
  await scroll.hover();
  for (let i = 0; i < 4; i++) await page.mouse.wheel(0, 3600);
  await expect.poll(() => scroll.evaluate(node => node.scrollHeight - node.clientHeight - node.scrollTop)).toBeLessThan(2);
  const last = dialog.getByRole('checkbox', {name: files[99].path});
  await expect(last).toBeVisible();
  await last.uncheck();
  await expect(last).not.toBeChecked();
  await expect.poll(() => loads).toBeGreaterThan(1);
  await expect(last).not.toBeChecked();
  await expect(dialog.getByRole('button', {name: 'Start download'})).toBeInViewport();
  expect(await page.evaluate(() => window.scrollY)).toBe(before);
});
