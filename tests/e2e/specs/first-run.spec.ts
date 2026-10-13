/**
 * First run: a fresh server asks for setup, the code from the log plus a name
 * registers the owner's passkey, and the owner lands on Home.
 */
import {expect, test} from '@playwright/test';
import {OWNER_NAME, env} from '../helpers/env';
import {VirtualAuthenticator} from '../helpers/passkeys';

test('the owner sets up the server with the code from the log and a passkey', async ({page}) => {
  const state = await (await page.request.get('/api/auth/state')).json();
  expect(state).toMatchObject({setup_required: true, authenticated: false, user: null, passkeys_ready: true, rp_id: 'localhost'});

  // Any page sends a fresh server to setup.
  await page.goto('/');
  await expect(page).toHaveURL(/\/setup$/);
  await expect(page.getByRole('heading', {name: 'Set up PopcornStash'})).toBeVisible();
  // `admin setup-code` prints a new code and the logged one stops working; the hint says so.
  await expect(page.getByText('print a new code. The one in the log then stops working.', {exact: false})).toBeVisible();

  const authenticator = await VirtualAuthenticator.attach(page);
  await page.getByLabel('Setup code').fill(env.setupCode);
  await page.getByLabel('Your name').fill(OWNER_NAME);
  await page.getByRole('button', {name: 'Create owner passkey'}).click();

  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByRole('heading', {level: 1, name: 'Home'})).toBeVisible();

  const credentials = await authenticator.credentials();
  expect(credentials).toHaveLength(1);
  expect(credentials[0]?.isResidentCredential).toBe(true);
  expect(credentials[0]?.rpId).toBe('localhost');

  const after = await (await page.request.get('/api/auth/state')).json();
  expect(after).toMatchObject({setup_required: false, authenticated: true, user: {display_name: OWNER_NAME, role: 'owner'}});

  // The setup code is single use.
  const reuse = await page.request.post('/api/auth/setup/start', {
    data: {setup_code: env.setupCode, display_name: 'Someone else'},
    headers: {Origin: new URL(page.url()).origin},
  });
  expect(reuse.status()).toBeGreaterThanOrEqual(400);

  await page.context().storageState({path: env.ownerState});
  await authenticator.save(env.ownerCredential);
});
