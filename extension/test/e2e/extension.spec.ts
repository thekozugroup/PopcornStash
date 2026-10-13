/**
 * Loads the built, unpacked extension in Chromium against a mock PopcornStash
 * server: pair from the options page, send a link from the popup, take over a
 * browser download, recover from a revoked token, sign out on Disconnect,
 * finish pairing in a second tab, leave refused, failed, browser-only and
 * PopcornStash's own downloads in Chrome, and audit every view's
 * accessibility in light and dark. Saves screenshots of the popup and the
 * options page in light and dark to evidence/extension/.
 *
 * Chrome cannot show permission prompts to an automated test, so the test
 * copy of the manifest pre-grants what the person would allow in those
 * prompts (access to 127.0.0.1 and the downloads permission). Everything else,
 * including the chrome.permissions.request calls, runs unchanged.
 */

import {test as base, chromium, expect, type BrowserContext, type Page, type Worker} from '@playwright/test';
import {cp, mkdir, readFile, rm, writeFile} from 'node:fs/promises';
import {createRequire} from 'node:module';
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

let axeSource: string | undefined;

/**
 * The accessibility bar for every view, in light and dark: no axe-core
 * violations (WCAG 2.2 A/AA), every control has a 44px hit area, and each
 * switch stands out from the page (and its knob from its track) at 3:1.
 */
async function audit(page: Page, label: string): Promise<void> {
  axeSource ??= await readFile(createRequire(import.meta.url).resolve('axe-core/axe.min.js'), 'utf8');
  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({colorScheme: scheme, reducedMotion: 'reduce'});
    await page.waitForTimeout(100);
    const where = `${label}, ${scheme}`;

    // DevTools evaluation is not bound by the page's script-src 'self'.
    if (!(await page.evaluate(() => 'axe' in window))) await page.evaluate(axeSource);
    const violations = await page.evaluate(async () => {
      type Result = {violations: {id: string; nodes: {target: string[]}[]}[]};
      const axe = (window as unknown as {axe: {run(context: Document, options: object): Promise<Result>}}).axe;
      const result = await axe.run(document, {
        runOnly: {type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa']},
      });
      return result.violations.map(v => `${v.id}: ${v.nodes.map(n => n.target.join(' ')).join(', ')}`);
    });
    expect(violations, where).toEqual([]);

    const small = await page.evaluate(() => {
      const found: string[] = [];
      for (const element of document.querySelectorAll<HTMLElement>('button, input, select, textarea, a[href], [role="switch"]')) {
        if (element.getClientRects().length === 0) continue;
        element.scrollIntoView({block: 'center'});
        const box = element.getBoundingClientRect();
        const x = box.left + box.width / 2;
        const y = box.top + box.height / 2;
        const missed: string[] = [];
        const hits = (px: number, py: number) => {
          const target = document.elementFromPoint(px, py);
          const hit = target !== null && (target === element || element.contains(target));
          if (!hit) missed.push(`(${Math.round(px)},${Math.round(py)}) is ${target ? `${target.tagName.toLowerCase()}.${target.className}` : 'outside'}`);
          return hit;
        };
        // Probe 21px either side of the centre: a 44px hit area with 1px of slack, because
        // hit testing snaps fractional edges to whole pixels. A 42px target fails.
        const tall = hits(x, y - 21) && hits(x, y + 21);
        const wide = box.width >= 44 || (hits(x - 21, y) && hits(x + 21, y));
        if (!tall || !wide) {
          const name = (element.textContent || element.getAttribute('aria-label') || element.id).trim();
          found.push(`${element.tagName.toLowerCase()} "${name}" ${Math.round(box.width)}x${Math.round(box.height)}: ${missed.join('; ')}`);
        }
      }
      return found;
    });
    expect(small, `${where}: controls under 44px`).toEqual([]);

    const faint = await page.evaluate(() => {
      const canvas = document.createElement('canvas');
      canvas.width = canvas.height = 1;
      const paint = canvas.getContext('2d', {willReadFrequently: true});
      if (!paint) return ['no canvas'];
      const luminance = (color: string) => {
        paint.clearRect(0, 0, 1, 1);
        paint.fillStyle = color;
        paint.fillRect(0, 0, 1, 1);
        const [r = 0, g = 0, b = 0] = paint.getImageData(0, 0, 1, 1).data;
        const lin = (v: number) => ((v /= 255) <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
        return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
      };
      const ratio = (a: string, b: string) => {
        const [hi = 0, lo = 0] = [luminance(a), luminance(b)].sort((p, q) => q - p);
        return (hi + 0.05) / (lo + 0.05);
      };
      const pageColor = getComputedStyle(document.body).backgroundColor;
      const found: string[] = [];
      for (const control of document.querySelectorAll<HTMLInputElement>('[role="switch"]')) {
        const track = getComputedStyle(control).backgroundColor;
        const knob = getComputedStyle(control, '::before').backgroundColor;
        const edge = ratio(track, pageColor);
        const thumb = ratio(knob, track);
        if (edge < 3 || thumb < 3) {
          found.push(`#${control.id} ${control.checked ? 'on' : 'off'}: track/page ${edge.toFixed(2)}, knob/track ${thumb.toFixed(2)}`);
        }
      }
      return found;
    });
    expect(faint, `${where}: switch contrast`).toEqual([]);
  }
  await page.emulateMedia({colorScheme: 'light', reducedMotion: 'reduce'});
}

/** Chrome's own record of the first download whose address contains `needle`. */
async function chromeDownload(worker: Worker, needle: string): Promise<{state: string; paused: boolean} | null> {
  return worker.evaluate(async text => {
    const items = await chrome.downloads.search({});
    const item = items.find(entry => entry.url.includes(text) || entry.finalUrl.includes(text));
    return item ? {state: item.state, paused: item.paused} : null;
  }, needle);
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

test('disconnect signs this browser out of PopcornStash', async ({context, extensionId, mock}) => {
  const options = await openOptions(context, extensionId);
  await pair(options, mock);
  await options.getByRole('button', {name: 'Disconnect'}).click();
  await expect(options.getByRole('heading', {name: 'Connect Chrome to PopcornStash'})).toBeVisible();
  await expect(options.getByText(/^Disconnected\. PopcornStash no longer accepts this browser/)).toBeVisible();
  // The Disconnect button is gone; keyboard focus lands on the address field, not the page body.
  await expect(options.getByLabel('Server address')).toBeFocused();

  // The key is revoked on the server, with the key itself and no browser cookies.
  const logout = mock.requests.find(r => r.method === 'POST' && r.path === '/api/auth/logout');
  expect(logout?.headers.authorization).toBe(`Bearer ${TOKEN}`);
  expect(logout?.headers.cookie).toBeUndefined();
  expect(mock.isRevoked()).toBe(true);
  expect(await options.evaluate(async () => (await chrome.storage.local.get('connection')).connection)).toBeUndefined();
});

test('disconnect while the server is unreachable says the key still works there', async ({context, extensionId, mock}) => {
  const options = await openOptions(context, extensionId);
  await pair(options, mock);
  await mock.close();
  await options.getByRole('button', {name: 'Disconnect'}).click();
  await expect(options.getByText(/could not be reached, so the access key still works there/)).toBeVisible({timeout: 20_000});
  await expect(options.getByRole('button', {name: 'Open Settings, Devices'})).toBeVisible();
  expect(await options.evaluate(async () => (await chrome.storage.local.get('connection')).connection)).toBeUndefined();
});

test('the popup and options page meet the accessibility bar in light and dark', async ({context, extensionId, mock}) => {
  const unpaired = await openPopup(context, extensionId);
  await expect(unpaired.getByRole('button', {name: 'Connect to PopcornStash'})).toBeVisible();
  await audit(unpaired, 'popup, not paired');
  await unpaired.close();

  const options = await openOptions(context, extensionId);
  await expect(options.getByRole('heading', {name: 'Connect Chrome to PopcornStash'})).toBeVisible();
  await audit(options, 'options, not paired');

  await options.getByLabel('Server address').fill(mock.origin);
  await options.getByRole('button', {name: 'Connect', exact: true}).click();
  await expect(options.getByText(USER_CODE)).toBeVisible();
  // Screen readers read the code one character at a time, not as a word.
  expect(await options.locator('.pairing').ariaSnapshot()).toContain('K Q 7 M, 2 X W D');
  await audit(options, 'options, pairing');

  mock.approve();
  await expect(options.getByRole('heading', {name: 'Chrome is connected to PopcornStash'})).toBeVisible({timeout: 15_000});
  await audit(options, 'options, connected');

  const popup = await openPopup(context, extensionId);
  await expect(popup.getByText('Night of the Living Dead (1968)')).toBeVisible();
  await popup.getByLabel('Paste a link or magnet').blur();
  await audit(popup, 'popup, paired');

  mock.failList(true);
  await popup.reload();
  await expect(popup.getByRole('button', {name: 'Try again'})).toBeVisible();
  await audit(popup, 'popup, downloads failed to load');
  mock.failList(false);
  await popup.close();

  await options.getByRole('button', {name: 'Disconnect'}).click();
  await expect(options.getByRole('heading', {name: 'Connect Chrome to PopcornStash'})).toBeVisible();
  await audit(options, 'options, disconnected');
});

test('a second options tab shows the connection instead of an expired code', async ({context, extensionId, mock}) => {
  const first = await openOptions(context, extensionId);
  await first.getByLabel('Server address').fill(mock.origin);
  await first.getByRole('button', {name: 'Connect', exact: true}).click();
  await expect(first.getByText(USER_CODE)).toBeVisible();
  const second = await openOptions(context, extensionId);
  await expect(second.getByText(USER_CODE)).toBeVisible();

  mock.approve();
  for (const page of [first, second]) {
    await expect(page.getByRole('heading', {name: 'Chrome is connected to PopcornStash'})).toBeVisible({timeout: 15_000});
  }
  await expect(second.getByText(/expired/)).toHaveCount(0);
});

test('downloads PopcornStash refuses or cannot fetch, browser-only downloads and its own files stay in Chrome', async ({
  context,
  extensionId,
  mock,
  worker,
}) => {
  const options = await openOptions(context, extensionId);
  await pair(options, mock);
  await options.getByLabel('Send all my browser downloads to PopcornStash').check();
  await expect(options.getByLabel('Send all my browser downloads to PopcornStash')).toBeChecked();

  const page = await context.newPage();
  await page.goto(`${mock.siteOrigin}/files/edge.html`);

  // PopcornStash refuses the link: Chrome's paused copy resumes and finishes.
  await page.click('#refused');
  await expect.poll(() => mock.sentDownloads().length, {timeout: 15_000}).toBe(1);
  await expect.poll(() => chromeDownload(worker, 'refused.bin'), {timeout: 20_000}).toEqual({state: 'complete', paused: false});

  // PopcornStash accepts it, but its own fetch fails at once: Chrome's copy resumes too.
  await page.click('#server-fails');
  await expect.poll(() => mock.sentDownloads().length, {timeout: 15_000}).toBe(2);
  await expect
    .poll(() => chromeDownload(worker, 'server-fails.bin'), {timeout: 20_000})
    .toEqual({state: 'complete', paused: false});

  // A data: link and a file saved from PopcornStash itself are never sent.
  await page.click('#data');
  await page.click('#own');
  await expect.poll(() => chromeDownload(worker, 'data:text/plain'), {timeout: 20_000}).toEqual({state: 'complete', paused: false});
  await expect.poll(() => chromeDownload(worker, '/files/own.bin'), {timeout: 20_000}).toEqual({state: 'complete', paused: false});
  expect(mock.sentDownloads()).toHaveLength(2);
});
