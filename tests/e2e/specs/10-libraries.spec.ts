/**
 * Libraries the owner defines: making one in Settings (name, icon, type,
 * claims, folder, who sees it) and seeing it in the sidebar; a kind it claims
 * sorted into it; a member who may not see it learning nothing about it, even
 * by address, until the owner shares it; a kind no library takes waiting in
 * Inbox and accepted into the library picked there; and deleting a library,
 * which keeps every file where it is.
 *
 * The tests build on each other (one server, run in order).
 */
import {execFileSync} from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import {expect, test, type BrowserContext, type Page} from '@playwright/test';
import {LIBRARY, env, exists, expectFile, joinAsMember, libraryPath, ownerContext, showPath, stashPath} from '../helpers/env';

test.use({storageState: async ({}, use) => use(env.ownerState)});
test.describe.configure({mode: 'serial'});

interface LibraryJson {
  id: string;
  name: string;
  icon: string;
  claims: string[];
  visibility: string | null;
  member_ids: string[] | null;
  item_count: number;
}

/** A short test clip, back-dated so the sorter treats it as finished copying. */
function makeClip(name: string, hue: number): string {
  const file = path.join(env.files, name);
  execFileSync('ffmpeg', [
    '-nostdin', '-hide_banner', '-loglevel', 'error', '-y',
    '-f', 'lavfi', '-i', `testsrc2=size=320x180:rate=24:duration=3,hue=h=${hue}`,
    '-f', 'lavfi', '-i', 'sine=frequency=520:sample_rate=48000:duration=3',
    '-map', '0:v', '-map', '1:a', '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '30', '-pix_fmt', 'yuv420p',
    '-c:a', 'aac', '-b:a', '64k', '-shortest', file,
  ]);
  return file;
}

/** Copies a file into Inbox as if it had finished copying an hour ago, and asks for a scan. */
async function dropInInbox(page: Page, file: string) {
  const target = stashPath(`Inbox/${path.basename(file)}`);
  fs.copyFileSync(file, target);
  const old = new Date(Date.now() - 3_600_000);
  fs.utimesSync(target, old, old);
  const scan = await page.request.post('/api/library/rescan', {headers: {Origin: origin(page)}});
  expect(scan.status()).toBe(202);
}

function origin(page: Page): string {
  return new URL(page.url() === 'about:blank' ? env.publicUrl : page.url()).origin;
}

async function libraries(page: Page): Promise<LibraryJson[]> {
  return ((await (await page.request.get('/api/libraries')).json()) as {libraries: LibraryJson[]}).libraries;
}

async function libraryNamed(page: Page, name: string): Promise<LibraryJson> {
  const found = (await libraries(page)).find(library => library.name === name);
  if (!found) throw new Error(`No library called ${name}`);
  return found;
}

let member: {context: BrowserContext; page: Page; id: string} | null = null;

test.afterAll(async () => {
  await member?.context.close();
});

test('the owner makes a library with its own name, icon and type, and it joins the sidebar', async ({page}) => {
  await page.goto('/settings/libraries');
  const list = page.getByRole('region', {name: 'Libraries'});
  for (const name of ['Movies', 'Shows', 'Home Videos', 'Photos']) {
    await expect(list.getByRole('listitem').filter({hasText: name}).first()).toBeVisible();
  }
  await expect(list.getByRole('listitem').filter({hasText: 'Shows'}).getByText('Takes TV episodes')).toBeVisible();
  // The owner's own folder that is not a library is offered as one.
  const folders = page.getByRole('region', {name: 'Folders not in a library'});
  await expect(folders.getByText('Family 2019/')).toBeVisible();
  await expect(folders.getByRole('link', {name: 'Make a library from Family 2019'})).toBeVisible();

  await page.getByRole('link', {name: 'Make a library', exact: true}).click();
  await expect(page).toHaveURL(/\/settings\/libraries\/new$/);
  await expect(page.getByRole('heading', {name: 'New library'})).toBeVisible();

  // Name and icon, with the sidebar row as it will look.
  await page.getByRole('textbox', {name: /^Name/}).fill('Kids');
  await expect(page.getByRole('textbox', {name: 'Folder name'})).toHaveValue('Kids');
  await page.getByRole('button', {name: 'Baby', exact: true}).click();
  await expect(page.getByRole('button', {name: 'Baby', exact: true})).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('img', {name: 'Sidebar preview: Kids'}).locator('svg.lucide-baby')).toBeVisible();

  // What it holds: series. Only the kind that fits a series is offered, with where it goes now.
  await expect(page.getByRole('radio', {name: /^Videos/})).toBeChecked();
  await page.getByRole('radio', {name: /^Series/}).click();
  const claims = page.getByRole('group', {name: 'Sort these here'}).or(page.getByRole('list', {name: 'Sort these here'})).first();
  await expect(page.getByRole('checkbox', {name: /^Movies/})).toHaveCount(0);
  await expect(claims.getByText('Sorted into Shows now')).toBeVisible();
  await page.getByRole('checkbox', {name: /^TV episodes/}).click();
  await expect(claims.getByText('Moves here from Shows')).toBeVisible();

  // Who sees it: only some people, nobody chosen yet, so only owners.
  await page.getByRole('radio', {name: /^Only some people/}).click();
  // The checkbox lists sit in the radios' column (a checkbox row had 8px of extra inset).
  const radioX = (await page.getByRole('radio', {name: /^Only some people/}).boundingBox())?.x ?? NaN;
  for (const checkbox of [page.getByRole('checkbox', {name: /^TV episodes/}), page.getByRole('group', {name: 'People'}).getByRole('checkbox').first()]) {
    if ((await checkbox.count()) === 0) continue;
    expect(Math.abs(((await checkbox.boundingBox())?.x ?? NaN) - radioX)).toBeLessThanOrEqual(1);
  }

  await page.getByRole('button', {name: 'Make library'}).click();
  // Taking TV episodes from Shows is confirmed first.
  const confirm = page.getByRole('alertdialog', {name: 'Sort TV episodes into Kids?'});
  await expect(confirm.getByText('Files already sorted stay where they are.', {exact: false})).toBeVisible();
  await confirm.getByRole('button', {name: 'Move and make library'}).click();

  await expect(page).toHaveURL(/\/settings\/libraries$/);
  const kidsRow = list.getByRole('listitem').filter({hasText: 'Kids'});
  for (const fact of ['Series', 'Takes TV episodes', 'Only owners']) {
    await expect(kidsRow.getByText(fact, {exact: true})).toBeVisible();
  }
  await expect(list.getByRole('listitem').filter({hasText: 'Shows'}).getByText('Takes nothing new')).toBeVisible();
  // A new folder in the stash, made empty; nothing else moved.
  expect(fs.statSync(stashPath('Kids')).isDirectory()).toBe(true);
  expect(exists('Shows/The Lantern Keepers/Season 02/The Lantern Keepers - S02E01.mp4')).toBe(true);

  // In the sidebar with its icon, after the four it joined.
  const nav = page.getByRole('navigation', {name: 'Main navigation'});
  await expect(nav.getByRole('link', {name: 'Kids', exact: true}).locator('svg.lucide-baby')).toBeVisible();
  const names = async () => (await nav.getByRole('link').filter({hasNotText: 'Popcorn'}).allInnerTexts()).map(text => text.trim().split('\n')[0]);
  expect((await names()).slice(0, 7)).toEqual(['Home', 'Movies', 'Shows', 'Home Videos', 'Photos', 'Kids', 'Other files']);

  // Moving it up applies at once, in the sidebar too, and keeps focus on its menu.
  await page.getByRole('button', {name: 'More for Kids'}).click();
  await page.getByRole('menuitem', {name: 'Move up'}).click();
  await expect.poll(async () => (await names()).slice(4, 6)).toEqual(['Kids', 'Photos']);
  await expect(page.getByRole('button', {name: 'More for Kids'})).toBeFocused();

  const kids = await libraryNamed(page, 'Kids');
  expect(kids).toMatchObject({icon: 'baby', claims: ['episode'], visibility: 'selected', member_ids: []});
  expect((await libraryNamed(page, 'Shows')).claims).toEqual([]);
});

test('a kind the library takes is sorted into it, and its page shows it', async ({page}) => {
  test.setTimeout(90_000);
  const kids = await libraryNamed(page, 'Kids');
  await page.goto(libraryPath(kids.id));
  await expect(page.getByRole('heading', {level: 1, name: 'Kids'})).toBeVisible();
  // Empty, with how it fills and the owner's one action.
  await expect(page.getByRole('heading', {name: 'Nothing in Kids yet'})).toBeVisible();
  await expect(page.getByText('New TV episodes land here on their own.')).toBeVisible();
  await expect(page.getByRole('link', {name: 'Edit library'})).toBeVisible();

  await dropInInbox(page, makeClip('Moss.Harbour.S01E02.720p.WEB.mp4', 200));
  await expectFile('Kids/Moss Harbour/Season 01/Moss Harbour - S01E02.mp4', 60_000);
  expect(exists('Inbox/Moss.Harbour.S01E02.720p.WEB.mp4')).toBe(false);

  // The open page follows by itself: the show, pictured, in a series grid.
  const grid = page.getByRole('list', {name: 'Kids'});
  await expect(grid.getByRole('link', {name: /^Moss Harbour/})).toBeVisible({timeout: 20_000});
  await expect(page.getByText('1 show, 1 episode', {exact: true})).toBeVisible();
  await grid.getByRole('link', {name: /^Moss Harbour/}).click();
  await expect(page).toHaveURL(new RegExp(`${showPath(kids.id, 'Moss Harbour')}$`));
  await expect(page.getByRole('main').getByRole('link', {name: 'Kids', exact: true})).toBeVisible();
  // Shows took nothing new: its episodes stay where they were.
  const shows = await libraryNamed(page, 'Shows');
  expect(shows.item_count).toBe(4);

  // Home: the shelf is a second-level heading in the outline, drawn in Section type (16px).
  await page.goto('/');
  const shelfHeading = page.getByRole('heading', {level: 2, name: 'Kids', exact: true});
  await expect(shelfHeading).toBeVisible();
  expect(await shelfHeading.evaluate(element => getComputedStyle(element).fontSize)).toBe('16px');

  // Its editor says what it now holds as a fact anyone can read, not as greyed choices.
  await page.goto(`/settings/libraries/${encodeURIComponent(kids.id)}`);
  const holds = page.getByTestId('holds-fact');
  await expect(holds).toHaveText(/Holds\s*Videos, arranged as series/);
  await expect(page.getByText('The type can change only while the library is empty.')).toBeVisible();
  await expect(page.getByRole('radio', {name: 'Series'})).toHaveCount(0);
  const ratio = await holds.getByText('Videos, arranged as series', {exact: true}).evaluate(element => {
    // Any CSS colour (the theme's are oklch) to linear sRGB through a canvas pixel.
    const linear = (css: string) => {
      const context = document.createElement('canvas').getContext('2d')!;
      context.fillStyle = css;
      context.fillRect(0, 0, 1, 1);
      return [...context.getImageData(0, 0, 1, 1).data.slice(0, 3)].map(value => {
        const c = value / 255;
        return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
      });
    };
    const luminance = (css: string) => {
      const [r = 0, g = 0, b = 0] = linear(css);
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    let node: Element | null = element;
    let background = 'rgba(0, 0, 0, 0)';
    while (node && background === 'rgba(0, 0, 0, 0)') {
      background = getComputedStyle(node).backgroundColor;
      node = node.parentElement;
    }
    const [light, dark] = [luminance(getComputedStyle(element).color), luminance(background)].sort((x, y) => y - x);
    return ((light ?? 1) + 0.05) / ((dark ?? 0) + 0.05);
  });
  expect(ratio).toBeGreaterThanOrEqual(4.5);
});

test('a member without access learns nothing about the library, even by its address', async ({browser}) => {
  test.setTimeout(90_000);
  const owner = await ownerContext(browser);
  const ownerPage = await owner.newPage();
  await ownerPage.goto('/');
  const kids = await libraryNamed(ownerPage, 'Kids');
  const episodes = (await (await ownerPage.request.get(`/api/items?library_id=${kids.id}`)).json()) as {items: {id: string}[]};
  const episode = episodes.items[0]?.id ?? '';
  expect(episode).toBeTruthy();

  member = await joinAsMember(browser, 'Ana Example');
  const page = member.page;
  const nav = page.getByRole('navigation', {name: 'Main navigation'});
  await expect(nav.getByRole('link', {name: 'Movies', exact: true})).toBeVisible();
  await expect(nav.getByRole('link', {name: 'Kids', exact: true})).toHaveCount(0);
  await expect(page.getByRole('region', {name: 'Kids', exact: true})).toHaveCount(0);

  // Every way in looks like a library or file that never existed.
  await page.goto(libraryPath(kids.id));
  await expect(page.getByRole('heading', {level: 1, name: 'This library is not here any more'})).toBeVisible();
  await page.goto(showPath(kids.id, 'Moss Harbour'));
  await expect(page.getByRole('heading', {level: 1, name: 'This library is not here any more'})).toBeVisible();
  await page.goto(`/item/${episode}`);
  await expect(page.getByRole('heading', {level: 1, name: 'This file is not in your library'})).toBeVisible();
  for (const url of [
    `/api/libraries/${kids.id}`,
    `/api/libraries/${kids.id}/shows`,
    `/api/libraries/${kids.id}/shows/Moss%20Harbour`,
    `/api/items?library_id=${kids.id}`,
    `/api/items/${episode}`,
    `/api/items/${episode}/thumb`,
    `/api/items/${episode}/file`,
  ]) {
    const response = await page.request.get(url);
    expect(response.status(), url).toBe(404);
  }
  const search = (await (await page.request.get('/api/items?q=Moss')).json()) as {items: unknown[]};
  expect(search.items).toEqual([]);
  const home = (await (await page.request.get('/api/home')).json()) as {libraries: {library: {id: string}}[]};
  expect(home.libraries.map(entry => entry.library.id)).not.toContain(kids.id);
  expect((await libraries(page)).map(library => library.name)).not.toContain('Kids');
  const progress = await page.request.put(`/api/items/${episode}/progress`, {
    data: {position_s: 1, duration_s: 3},
    headers: {Origin: origin(page)},
  });
  expect(progress.status()).toBe(404);

  // The owner shares it with Ana; her open page follows by itself.
  await page.goto('/');
  await ownerPage.goto(`/settings/libraries/${kids.id}`);
  await expect(ownerPage.getByRole('heading', {name: 'Kids'})).toBeVisible();
  await ownerPage.getByRole('checkbox', {name: 'Ana Example'}).click();
  await ownerPage.getByRole('button', {name: 'Save changes'}).click();
  await expect(ownerPage).toHaveURL(/\/settings\/libraries$/);
  await expect(ownerPage.getByRole('listitem').filter({hasText: 'Kids'}).getByText('Only Ana Example')).toBeVisible();
  await expect(nav.getByRole('link', {name: 'Kids', exact: true})).toBeVisible({timeout: 20_000});
  await page.goto(`/item/${episode}`);
  await expect(page.getByRole('heading', {level: 1})).not.toHaveText('This file is not in your library');
  expect((await page.request.get(`/api/items/${episode}`)).status()).toBe(200);
  await owner.close();
});

test('a kind no library takes waits in Inbox and is accepted into the library picked there', async ({page}) => {
  test.setTimeout(90_000);
  await page.goto('/');
  const headers = {Origin: origin(page)};
  // A one-offs library that takes nothing new, and nobody taking camera videos.
  const created = await page.request.post('/api/libraries', {
    data: {name: 'Holidays', icon: 'tree-palm', content_type: 'video', structure: 'one_offs', claims: [], folder: {mode: 'new', name: 'Holidays'}, visibility: 'everyone', member_ids: []},
    headers,
  });
  expect(created.status(), await created.text()).toBe(201);
  const holidays = (await created.json()) as LibraryJson;
  const unclaim = await page.request.patch(`/api/libraries/${LIBRARY.homeVideos}`, {data: {claims: []}, headers});
  expect(unclaim.status(), await unclaim.text()).toBe(200);

  try {
    const name = 'VID_20240704_101500.mp4';
    await dropInInbox(page, makeClip(name, 90));
    await expect
      .poll(async () => {
        const inbox = (await (await page.request.get('/api/inbox')).json()) as {items: {rel_path: string; suggestion: {waiting: string | null; place: string | null} | null}[]};
        return inbox.items.find(item => item.rel_path === `Inbox/${name}`)?.suggestion ?? null;
      }, {timeout: 60_000})
      .toMatchObject({waiting: 'choose_library', place: null});

    await page.goto('/inbox');
    const row = page.getByRole('article', {name});
    await expect(row.getByText('No library takes camera videos yet.')).toBeVisible();
    const picker = row.getByRole('combobox', {name: 'Library'}).or(row.getByRole('button', {name: /^Library/})).first();
    await expect(picker).toContainText('Choose a library');
    await expect(row.getByRole('button', {name: 'Accept'})).toBeDisabled();
    await picker.click();
    await page.getByRole('option', {name: 'Holidays', exact: true}).click();
    await expect(picker).toContainText('Holidays');
    await row.getByRole('button', {name: 'Accept'}).click();
    await expect(row).toBeHidden();
    await expect(page.getByLabel('Notifications').getByText('Moved to Holidays.')).toBeVisible();

    await expectFile(`Holidays/2024/07/${name}`);
    expect(exists(`Inbox/${name}`)).toBe(false);
    const accepted = (await (await page.request.get(`/api/items?library_id=${holidays.id}`)).json()) as {items: {library_id: string; place: string}[]};
    expect(accepted.items).toHaveLength(1);
    expect(accepted.items[0]).toMatchObject({library_id: holidays.id, place: 'library'});
  } finally {
    const restore = await page.request.patch(`/api/libraries/${LIBRARY.homeVideos}`, {data: {claims: ['home_video']}, headers});
    expect(restore.status(), await restore.text()).toBe(200);
  }
});

test('deleting a library takes it out of the app and keeps every file where it is', async ({page}) => {
  const holidays = await libraryNamed(page, 'Holidays');
  const file = 'Holidays/2024/07/VID_20240704_101500.mp4';
  expect(exists(file)).toBe(true);

  await page.goto('/settings/libraries');
  await page.getByRole('button', {name: 'More for Holidays'}).click();
  await page.getByRole('menuitem', {name: 'Delete…'}).click();
  const confirm = page.getByRole('alertdialog', {name: 'Delete Holidays?'});
  await expect(confirm.getByText('Its files stay in Holidays/ and only owners will see them.', {exact: false})).toBeVisible();
  await confirm.getByRole('button', {name: 'Delete library'}).click();
  await expect(confirm).toBeHidden();

  const list = page.getByRole('region', {name: 'Libraries'});
  await expect(list.getByRole('listitem').filter({hasText: 'Holidays'})).toHaveCount(0);
  await expect(page.getByRole('navigation', {name: 'Main navigation'}).getByRole('link', {name: 'Holidays'})).toHaveCount(0);
  // Its folder is now a folder that is not a library, offered as one again.
  const folders = page.getByRole('region', {name: 'Folders not in a library'});
  await expect(folders.getByText('Holidays/')).toBeVisible();
  await expect(folders.getByRole('link', {name: 'Make a library from Holidays'})).toBeVisible();

  // Every file stays on disk, still indexed, now seen by owners only.
  expect(exists(file)).toBe(true);
  const items = (await (await page.request.get('/api/items?place=folder&limit=200')).json()) as {items: {rel_path: string; library_id: string | null}[]};
  expect(items.items.find(item => item.rel_path === file)).toMatchObject({library_id: null});
  await page.goto(libraryPath(holidays.id));
  await expect(page.getByRole('heading', {level: 1, name: 'This library is not here any more'})).toBeVisible();
});
