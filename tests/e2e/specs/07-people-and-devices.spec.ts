/**
 * Sharing: an invite link lets a family member register their own passkey,
 * and members cannot reach owner-only settings. Device pairing: a client
 * (here acting as the Chrome extension) asks for a code, the owner approves it
 * on /pair, and the client's token can add downloads but not read the library.
 */
import {expect, request as playwrightRequest, test} from '@playwright/test';
import {LIBRARY, env, expectNotClipped, libraryPath, ownerContext} from '../helpers/env';
import {VirtualAuthenticator} from '../helpers/passkeys';

const MEMBER_NAME = 'Sam Example';
const EXTENSION_ORIGIN = 'chrome-extension://abcdefghijklmnopabcdefghijklmnop';

test('an invited family member joins with a passkey and cannot open owner settings', async ({browser, baseURL}) => {
  const owner = await ownerContext(browser);
  const ownerPage = await owner.newPage();
  await ownerPage.goto('/settings/people');
  await expect(ownerPage.getByRole('heading', {name: 'People'})).toBeVisible();
  await ownerPage.getByRole('button', {name: 'Invite someone'}).click();
  const dialog = ownerPage.getByRole('dialog');
  await expect(dialog.getByRole('radio', {name: /^Member/})).toBeChecked();
  await dialog.getByRole('button', {name: 'Create invite link'}).click();
  await expect(dialog.getByText('Send this link')).toBeVisible();
  const link = (await dialog.locator('output').textContent())?.trim() ?? '';
  expect(link).toMatch(new RegExp(`^${baseURL}/invite/[A-Za-z0-9_-]+$`));
  await dialog.getByRole('button', {name: 'Done'}).click();

  // The new person opens the link on their own device.
  const member = await browser.newContext();
  const page = await member.newPage();
  const authenticator = await VirtualAuthenticator.attach(page);
  await page.goto(link);
  await expect(page.getByRole('heading', {name: 'Join PopcornStash'})).toBeVisible();
  await expect(page.getByText('You are invited as a member.', {exact: false})).toBeVisible();
  await page.getByLabel('Your name').fill(MEMBER_NAME);
  await page.getByRole('button', {name: 'Create passkey and join'}).click();
  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByRole('heading', {level: 1, name: 'Home'})).toBeVisible();
  expect(await authenticator.credentials()).toHaveLength(1);
  expect(await (await page.request.get('/api/me')).json()).toMatchObject({display_name: MEMBER_NAME, role: 'member'});

  // The owner's People page, still open, shows the new person and drops the used invite by itself.
  await expect(ownerPage.getByRole('listitem').filter({hasText: MEMBER_NAME})).toBeVisible({timeout: 15_000});
  await expect(ownerPage.getByText('No open invites.', {exact: false})).toBeVisible({timeout: 15_000});

  // Members can browse...
  await page.goto(libraryPath(LIBRARY.movies));
  await expect(page.getByRole('list', {name: 'Movies'}).getByRole('link', {name: /^Paper Boats/})).toBeVisible();

  // ...but owner-only settings are hidden, and closed if reached by address.
  await page.goto('/settings');
  const tabs = page.getByRole('navigation', {name: 'Tabs'});
  await expect(tabs.getByRole('link', {name: 'Devices'})).toBeVisible();
  await expect(tabs.getByRole('link', {name: 'Security'})).toBeVisible();
  for (const owners of ['VPN', 'People', 'Libraries', 'System']) {
    await expect(tabs.getByRole('link', {name: owners})).toHaveCount(0);
  }
  for (const path of ['/settings/vpn', '/settings/people', '/settings/system', '/settings/libraries', '/settings/libraries/new']) {
    await page.goto(path);
    await expect(page.getByRole('heading', {name: 'Only the owner can change this'})).toBeVisible();
  }
  for (const url of ['/api/settings', '/api/users', '/api/invites', '/api/system', '/api/audit', '/api/vpn/nordvpn/countries', '/api/libraries/folders']) {
    const response = await page.request.get(url);
    expect(response.status(), url).toBe(403);
    expect((await response.json()).error.code, url).toBe('forbidden');
  }
  const settingsWrite = await page.request.put('/api/settings', {data: {auto_sort: false}, headers: {Origin: baseURL ?? ''}});
  expect(settingsWrite.status()).toBe(403);

  // Members can add to the library, but deleting is for owners.
  const added = await page.request.post('/api/downloads', {
    data: {url: 'http://203.0.113.10/e2e/member-clip.mp4'},
    headers: {Origin: baseURL ?? ''},
  });
  expect(added.status(), await added.text()).toBe(200);
  const paperBoats = (await (await page.request.get('/api/items?q=Paper%20Boats')).json()).items[0];
  expect((await page.request.delete(`/api/items/${paperBoats.id}`, {headers: {Origin: baseURL ?? ''}})).status()).toBe(403);
  await page.goto(`/item/${paperBoats.id}`);
  await page.getByRole('button', {name: 'More actions'}).click();
  await expect(page.getByRole('menuitem', {name: 'Edit details'})).toBeVisible();
  await expect(page.getByRole('menuitem', {name: 'Move to Trash'})).toHaveCount(0);
  // Moving files between libraries is for owners too.
  await expect(page.getByRole('menuitem', {name: /library/})).toHaveCount(0);
  await page.keyboard.press('Escape');
  // The owner removes the download that can never finish.
  const memberDownload = (await added.json()) as {id: string};
  expect((await ownerPage.request.delete(`/api/downloads/${memberDownload.id}`, {headers: {Origin: baseURL ?? ''}})).status()).toBe(204);

  // The link worked once.
  const again = await browser.newContext();
  const againPage = await again.newPage();
  await againPage.goto(link);
  await expect(againPage.getByRole('heading', {name: 'This invite link no longer works'})).toBeVisible();

  // The owner sees the new member.
  await ownerPage.reload();
  await expect(ownerPage.getByText(MEMBER_NAME, {exact: true})).toBeVisible();

  await Promise.all([owner.close(), member.close(), again.close()]);
});

test('a device pairs with a code, gets its token once, and can only add downloads', async ({browser, baseURL}) => {
  // The client side: no cookie, a chrome-extension:// origin, like the extension.
  const client = await playwrightRequest.newContext({baseURL, extraHTTPHeaders: {Origin: EXTENSION_ORIGIN}});
  const asked = await client.post('/api/pairing/request', {data: {client_name: 'Chrome on the test laptop', client_kind: 'chrome-extension'}});
  expect(asked.status()).toBe(200);
  const pairing = (await asked.json()) as {pairing_id: string; user_code: string; poll_secret: string; verify_url: string; interval: number};
  expect(pairing.user_code).toMatch(/^[A-Z0-9]{4}-[A-Z0-9]{4}$/);
  expect(pairing.verify_url).toBe(`${baseURL}/pair?code=${pairing.user_code}`);
  const poll = () => client.post('/api/pairing/poll', {data: {pairing_id: pairing.pairing_id, poll_secret: pairing.poll_secret}});

  const waiting = await poll();
  expect(waiting.status()).toBe(202);
  expect(await waiting.json()).toEqual({status: 'pending'});

  // A wrong secret learns nothing.
  const guess = await client.post('/api/pairing/poll', {data: {pairing_id: pairing.pairing_id, poll_secret: 'x'.repeat(43)}});
  expect(guess.ok()).toBe(false);

  // The owner approves on /pair after typing the code the device shows.
  const owner = await ownerContext(browser);
  const page = await owner.newPage();
  // No device is connected yet.
  await page.goto('/settings/devices');
  await expectNotClipped(page.getByText('No devices yet.', {exact: false}));
  await page.goto('/pair');
  await page.getByRole('textbox', {name: 'Code on the device'}).fill(pairing.user_code.replace('-', '').toLowerCase());
  await page.getByRole('button', {name: 'Find device'}).click();
  const panel = page.getByRole('region', {name: 'Device asking to pair'});
  await expect(panel.getByText('Chrome on the test laptop')).toBeVisible();
  await expect(panel.getByText(pairing.user_code)).toBeVisible();
  await panel.getByRole('button', {name: 'Approve'}).click();
  await expect(page.getByRole('heading', {name: 'Chrome on the test laptop is connected'})).toBeVisible();

  // The token is delivered exactly once.
  const approved = await poll();
  expect(approved.status()).toBe(200);
  const delivered = (await approved.json()) as {status: string; token: string; scopes: string[]};
  expect(delivered.status).toBe('approved');
  expect(delivered.token).toMatch(/^pst_[A-Za-z0-9_-]{43}$/);
  expect(delivered.scopes.sort()).toEqual(['downloads:create', 'downloads:read']);
  const second = await poll();
  expect(await second.text()).not.toContain(delivered.token);
  expect(second.status()).not.toBe(200);

  // The token works for adding and reading downloads (bearer requests need no Origin)...
  const device = await playwrightRequest.newContext({baseURL, extraHTTPHeaders: {Authorization: `Bearer ${delivered.token}`}});
  // 203.0.113.0/24 is a documentation range: a public address that nothing answers.
  const added = await device.post('/api/downloads', {data: {url: 'http://203.0.113.10/e2e/holiday-clip.mp4'}});
  expect(added.status(), await added.text()).toBe(200);
  const download = (await added.json()) as {id: string; kind: string; name: string};
  expect(download).toMatchObject({kind: 'http', name: 'holiday-clip.mp4'});
  expect((await device.get('/api/downloads')).status()).toBe(200);

  // ...and for nothing else.
  for (const url of ['/api/items', '/api/home', '/api/libraries', '/api/me/passkeys', '/api/settings']) {
    const response = await device.get(url);
    expect(response.status(), url).toBe(403);
  }
  // The pairing shows in Settings → Devices.
  await page.goto('/settings/devices');
  await expect(page.getByText('Chrome on the test laptop').first()).toBeVisible();

  // Clean up the download that can never finish.
  const removed = await page.request.delete(`/api/downloads/${download.id}`, {headers: {Origin: baseURL ?? ''}});
  expect(removed.status()).toBe(204);
  await Promise.all([client.dispose(), device.dispose(), owner.close()]);
});
