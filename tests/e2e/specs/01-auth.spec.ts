/**
 * Signing out and back in with the passkey (no username), and a browser
 * without the passkey staying at the sign-in page.
 */
import {expect, test} from '@playwright/test';
import {OWNER_NAME, env, runAdmin, signInWithPasskey} from '../helpers/env';
import {VirtualAuthenticator} from '../helpers/passkeys';

test('the owner signs out and signs back in with only the passkey', async ({page}) => {
  // A fresh browser: sign in with the owner's passkey (a new session, so the saved one stays valid).
  const authenticator = await signInWithPasskey(page);
  await expect(page.getByText(OWNER_NAME)).toBeVisible();

  await page.getByRole('button', {name: 'Sign out'}).click();
  await expect(page).toHaveURL(/\/login$/);
  await expect(page.getByRole('heading', {name: 'Sign in to PopcornStash'})).toBeVisible();
  // There is nothing to type: no username or password fields.
  await expect(page.getByRole('textbox')).toHaveCount(0);
  expect((await page.request.get('/api/me')).status()).toBe(401);

  // Signed-out pages send the visitor back to sign in, remembering where they were going.
  await page.goto('/movies');
  await expect(page).toHaveURL(/\/login\?next=%2Fmovies$/);

  await page.getByRole('button', {name: 'Sign in with a passkey'}).click();
  await expect(page).toHaveURL(/\/movies$/);
  await expect(page.getByRole('heading', {level: 1, name: 'Movies'})).toBeVisible();
  const me = await (await page.request.get('/api/me')).json();
  expect(me).toMatchObject({display_name: OWNER_NAME, role: 'owner'});
  await authenticator.save(env.ownerCredential);
});

test('a browser without the passkey cannot get past the sign-in page', async ({browser}) => {
  const context = await browser.newContext();
  const page = await context.newPage();
  // An authenticator with no PopcornStash passkey on it.
  const authenticator = await VirtualAuthenticator.attach(page);
  expect(await authenticator.credentials()).toHaveLength(0);

  for (const path of ['/', '/movies', '/settings', '/inbox']) {
    await page.goto(path);
    await expect(page).toHaveURL(/\/login(\?next=.*)?$/);
    await expect(page.getByRole('heading', {name: 'Sign in to PopcornStash'})).toBeVisible();
  }

  await page.getByRole('button', {name: 'Sign in with a passkey'}).click();
  // The browser has nothing to offer; the visitor stays on the sign-in page.
  await page.waitForTimeout(1500);
  await expect(page).toHaveURL(/\/login/);
  await expect(page.getByRole('heading', {name: 'Sign in to PopcornStash'})).toBeVisible();

  const state = await (await page.request.get('/api/auth/state')).json();
  expect(state).toMatchObject({setup_required: false, authenticated: false, user: null});
  for (const url of ['/api/me', '/api/items', '/api/library/summary', '/api/downloads']) {
    const response = await page.request.get(url);
    expect(response.status(), url).toBe(401);
  }
  // Setup is closed once an owner exists, even with the old code.
  const setup = await page.request.post('/api/auth/setup/start', {
    data: {setup_code: env.setupCode, display_name: 'Intruder'},
    headers: {Origin: new URL(page.url()).origin},
  });
  expect(setup.ok()).toBe(false);
  await context.close();
});

test('the owner recovers access with a link printed by the admin command', async ({browser, baseURL}) => {
  const base = baseURL ?? '';
  // Setup is over, so the setup-code command says so instead of printing a code.
  expect(runAdmin(['setup-code'], base)).toContain('An owner already exists.');

  const printed = runAdmin(['reset-owner'], base);
  const link = /(http:\/\/\S+\/recover\/\S+)/.exec(printed)?.[1];
  expect(link, printed).toBeTruthy();

  // A new device with no passkey on it.
  const context = await browser.newContext();
  const page = await context.newPage();
  const authenticator = await VirtualAuthenticator.attach(page);
  await page.goto(link ?? '');
  await expect(page.getByRole('heading', {name: 'Recover owner access'})).toBeVisible();
  await page.getByRole('button', {name: 'Create new owner passkey'}).click();
  // Straight to Security, where lost passkeys can be removed.
  await expect(page).toHaveURL(/\/settings\/security$/);
  await expect(page.getByRole('heading', {level: 1, name: 'Settings'})).toBeVisible();
  expect(await (await page.request.get('/api/me')).json()).toMatchObject({display_name: OWNER_NAME, role: 'owner'});
  expect(await authenticator.credentials()).toHaveLength(1);
  // The owner now has the original passkey and the new one.
  expect(await (await page.request.get('/api/me/passkeys')).json()).toHaveLength(2);

  // The link works once.
  const again = await browser.newContext();
  const againPage = await again.newPage();
  await againPage.goto(link ?? '');
  await againPage.getByRole('button', {name: 'Create new owner passkey'}).click();
  await expect(againPage.getByRole('heading', {name: 'This recovery link no longer works'})).toBeVisible();
  await expect(againPage.getByText('docker exec popcornstash popcornstash admin reset-owner')).toBeVisible();
  await Promise.all([context.close(), again.close()]);
});
