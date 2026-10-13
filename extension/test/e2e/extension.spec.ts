/**
 * Loads the built, unpacked extension in Chromium against a mock PopcornStash
 * server: pair from the options page, send a link from the popup, take over a
 * browser download, and recover from a revoked token. Saves screenshots of the
 * popup and the options page in light and dark to evidence/extension/.
 *
 * Chrome cannot show permission prompts to an automated test, so the test
 * copy of the manifest pre-grants what the person would allow in those
 * prompts (access to 127.0.0.1 and the downloads permission). Everything else,
 * including the chrome.permissions.request calls, runs unchanged.
 */

import {test as base, chromium, expect, type BrowserContext, type Page, type Worker} from '@playwright/test';
import {cp, mkdir, readFile, rm, writeFile} from 'node:fs/promises';
import {dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {startMockServer, TOKEN, USER_CODE, type MockServer} from './mock-server';

const here = dirname(fileURLToPath(import.meta.url));
const extensionRoot = join(here, '..', '..');
const evidence = join(extensionRoot, '..', 'evidence', 'extension');

/** Permissions the person would grant in Chrome's prompts, pre-granted for this test run. */
interface Grants {
  allSitesAndCookies?: boolean;
}

async function prepareExtension(name: string, grants: Grants = {}): Promise<string> {
  const target = join(extensionRoot, '.e2e', name);
  await rm(target, {recursive: true, force: true});
  await mkdir(target, {recursive: true});
  await cp(join(extensionRoot, 'dist'), target, {recursive: true});
  const manifestPath = join(target, 'manifest.json');
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8')) as Record<string, unknown>;
  manifest.host_permissions = grants.allSitesAndCookies ? ['<all_urls>'] : ['http://127.0.0.1/*'];
  manifest.permissions = [
    ...(manifest.permissions as string[]),
    'downloads',
    ...(grants.allSitesAndCookies ? ['cookies'] : []),
  ];
  await writeFile(manifestPath, JSON.stringify(manifest, null, 2));
  return target;
}

interface Fixtures {
  mock: MockServer;
  context: BrowserContext;
  worker: Worker;
  extensionId: string;
}

const test = base.extend<Fixtures>({
  mock: async ({}, use) => {
    const mock = await startMockServer();
    await use(mock);
    await mock.close();
  },
  context: async ({}, use, testInfo) => {
    const grants: Grants = {allSitesAndCookies: testInfo.title.includes('cookies')};
    const path = await prepareExtension(`ext-${testInfo.workerIndex}-${testInfo.retry}`, grants);
    const context = await chromium.launchPersistentContext('', {
      channel: 'chromium',
      headless: true,
      args: [`--disable-extensions-except=${path}`, `--load-extension=${path}`],
    });
    await use(context);
    await context.close();
  },
  worker: async ({context}, use) => {
    let [worker] = context.serviceWorkers();
    worker ??= await context.waitForEvent('serviceworker', {timeout: 20_000});
    await use(worker);
  },
  extensionId: async ({context, worker}, use) => {
    const id = new URL(worker.url()).host;
    // On install the extension opens its options page; wait for it so it cannot take over a test tab.
    const optionsUrl = `chrome-extension://${id}/options.html`;
    let welcome: Page | undefined;
    for (let waited = 0; !welcome && waited < 10_000; waited += 100) {
      welcome = context.pages().find(page => page.url() === optionsUrl);
      if (!welcome) await new Promise(resolve => setTimeout(resolve, 100));
    }
    await welcome?.close();
    await use(id);
  },
});

async function openOptions(context: BrowserContext, extensionId: string): Promise<Page> {
  const page = await context.newPage();
  await page.setViewportSize({width: 1100, height: 900});
  await page.goto(`chrome-extension://${extensionId}/options.html`);
  return page;
}

async function openPopup(context: BrowserContext, extensionId: string): Promise<Page> {
  const page = await context.newPage();
  await page.setViewportSize({width: 344, height: 640});
  await page.goto(`chrome-extension://${extensionId}/popup.html`);
  return page;
}

async function pair(page: Page, mock: MockServer): Promise<void> {
  await page.getByLabel('Server address').fill(`${mock.origin}/settings/devices`);
  await page.getByRole('button', {name: 'Connect', exact: true}).click();
  await expect(page.getByText(USER_CODE)).toBeVisible();
  expect(mock.pairingRequested()).toBe(true);
  mock.approve();
  await expect(page.getByRole('heading', {name: 'Chrome is connected to PopcornStash'})).toBeVisible({timeout: 15_000});
}

async function shoot(page: Page, name: string, target: 'page' | 'body' = 'page'): Promise<void> {
  await mkdir(evidence, {recursive: true});
  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({colorScheme: scheme, reducedMotion: 'reduce'});
    await page.waitForTimeout(150);
    const path = join(evidence, `${name}-${scheme}.png`);
    if (target === 'body') await page.locator('body').screenshot({path});
    else await page.screenshot({path, fullPage: true});
  }
  await page.emulateMedia({colorScheme: 'light'});
}

test('pairs from the options page and sends a link from the popup', async ({context, extensionId, mock}) => {
  const unpaired = await openPopup(context, extensionId);
  await expect(unpaired.getByRole('button', {name: 'Connect to PopcornStash'})).toBeVisible();
  await shoot(unpaired, 'popup-not-paired', 'body');
  await unpaired.close();

  const options = await openOptions(context, extensionId);
  await expect(options.getByRole('heading', {name: 'Connect Chrome to PopcornStash'})).toBeVisible();
  await shoot(options, 'options-not-paired');

  // A plain-http LAN address is refused before anything is requested.
  await options.getByLabel('Server address').fill('http://192.168.1.20:8788');
  await options.getByRole('button', {name: 'Connect', exact: true}).click();
  await expect(options.getByText(/Plain http only works for localhost, because/)).toBeVisible();
  expect(mock.requests).toHaveLength(0);

  await options.getByLabel('Server address').fill(`${mock.origin}/settings/devices`);
  await options.getByRole('button', {name: 'Connect', exact: true}).click();
  await expect(options.getByText(USER_CODE)).toBeVisible();
  const pairingRequest = mock.requests.find(r => r.path === '/api/pairing/request');
  expect(pairingRequest?.body).toEqual({client_name: 'Chrome on Linux', client_kind: 'chrome-extension'});
  await shoot(options, 'options-pairing');

  mock.approve();
  await expect(options.getByRole('heading', {name: 'Chrome is connected to PopcornStash'})).toBeVisible({timeout: 15_000});
  await expect(options.getByText(new URL(mock.origin).host, {exact: true})).toBeVisible();
  await shoot(options, 'options-connected');

  // The token lives in chrome.storage.local, never sync.
  const stored = await options.evaluate(async () => ({
    local: await chrome.storage.local.get('connection'),
    sync: await chrome.storage.sync.get(null),
  }));
  expect((stored.local.connection as {token: string}).token).toBe(TOKEN);
  expect(stored.sync).toEqual({});

  const popup = await openPopup(context, extensionId);
  await expect(popup.getByText('Protected · NordVPN · Netherlands')).toBeVisible();
  await expect(popup.getByText('Night of the Living Dead (1968)')).toBeVisible();

  const link = 'https://media.example.org/films/Big%20Buck%20Bunny%20(2008).mp4';
  await popup.getByLabel('Paste a link or magnet').fill(link);
  await popup.getByRole('button', {name: 'Send', exact: true}).click();
  await expect(popup.getByText('Sent: Big Buck Bunny (2008).mp4')).toBeVisible();

  const [sent] = mock.sentDownloads();
  expect(sent).toBeDefined();
  expect(sent?.headers.authorization).toBe(`Bearer ${TOKEN}`);
  expect(sent?.headers.cookie).toBeUndefined();
  expect((sent?.body as {url: string}).url).toBe(link);
  expect((sent?.body as {headers?: {cookie?: string}}).headers?.cookie).toBeUndefined();
  await expect(popup.locator('.download-name').first()).toHaveText('Big Buck Bunny (2008).mp4');
  await popup.getByLabel('Paste a link or magnet').blur();
  // Chrome caps popups at 600px high; five downloads and a message must fit without scrolling.
  expect(await popup.evaluate(() => Math.ceil(document.body.getBoundingClientRect().height))).toBeLessThanOrEqual(600);
  await shoot(popup, 'popup', 'body');

  // A magnet link goes through as-is, with no browser headers.
  const magnet = 'magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Big+Buck+Bunny';
  await popup.getByLabel('Paste a link or magnet').fill(magnet);
  await popup.getByRole('button', {name: 'Send', exact: true}).click();
  await expect(popup.getByText('Sent: Big Buck Bunny', {exact: true})).toBeVisible();
  expect(mock.sentDownloads()[1]?.body).toEqual({url: magnet});

  // Something that only exists in the browser is refused in the popup.
  await popup.getByLabel('Paste a link or magnet').fill('blob:https://media.example.org/1234');
  await popup.getByRole('button', {name: 'Send', exact: true}).click();
  await expect(popup.getByText(/only exists inside this browser tab/)).toBeVisible();
  expect(mock.sentDownloads()).toHaveLength(2);
});

test('hands a browser download to PopcornStash with the site cookies when those options are on', async ({
  context,
  extensionId,
  mock,
  worker,
}) => {
  const options = await openOptions(context, extensionId);
  await pair(options, mock);
  await options.getByLabel('Send all my browser downloads to PopcornStash').check();
  await expect(options.getByLabel('Send all my browser downloads to PopcornStash')).toBeChecked();
  await options.getByLabel("Include this site's sign-in cookies so protected downloads work").check();
  await expect(options.getByLabel("Include this site's sign-in cookies so protected downloads work")).toBeChecked();

  // The download site has a sign-in cookie; the PopcornStash server has its own session cookie.
  const site = new URL(mock.siteOrigin);
  const server = new URL(mock.origin);
  await context.addCookies([
    {name: 'films_session', value: 'signed-in', domain: site.hostname, path: '/'},
    {name: 'stash', value: 'never-forwarded', domain: server.hostname, path: '/'},
  ]);

  const page = await context.newPage();
  await page.goto(`${mock.siteOrigin}/files/page.html`);
  await page.click('#file');

  await expect.poll(() => mock.sentDownloads().length, {timeout: 20_000}).toBe(1);
  const body = mock.sentDownloads()[0]?.body as {url: string; headers?: Record<string, string>};
  expect(body.url).toBe(`${mock.siteOrigin}/files/Big%20Buck%20Bunny.mkv`);
  expect(body.headers?.referer).toBe(`${mock.siteOrigin}/files/page.html`);
  expect(body.headers?.user_agent).toContain('Chrome');
  expect(body.headers?.cookie).toBe('films_session=signed-in');
  // The extension talks to PopcornStash with its token only, never the browser's cookies.
  expect(mock.sentDownloads()[0]?.headers.cookie).toBeUndefined();

  // Chrome's own copy is cancelled and removed from its list once the server has it.
  await expect
    .poll(
      () =>
        worker.evaluate(async () => {
          const items = await chrome.downloads.search({});
          return items.filter(item => item.url.includes('Big%20Buck%20Bunny.mkv') && item.state === 'in_progress').length;
        }),
      {timeout: 20_000},
    )
    .toBe(0);
});

test('a revoked token sends the person back to pairing', async ({context, extensionId, mock}) => {
  const options = await openOptions(context, extensionId);
  await pair(options, mock);
  mock.revoke();

  const popup = await openPopup(context, extensionId);
  await expect(popup.getByRole('heading', {name: 'This browser was disconnected'})).toBeVisible({timeout: 15_000});
  await expect(popup.getByRole('button', {name: 'Pair again'})).toBeVisible();

  await options.reload();
  await expect(options.getByText(/stopped accepting this browser/)).toBeVisible();
  await expect(options.getByLabel('Server address')).toHaveValue(mock.origin);
  const token = await options.evaluate(async () => (await chrome.storage.local.get('connection')).connection);
  expect(token).toBeUndefined();
});
