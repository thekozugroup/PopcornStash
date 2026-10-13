/**
 * Settings, System: the version links to this build's source code, and when
 * STASH_HWACCEL sets hardware acceleration (the server says `set_by_env`) the
 * control is greyed out with the variable named, and saving the other streaming
 * settings leaves hardware acceleration out of the request. When the encoder it
 * names is not on the server, the page says videos are converted on the processor.
 */
import {expect, test, type Page} from '@playwright/test';
import {ownerContext} from '../helpers/env';

const SOURCE = /^https:\/\/github\.com\/thekozugroup\/PopcornStash(\/tree\/v[0-9A-Za-z.-]+)?$/;
const LOCKED_SENTENCE =
  'To change this, edit or remove STASH_HWACCEL on the container, then recreate it (docker compose up -d, or Apply on Unraid).';
const MISSING = 'Not found on this server, so videos are converted on the processor. Add /dev/dri to the container.';

async function screenshots(page: Page, name: string) {
  const info = test.info();
  for (const [label, size] of [
    ['desktop', {width: 1280, height: 900}],
    ['phone', {width: 390, height: 844}],
  ] as const) {
    await page.setViewportSize(size);
    await page.screenshot({path: info.outputPath(`${name}-${label}.png`), fullPage: true});
  }
  await page.setViewportSize({width: 1280, height: 860});
}

test('the version links to the source code, and hardware acceleration can be chosen', async ({browser}) => {
  const owner = await ownerContext(browser);
  const page = await owner.newPage();
  await page.goto('/settings/system');
  const link = page.getByRole('link', {name: /Source code/});
  await expect(link).toBeVisible();
  await expect(link).toHaveAttribute('href', SOURCE);
  const system = await (await page.request.get('/api/system')).json();
  expect(system.source).toMatch(SOURCE);
  expect(system.hwaccel.set_by_env).toBe(false);
  await expect(page.getByRole('combobox', {name: 'Hardware acceleration'})).toBeEnabled();
  await expect(page.getByText(LOCKED_SENTENCE)).toHaveCount(0);
  await expect(page.getByText(MISSING)).toHaveCount(0);
  await screenshots(page, 'system');
  await owner.close();
});

test('STASH_HWACCEL greys out hardware acceleration and saves leave it out', async ({browser}) => {
  const owner = await ownerContext(browser);
  const page = await owner.newPage();
  // The server as it answers with STASH_HWACCEL=vaapi set on the container.
  await page.route('**/api/system', async route => {
    const response = await route.fetch();
    const body = await response.json();
    await route.fulfill({
      response,
      json: {...body, hwaccel: {...body.hwaccel, configured: 'vaapi', available: ['vaapi'], set_by_env: true}},
    });
  });
  const saved: unknown[] = [];
  await page.route('**/api/settings', async route => {
    const response = await route.fetch({method: 'GET'});
    const current = await response.json();
    current.transcode.hwaccel = 'vaapi';
    if (route.request().method() === 'PUT') {
      const patch = route.request().postDataJSON() as {transcode?: Record<string, unknown>};
      saved.push(patch);
      await route.fulfill({response, json: {...current, transcode: {...current.transcode, ...patch.transcode}}});
    } else {
      await route.fulfill({response, json: current});
    }
  });

  await page.goto('/settings/system');
  const hardware = page.getByRole('combobox', {name: 'Hardware acceleration'});
  await expect(hardware).toBeDisabled();
  await expect(hardware).toContainText('VAAPI');
  await expect(page.getByText(LOCKED_SENTENCE)).toBeVisible();
  await expect(page.getByText(MISSING)).toHaveCount(0);
  await screenshots(page, 'system-hwaccel-set-by-env');

  await page.getByRole('combobox', {name: 'Quality'}).click();
  await page.getByRole('option', {name: /Smaller streams/}).click();
  await page.getByRole('button', {name: 'Save changes'}).click();
  await expect(page.getByLabel('Notifications').getByText('Streaming settings saved.', {exact: false})).toBeVisible();
  expect(saved).toHaveLength(1);
  expect(saved[0]).toMatchObject({transcode: {crf: 28}});
  expect((saved[0] as {transcode: Record<string, unknown>}).transcode).not.toHaveProperty('hwaccel');
  await owner.close();
});

test('STASH_HWACCEL naming an encoder this server lacks says the processor converts instead', async ({browser}) => {
  const owner = await ownerContext(browser);
  const page = await owner.newPage();
  // STASH_HWACCEL=vaapi on a container without /dev/dri: the server falls back to the processor.
  await page.route('**/api/system', async route => {
    const response = await route.fetch();
    const body = await response.json();
    await route.fulfill({
      response,
      json: {...body, hwaccel: {...body.hwaccel, configured: 'vaapi', available: [], set_by_env: true}},
    });
  });
  await page.route('**/api/settings', async route => {
    const response = await route.fetch();
    const current = await response.json();
    await route.fulfill({response, json: {...current, transcode: {...current.transcode, hwaccel: 'vaapi'}}});
  });

  await page.goto('/settings/system');
  const hardware = page.getByRole('combobox', {name: 'Hardware acceleration'});
  await expect(hardware).toBeDisabled();
  await expect(hardware).toContainText('VAAPI');
  await expect(page.getByText(LOCKED_SENTENCE)).toBeVisible();
  await expect(page.getByLabel('Streaming').getByText(MISSING)).toBeVisible();
  await screenshots(page, 'system-hwaccel-missing');
  await owner.close();
});
