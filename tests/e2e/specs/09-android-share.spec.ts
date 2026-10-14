/**
 * The Android share sheet (PWA share target).
 *
 * The manifest asks Android for links and for files. Chrome posts a share as
 * multipart form data to /share; the service worker keeps shared files on the
 * device and opens the share page, where the person checks them and uploads
 * them through Downloads like any other upload. A share without files keeps
 * the link flow. The kept copies are deleted once uploaded or cancelled, and
 * they wait for sign-in when nobody is signed in. On a device that several
 * people use, nothing one person shared carries over to the next one.
 */
import {expect, test, type Browser, type BrowserContext, type Page} from '@playwright/test';
import {env, exists, expectFile, ownerContext} from '../helpers/env';
import {VirtualAuthenticator} from '../helpers/passkeys';
import {
  makeClip,
  makePhoto,
  postShare,
  sortedFromCompleted,
  storedShareKeys,
  submitShareForm,
  withServiceWorker,
} from '../helpers/share';

test.use({storageState: async ({}, use) => use(env.ownerState)});

interface DownloadSummary {
  id: string;
  name: string;
  kind: string;
  state: string;
  source: string;
  item_ids: string[];
}

async function downloadsNamed(page: Page, name: string): Promise<DownloadSummary[]> {
  const body = (await (await page.request.get('/api/downloads?state=all')).json()) as {items: DownloadSummary[]};
  return body.items.filter(download => download.name === name);
}

test('the manifest offers PopcornStash for photos, videos and other files as well as links', async ({request}) => {
  const response = await request.get('/manifest.webmanifest');
  expect(response.status()).toBe(200);
  const manifest = await response.json();
  expect(manifest.share_target).toEqual({
    action: '/share',
    method: 'POST',
    enctype: 'multipart/form-data',
    params: {
      title: 'title',
      text: 'text',
      url: 'url',
      files: [{name: 'files', accept: ['image/*', 'video/*', 'audio/*', 'application/pdf', '*/*']}],
    },
  });
});

test('a photo and a video shared from another app upload through Downloads and are sorted', async ({page}) => {
  const videoName = 'Juniper.Hollow.2015.720p.WEB.mp4';
  const photoName = 'IMG_20240612_101500.jpg';
  const video = makeClip(videoName);
  const photo = makePhoto(photoName);

  await withServiceWorker(page);
  const landed = await postShare(page, {
    fields: {title: 'Holiday'},
    files: [
      {path: video, type: 'video/mp4'},
      {path: photo, type: 'image/jpeg'},
    ],
  });
  // The service worker answered: the server itself only serves the share page on GET.
  expect(landed.redirected, `the share was answered with ${landed.status} at ${landed.url}`).toBe(true);
  const target = new URL(landed.url);
  expect(target.pathname).toBe('/share');
  expect(target.searchParams.get('files')).toMatch(/^[0-9a-f-]{36}$/);
  // Both files (and the note listing them) are kept on the device until the person decides.
  expect(await storedShareKeys(page)).toHaveLength(3);

  await page.goto(landed.url);
  await expect(page.getByRole('heading', {level: 1, name: 'Add shared files'})).toBeVisible();
  const list = page.getByRole('list', {name: 'Shared files'});
  await expect(list.getByRole('listitem')).toHaveCount(2);
  await expect(list.getByRole('listitem').filter({hasText: videoName})).toContainText('Video');
  await expect(list.getByRole('listitem').filter({hasText: photoName})).toContainText('Photo');
  // Nothing is uploaded before the person confirms.
  expect(await downloadsNamed(page, videoName)).toEqual([]);

  await page.getByRole('button', {name: 'Upload to Downloads'}).click();
  await expect(page).toHaveURL(/\/downloads$/);

  // Both pass through Downloads/Completed and are sorted into the library.
  await expectFile('Movies/Juniper Hollow (2015)/Juniper Hollow (2015).mp4', 30_000);
  await expectFile('Photos/2024/06/IMG_20240612_101500.jpg', 30_000);
  await expect.poll(() => sortedFromCompleted(videoName)).toBe(true);
  await expect.poll(() => sortedFromCompleted(photoName)).toBe(true);
  for (const name of [videoName, photoName]) {
    await expect
      .poll(async () => (await downloadsNamed(page, name)).map(download => [download.kind, download.state]))
      .toEqual([['upload', 'sorted']]);
  }

  // The copies kept on the device are deleted once the uploads are done.
  await expect.poll(() => storedShareKeys(page), {timeout: 15_000}).toEqual([]);
  await page.goto(landed.url);
  await expect(page.getByRole('heading', {name: 'Nothing left to upload'})).toBeVisible();
});

test('cancelling a share deletes the kept files and uploads nothing', async ({page}) => {
  const name = 'IMG_20230102_090000.jpg';
  const photo = makePhoto(name);
  await withServiceWorker(page);
  const landed = await postShare(page, {files: [{path: photo, type: 'image/jpeg'}]});
  expect(landed.redirected).toBe(true);
  await page.goto(landed.url);
  await expect(page.getByRole('heading', {level: 1, name: 'Add shared files'})).toBeVisible();
  await expect(page.getByRole('list', {name: 'Shared files'}).getByRole('listitem')).toHaveCount(1);

  await page.getByRole('button', {name: 'Cancel'}).click();
  await expect(page).toHaveURL(/\/$/);
  await expect.poll(() => storedShareKeys(page)).toEqual([]);
  expect(await downloadsNamed(page, name)).toEqual([]);
});

test('files shared while signed out wait for sign-in, then upload', async ({browser}) => {
  const name = 'Marble.Stairs.2012.720p.WEB.mp4';
  const clip = makeClip(name, 3, 90);
  // An explicitly empty state: new contexts would otherwise inherit this file's signed-in state.
  const context = await browser.newContext({storageState: {cookies: [], origins: []}});
  const page = await context.newPage();
  const authenticator = await VirtualAuthenticator.attach(page);
  await authenticator.load(env.ownerCredential);

  await withServiceWorker(page, '/login');
  const landed = await postShare(page, {files: [{path: clip, type: 'video/mp4'}]});
  expect(landed.redirected).toBe(true);
  await page.goto(landed.url);
  await expect(page).toHaveURL(/\/login\?next=%2Fshare%3Ffiles%3D/);
  // Nothing is lost while the person signs in.
  expect(await storedShareKeys(page)).toHaveLength(2);

  await page.getByRole('button', {name: 'Sign in with a passkey'}).click();
  await authenticator.save(env.ownerCredential);
  await expect(page).toHaveURL(/\/share\?files=/);
  await expect(page.getByRole('list', {name: 'Shared files'}).getByRole('listitem')).toHaveCount(1);
  await page.getByRole('button', {name: 'Upload to Downloads'}).click();
  await expect(page).toHaveURL(/\/downloads$/);
  await expectFile('Movies/Marble Stairs (2012)/Marble Stairs (2012).mp4', 30_000);
  await expect.poll(() => storedShareKeys(page), {timeout: 15_000}).toEqual([]);
  await context.close();
});

test('a shared upload cut off by a reload continues from the copy kept on the device', async ({page}) => {
  const name = 'Lamp.Market.2010.720p.WEB.mp4';
  const clip = makeClip(name, 3, 320);
  // Hold each chunk long enough to reload the page while it is on its way.
  await page.route(/\/api\/uploads\/[^/]+\?offset=\d+$/, async route => {
    await new Promise(resolve => setTimeout(resolve, 2500));
    await route.continue().catch(() => undefined);
  });
  // Leaving while files send asks first; the person chooses to leave.
  page.on('dialog', dialog => void dialog.accept());

  await withServiceWorker(page);
  const landed = await postShare(page, {files: [{path: clip, type: 'video/mp4'}]});
  await page.goto(landed.url);
  await page.getByRole('button', {name: 'Upload to Downloads'}).click();
  await expect(page).toHaveURL(/\/downloads$/);
  await expect(page.getByRole('progressbar', {name: `${name} progress`})).toBeVisible();
  await expect(page.getByText(/^Upload · Uploading/).first()).toBeVisible();

  await page.reload();
  // The upload carries on by itself: nobody has to choose the file again.
  await expect(page.getByRole('progressbar', {name: `${name} progress`})).toBeVisible();
  await expectFile('Movies/Lamp Market (2010)/Lamp Market (2010).mp4', 40_000);
  await expect.poll(async () => (await downloadsNamed(page, name)).map(download => download.state)).toEqual(['sorted']);
  await expect.poll(() => storedShareKeys(page), {timeout: 15_000}).toEqual([]);
  await page.unroute(/\/api\/uploads\//);
});

test('a link shared from another app still opens the link flow', async ({page, baseURL}) => {
  const shared = 'https://203.0.113.10/e2e/shared-link.mp4';
  await withServiceWorker(page);
  await submitShareForm(page, {title: 'Beach day', text: `Look at this ${shared}`, url: ''});
  await expect(page).toHaveURL(/\/share\?title=Beach/);
  await expect(page.getByRole('heading', {level: 1, name: 'Add shared link'})).toBeVisible();
  await expect(page.getByText(shared)).toBeVisible();
  // Nothing is kept on the device for a link.
  expect(await storedShareKeys(page)).toEqual([]);

  await page.getByRole('button', {name: 'Add to Downloads'}).click();
  await expect(page.getByRole('heading', {level: 1, name: 'Added to Downloads'})).toBeVisible();
  const added = (await page.request.get('/api/downloads?state=all').then(response => response.json())).items.find(
    (download: DownloadSummary) => download.source === shared,
  ) as DownloadSummary;
  expect(added).toMatchObject({kind: 'http', name: 'shared-link.mp4'});
  // 203.0.113.0/24 never answers; remove the download again.
  expect((await page.request.delete(`/api/downloads/${added.id}`, {headers: {Origin: baseURL ?? ''}})).status()).toBe(204);
});

test('Settings and the share page explain how to share from an Android phone', async ({page}) => {
  await page.goto('/settings/devices');
  // The Android app leads: what it does, where to get it, and how it signs in or pairs here.
  const app = page.getByRole('region', {name: 'Android app'});
  await expect(app).toContainText('links, photos and videos');
  await expect(app).toContainText('not in a store');
  await expect(app.getByRole('link', {name: /How to build and install it/})).toHaveAttribute(
    'href',
    /docs\/android\.md$/,
  );
  // Pairing works on every server, so it comes first; the passkey only once the server is set up for it.
  const steps = app.getByRole('list', {name: 'How to connect the Android app'}).getByRole('listitem');
  await expect(steps.last()).toContainText(/^Choose Pair with a code/);
  await expect(steps.last()).toContainText('Once you set the server up for it, you can sign in with your passkey');
  await expect(steps.last().getByRole('link', {name: /How to set it up/})).toHaveAttribute(
    'href',
    /docs\/android\.md#sign-in-with-a-passkey$/,
  );
  // Without the app, installing the web app from Chrome still adds PopcornStash to the share sheet.
  const web = page.getByRole('region', {name: 'Share without the app'});
  await expect(web).toContainText('Install app');
  await expect(web).toContainText('Add to Home screen');
  await expect(web).toContainText('Share, then PopcornStash');
  // In that order: the app, the Chrome extension, then the way without the app.
  const top = async (name: string) => (await page.getByRole('region', {name}).boundingBox())?.y ?? -1;
  const appTop = await top('Android app');
  const extensionTop = await top('Chrome extension');
  expect(appTop).toBeGreaterThan(0);
  expect(appTop).toBeLessThan(extensionTop);
  expect(extensionTop).toBeLessThan(await top('Share without the app'));
  // Every group heading sits on one line in the intro column.
  for (const heading of await page.locator('section[aria-labelledby] h3').all()) {
    const box = await heading.boundingBox();
    const lineHeight = await heading.evaluate(element => parseFloat(getComputedStyle(element).lineHeight));
    expect(box?.height ?? 0, (await heading.textContent()) ?? '').toBeLessThan(lineHeight * 1.5);
  }

  // Opened with nothing shared, the share page says the same.
  await page.goto('/share');
  const note = page.getByRole('region', {name: 'On Android'});
  await expect(note).toContainText('Android app');
  await expect(note).toContainText('Install app');
  await expect(note).toContainText('links, photos and videos');
});

const UPLOADS_KEY = 'popcornstash.uploads.v1';

/** Interrupted uploads the web app remembers on this device (web/src/components/uploads/uploadQueue.ts). */
async function rememberedUploads(page: Page): Promise<{uploadId: string; name: string}[]> {
  const raw = await page.evaluate(key => localStorage.getItem(key), UPLOADS_KEY);
  return raw ? (JSON.parse(raw) as {uploadId: string; name: string}[]) : [];
}

/** An invite link for a new family member, made by the owner on the owner's own device. */
async function memberInvite(browser: Browser): Promise<string> {
  const owner = await ownerContext(browser);
  const page = await owner.newPage();
  await page.goto('/settings/people');
  await page.getByRole('button', {name: 'Invite someone'}).click();
  const dialog = page.getByRole('dialog');
  await dialog.getByRole('button', {name: 'Create invite link'}).click();
  await expect(dialog.getByText('Send this link')).toBeVisible();
  const link = (await dialog.locator('output').textContent())?.trim() ?? '';
  expect(link).toMatch(/\/invite\//);
  await owner.close();
  return link;
}

/**
 * One family tablet: the owner signs in on it, shares a clip from another app
 * and starts its upload, which is held mid-way. Resolves with the upload's id.
 */
async function ownerSharesOnTablet(
  browser: Browser,
  name: string,
  hue: number,
): Promise<{device: BrowserContext; page: Page; uploadId: string}> {
  const clip = makeClip(name, 3, hue);
  const device = await browser.newContext({storageState: {cookies: [], origins: []}});
  const page = await device.newPage();
  const authenticator = await VirtualAuthenticator.attach(page);
  await authenticator.load(env.ownerCredential);
  await withServiceWorker(page, '/login');
  await page.getByRole('button', {name: 'Sign in with a passkey'}).click();
  await expect(page).toHaveURL(/\/$/);
  await authenticator.save(env.ownerCredential);

  await page.route(/\/api\/uploads\/[^/]+\?offset=\d+$/, async route => {
    await new Promise(resolve => setTimeout(resolve, 2500));
    await route.continue().catch(() => undefined);
  });
  page.on('dialog', dialog => void dialog.accept());
  const landed = await postShare(page, {files: [{path: clip, type: 'video/mp4'}]});
  await page.goto(landed.url);
  await page.getByRole('button', {name: 'Upload to Downloads'}).click();
  await expect(page).toHaveURL(/\/downloads$/);
  await expect(page.getByRole('progressbar', {name: `${name} progress`})).toBeVisible();
  await expect.poll(async () => (await rememberedUploads(page)).map(record => record.name)).toContain(name);
  const uploadId = (await rememberedUploads(page)).find(record => record.name === name)?.uploadId ?? '';
  expect(await storedShareKeys(page)).toHaveLength(2);
  return {device, page, uploadId};
}

/** A family member joins on the tablet with the invite, then opens Downloads. */
async function memberJoinsAndOpensDownloads(page: Page, link: string, displayName: string): Promise<void> {
  await page.goto(link);
  await page.getByLabel('Your name').fill(displayName);
  await page.getByRole('button', {name: 'Create passkey and join'}).click();
  await expect(page).toHaveURL(/\/$/);
  expect(await (await page.request.get('/api/me')).json()).toMatchObject({role: 'member'});
  await page.goto('/downloads');
  await expect(page.getByRole('heading', {level: 1, name: 'Downloads'})).toBeVisible();
}

/** Gives a wrongly continued upload time to land, then checks that nothing did. */
async function expectNothingUploaded(page: Page, name: string, sorted: string): Promise<void> {
  await page.waitForTimeout(8_000);
  await expect(page.getByRole('progressbar', {name: `${name} progress`})).toHaveCount(0);
  expect(await downloadsNamed(page, name)).toEqual([]);
  expect(exists(sorted)).toBe(false);
}

test('signing out stops shared uploads and deletes the kept files, so the next person gets none of them', async ({
  browser,
  baseURL,
}) => {
  const link = await memberInvite(browser);
  const name = 'Quiet.Orchard.2009.720p.WEB.mp4';
  const {device, page, uploadId} = await ownerSharesOnTablet(browser, name, 140);

  await page.getByRole('button', {name: 'Sign out'}).click();
  await expect(page).toHaveURL(/\/login/);
  // Nothing of the owner's is left on the device, and the partial upload is gone from the server.
  await expect.poll(() => storedShareKeys(page)).toEqual([]);
  expect(await rememberedUploads(page)).toEqual([]);
  await page.unroute(/\/api\/uploads\//);
  const owner = await ownerContext(browser);
  const status = await owner.request.get(`/api/uploads/${uploadId}`, {headers: {Origin: baseURL ?? ''}});
  expect(await status.json()).toMatchObject({state: 'cancelled'});
  await owner.close();

  await memberJoinsAndOpensDownloads(page, link, 'Alex Example');
  await expectNothingUploaded(page, name, 'Movies/Quiet Orchard (2009)/Quiet Orchard (2009).mp4');
  await device.close();
});

test('an upload cut off when the session ends never continues under the next person on the device', async ({browser, baseURL}) => {
  const link = await memberInvite(browser);
  const name = 'Paper.Lantern.2013.720p.WEB.mp4';
  const {device, page} = await ownerSharesOnTablet(browser, name, 260);

  // The session ends without the Sign out button (it expired, or was ended elsewhere).
  expect((await page.request.post('/api/auth/logout', {data: {}, headers: {Origin: baseURL ?? ''}})).status()).toBe(204);
  await page.unroute(/\/api\/uploads\//);
  await page.goto('/login');
  await expect(page.getByRole('button', {name: 'Sign in with a passkey'})).toBeVisible();

  // Someone else signs in on the same device: the owner's upload does not continue as theirs.
  await memberJoinsAndOpensDownloads(page, link, 'Jo Example');
  await expectNothingUploaded(page, name, 'Movies/Paper Lantern (2013)/Paper Lantern (2013).mp4');
  // It stays the owner's: still remembered on the device for them, and still kept.
  expect((await rememberedUploads(page)).map(record => record.name)).toContain(name);
  expect(await storedShareKeys(page)).toHaveLength(2);
  await device.close();
});
