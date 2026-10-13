/**
 * Web links and torrents.
 *
 * The link guard refuses local and private addresses by design (no fetching
 * from the server's own network), so a link to a test server on 127.0.0.1 is
 * refused with a plain message and never fetched. Without the supervisor the
 * server has no torrent engine: magnets are refused in plain words and the
 * VPN settings say torrents are off.
 */
import http from 'node:http';
import type {AddressInfo} from 'node:net';
import {expect, test} from '@playwright/test';
import {env} from '../helpers/env';
import {VirtualAuthenticator} from '../helpers/passkeys';

test.use({storageState: async ({}, use) => use(env.ownerState)});

const REFUSED = 'Links to local or private network addresses cannot be downloaded.';
const TORRENTS_OFF = 'Torrents are turned off on this server.';

test('a link to a private address is refused plainly and never fetched', async ({page}) => {
  const hits: string[] = [];
  const server = http.createServer((request, response) => {
    hits.push(request.url ?? '');
    response.writeHead(200, {'Content-Type': 'video/mp4', 'Content-Length': '4'});
    response.end('test');
  });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const {port} = server.address() as AddressInfo;
  try {
    // The test server really answers, so a refusal is the guard's doing.
    expect((await fetch(`http://127.0.0.1:${port}/clip.mp4`)).status).toBe(200);
    hits.length = 0;

    await page.goto('/downloads');
    const field = page.getByRole('textbox', {name: 'Paste a link or magnet'});
    await field.fill(`http://127.0.0.1:${port}/clip.mp4`);
    await page.getByRole('button', {name: 'Add', exact: true}).click();
    await expect(page.getByRole('alert').filter({hasText: REFUSED})).toBeVisible();
    // The link stays in the field so it can be corrected.
    await expect(field).toHaveValue(`http://127.0.0.1:${port}/clip.mp4`);

    // Names that resolve to private addresses are refused the same way.
    for (const url of [`http://localhost:${port}/clip.mp4`, `http://[::1]:${port}/clip.mp4`, 'http://192.168.1.20/clip.mp4']) {
      const response = await page.request.post('/api/downloads', {data: {url}, headers: {Origin: new URL(page.url()).origin}});
      expect(response.status(), url).toBe(400);
      expect((await response.json()).error).toMatchObject({code: 'bad_request', message: REFUSED});
    }

    expect(hits, 'the server never fetched the private link').toEqual([]);
    const downloads = await (await page.request.get('/api/downloads?state=all')).json();
    expect(downloads.items.filter((d: {kind: string}) => d.kind === 'http')).toEqual([]);
  } finally {
    server.close();
  }
});

test('a magnet link is refused in plain words while torrents are off', async ({page}) => {
  await page.goto('/downloads');
  // The page says up front that torrents are off, what still works, and where to look,
  // without the server's bare fallback repeating the tag.
  const panel = page.getByRole('region', {name: 'Add to downloads'});
  await expect(panel.getByText('Torrents off', {exact: true})).toBeVisible();
  await expect(panel.getByText('Web links and uploads still work.', {exact: true})).toBeVisible();
  await expect(panel.getByText(TORRENTS_OFF)).toHaveCount(0);
  await expect(panel.getByRole('link', {name: 'Open VPN settings'})).toBeVisible();

  const magnet = 'magnet:?xt=urn:btih:c9e15763f722f23e98a29decdfae341b98d53056&dn=Open.Sample.2020.1080p';
  await page.getByRole('textbox', {name: 'Paste a link or magnet'}).fill(magnet);
  await page.getByRole('button', {name: 'Add', exact: true}).click();
  await expect(page.getByRole('alert').filter({hasText: TORRENTS_OFF})).toBeVisible();

  const response = await page.request.post('/api/downloads', {data: {url: magnet}, headers: {Origin: new URL(page.url()).origin}});
  expect(response.status()).toBe(503);
  expect((await response.json()).error).toMatchObject({code: 'vpn_unavailable', message: TORRENTS_OFF});
  const downloads = await (await page.request.get('/api/downloads?state=all')).json();
  expect(downloads.items.filter((d: {kind: string}) => d.kind === 'torrent')).toEqual([]);
});

test('Settings say plainly that torrents are off', async ({page}) => {
  const status = await (await page.request.get('/api/vpn/status')).json();
  // Off by design, flagged as such (the UI does not guess from the wording of `error`).
  expect(status).toMatchObject({reachable: false, configured: false, torrents_off: true, error: TORRENTS_OFF});

  await page.goto('/settings/vpn');
  // The section keeps its h2 for the outline; the selected tab shows the name on screen.
  await expect(page.getByRole('heading', {level: 2, name: 'VPN'})).toBeAttached();
  await expect(page.getByRole('navigation', {name: 'Tabs'}).getByRole('link', {name: 'VPN'})).toHaveAttribute('aria-current', 'true');
  const vpn = page.getByRole('region', {name: 'VPN status'});
  await expect(vpn.getByText('Torrents are off on this server')).toBeVisible();
  // What still works. The bare fallback reason would only repeat the title, so it is left out.
  await expect(vpn.getByText('Web links and uploads still work.', {exact: true})).toBeVisible();
  await expect(vpn.getByText(TORRENTS_OFF)).toHaveCount(0);
  // Nothing promises a container fix that may not apply.
  await expect(page.getByText(/NET_ADMIN/)).toHaveCount(0);
  // The sidebar tag says it too, in words, not only a colour.
  await expect(page.getByRole('status', {name: TORRENTS_OFF})).toHaveText('Torrents off');

  // System health says the engine is off on purpose, not that something crashed.
  await page.goto('/settings/system');
  await expect(page.getByText('Off on this server', {exact: true})).toBeVisible();
});

test('a shared magnet says plainly that it cannot be added while torrents are off', async ({page}) => {
  const magnet = 'magnet:?xt=urn:btih:c9e15763f722f23e98a29decdfae341b98d53056&dn=Holiday';
  await page.goto(`/share?text=${encodeURIComponent(magnet)}`);
  await expect(page.getByRole('heading', {level: 1, name: 'Add shared link'})).toBeVisible();
  await expect(page.getByText('A torrent. Torrents are off on this server, so it cannot be added.')).toBeVisible();
  // Not "it waits until the VPN is ready": nothing waits while torrents are off.
  await expect(page.getByText(/waits until the VPN/)).toHaveCount(0);
  await expect(page.getByRole('button', {name: 'Add to Downloads'})).toBeDisabled();

  // The Downloads field says the same before anything is sent.
  await page.goto('/downloads');
  await page.getByRole('textbox', {name: 'Paste a link or magnet'}).fill(magnet);
  const panel = page.getByRole('region', {name: 'Add to downloads'});
  await expect(panel.getByText('A torrent. Torrents are off on this server, so it cannot be added.')).toBeVisible();
  await expect(panel.getByText(/waits until the VPN/)).toHaveCount(0);
});

test('a link shared from another app (PWA share target) is added after signing in', async ({browser, baseURL}) => {
  // Shared while signed out: sign-in comes first, then the shared link is still there.
  // (An explicitly empty state: new contexts would otherwise inherit this file's signed-in state.)
  const context = await browser.newContext({storageState: {cookies: [], origins: []}});
  const page = await context.newPage();
  const authenticator = await VirtualAuthenticator.attach(page);
  await authenticator.load(env.ownerCredential);
  const shared = 'https://203.0.113.10/e2e/shared-clip.mp4';
  await page.goto(`/share?title=${encodeURIComponent('Beach day')}&text=${encodeURIComponent(`Look at this ${shared}`)}`);
  await expect(page).toHaveURL(/\/login\?next=%2Fshare%3F/);
  await page.getByRole('button', {name: 'Sign in with a passkey'}).click();
  await authenticator.save(env.ownerCredential);

  await expect(page).toHaveURL(/\/share\?/);
  await expect(page.getByRole('heading', {level: 1, name: 'Add shared link'})).toBeVisible();
  await expect(page.getByText(shared)).toBeVisible();
  await page.getByRole('button', {name: 'Add to Downloads'}).click();
  await expect(page.getByRole('heading', {level: 1, name: 'Added to Downloads'})).toBeVisible();

  const downloads = await (await page.request.get('/api/downloads?state=all')).json();
  const added = downloads.items.find((d: {source: string}) => d.source === shared);
  expect(added).toMatchObject({kind: 'http', name: 'shared-clip.mp4'});
  // 203.0.113.0/24 never answers; remove the download again.
  expect((await page.request.delete(`/api/downloads/${added.id}`, {headers: {Origin: baseURL ?? ''}})).status()).toBe(204);
  await context.close();
});
