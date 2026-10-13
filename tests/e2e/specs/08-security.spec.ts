/**
 * Request security (docs/architecture.md section 7): cookie-authenticated
 * writes need a matching Origin, and every response carries the security headers.
 */
import {expect, request as playwrightRequest, test} from '@playwright/test';
import {env, findItem} from '../helpers/env';

const CSP =
  "default-src 'self'; img-src 'self' blob: data:; media-src 'self' blob:; worker-src 'self' blob:; " +
  "style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; " +
  "base-uri 'none'; form-action 'self'";

test.use({storageState: async ({}, use) => use(env.ownerState)});

test('a cookie-authenticated POST without a matching Origin is refused', async ({baseURL}) => {
  // The owner's cookie, but no Origin header (as a cross-site form or script without CORS would send).
  const bare = await playwrightRequest.newContext({baseURL, storageState: env.ownerState});
  expect((await bare.get('/api/me')).status()).toBe(200);

  const noOrigin = await bare.post('/api/library/rescan');
  expect(noOrigin.status()).toBe(403);
  expect(await noOrigin.json()).toEqual({error: {code: 'forbidden', message: 'This request came from another site.'}});

  const otherSite = await bare.post('/api/library/rescan', {headers: {Origin: 'https://evil.example'}});
  expect(otherSite.status()).toBe(403);
  const otherReferer = await bare.post('/api/library/rescan', {headers: {Referer: 'https://evil.example/page'}});
  expect(otherReferer.status()).toBe(403);
  // Look-alike origins on another port are another site too.
  const otherPort = await bare.post('/api/library/rescan', {headers: {Origin: 'http://localhost:1'}});
  expect(otherPort.status()).toBe(403);

  // A form-encoded body is refused even from the right origin (forms cannot send JSON).
  const form = await bare.post('/api/downloads', {
    headers: {Origin: baseURL ?? '', 'Content-Type': 'application/x-www-form-urlencoded'},
    data: 'url=https%3A%2F%2Fexample.com%2Fa.mp4',
  });
  expect(form.ok()).toBe(false);
  expect([400, 403, 415]).toContain(form.status());

  // The same request with the app's own origin goes through.
  const same = await bare.post('/api/library/rescan', {headers: {Origin: baseURL ?? ''}});
  expect(same.status()).toBe(202);
  await bare.dispose();
});

test('pages and API responses carry the security headers', async ({page, baseURL}) => {
  const violations: string[] = [];
  page.on('console', message => {
    if (/Content Security Policy/i.test(message.text())) violations.push(message.text());
  });

  const document = await page.goto('/');
  expect(document?.status()).toBe(200);
  const headers = document?.headers() ?? {};
  expect(headers['content-security-policy']).toBe(CSP);
  expect(headers['x-content-type-options']).toBe('nosniff');
  expect(headers['referrer-policy']).toBe('no-referrer');
  expect(headers['permissions-policy']).toBe('camera=(), microphone=(), geolocation=()');
  expect(headers['cross-origin-opener-policy']).toBe('same-origin');
  expect(headers['cache-control']).toBe('no-cache');
  // Plain http on localhost: no HSTS.
  expect(headers['strict-transport-security']).toBeUndefined();
  await expect(page.getByRole('heading', {level: 1, name: 'Home'})).toBeVisible();

  for (const url of ['/api/health', '/api/auth/state', '/api/library/summary', '/login']) {
    const response = await page.request.get(url);
    expect(response.headers()['content-security-policy'], url).toBe(CSP);
    expect(response.headers()['x-content-type-options'], url).toBe('nosniff');
  }

  // Library files are served sandboxed, so a file in the stash cannot act as the signed-in user.
  const audio = await findItem(page.request, 'Morning Notes');
  const file = await page.request.get(`/api/items/${audio.id}/file`);
  expect(file.status()).toBe(200);
  expect(file.headers()['content-security-policy']).toMatch(/^default-src 'none'; .*; sandbox$/);
  expect(file.headers()['x-content-type-options']).toBe('nosniff');
  // PDFs skip the sandbox (Chrome's viewer refuses it) but still may not run scripts or connect anywhere.
  const pdf = await findItem(page.request, 'Recipe Card');
  const pdfFile = await page.request.get(`/api/items/${pdf.id}/file`);
  expect(pdfFile.headers()['content-type']).toBe('application/pdf');
  expect(pdfFile.headers()['content-security-policy']).toMatch(/^default-src 'none';/);
  expect(pdfFile.headers()['content-security-policy']).not.toMatch(/script-src|connect-src/);

  // The session cookie is HttpOnly and SameSite=Strict (named `stash` on http://localhost).
  const cookies = await page.context().cookies(baseURL);
  const session = cookies.find(cookie => cookie.name === 'stash' || cookie.name === '__Host-stash');
  expect(session).toMatchObject({name: 'stash', httpOnly: true, sameSite: 'Strict', path: '/'});

  // The app itself runs under the policy without violations.
  await page.goto('/movies');
  await expect(page.getByRole('heading', {level: 1, name: 'Movies'})).toBeVisible();
  await page.goto('/settings/system');
  await expect(page.getByRole('heading', {level: 1, name: 'Settings'})).toBeVisible();
  expect(violations).toEqual([]);
});
