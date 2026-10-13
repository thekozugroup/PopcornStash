/**
 * Paths and values the global setup hands to the tests, plus small shared helpers.
 */
import {execFileSync} from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import {expect, type APIRequestContext, type Browser, type BrowserContext, type Locator, type Page} from '@playwright/test';
import {VirtualAuthenticator} from './passkeys';

function required(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set; run the suite with \`npx playwright test\` so the global setup runs.`);
  return value;
}

export const env = {
  get work() {
    return required('E2E_WORK');
  },
  get stash() {
    return required('E2E_STASH');
  },
  get files() {
    return required('E2E_FILES');
  },
  get serverLog() {
    return required('E2E_SERVER_LOG');
  },
  get serverBin() {
    return required('E2E_SERVER_BIN');
  },
  get config() {
    return required('E2E_CONFIG');
  },
  get setupCode() {
    return required('E2E_SETUP_CODE');
  },
  /** Cookie of the owner's first session (saved by the first-run test). */
  get ownerState() {
    return path.join(this.work, 'owner-state.json');
  },
  /** The owner's passkey, exported from the virtual authenticator. */
  get ownerCredential() {
    return path.join(this.work, 'owner-credential.json');
  },
};

export const OWNER_NAME = 'Robin Example';

/** A file inside the sample stash. */
export function stashPath(rel: string): string {
  return path.join(env.stash, ...rel.split('/'));
}

export function exists(rel: string): boolean {
  return fs.existsSync(stashPath(rel));
}

/** Waits until a stash-relative path exists (sorting runs in the background). */
export async function expectFile(rel: string, timeout = 20_000) {
  await expect.poll(() => exists(rel), {timeout, message: `expected ${rel} to exist`}).toBe(true);
}

/** A browser context signed in as the owner with the saved cookie. */
export async function ownerContext(browser: Browser): Promise<BrowserContext> {
  return browser.newContext({storageState: env.ownerState});
}

/**
 * Signs in on `page` with the owner's passkey (loaded into a new virtual
 * authenticator), then saves the passkey back so its sign count stays current.
 */
export async function signInWithPasskey(page: Page): Promise<VirtualAuthenticator> {
  const authenticator = await VirtualAuthenticator.attach(page);
  await authenticator.load(env.ownerCredential);
  await page.goto('/login');
  await page.getByRole('button', {name: 'Sign in with a passkey'}).click();
  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByRole('heading', {level: 1, name: 'Home'})).toBeVisible();
  await authenticator.save(env.ownerCredential);
  return authenticator;
}

/** JSON body of an API call made with the page's cookie (Origin is set by the request context). */
export async function apiJson<T>(request: APIRequestContext, url: string): Promise<T> {
  const response = await request.get(url);
  expect(response.status(), `${url} answered ${response.status()}: ${await response.text()}`).toBe(200);
  return (await response.json()) as T;
}

export interface ItemSummary {
  id: string;
  title: string;
  category: string;
  status: string;
  rel_path: string;
  taken_at: number | null;
  camera: string | null;
  suggestion?: {title: string; category: string; destination: string; confidence: string} | null;
}

/** Finds an item by title through the API (as the owner). */
export async function findItem(request: APIRequestContext, title: string, query = ''): Promise<ItemSummary> {
  const page = await apiJson<{items: ItemSummary[]}>(request, `/api/items?limit=200${query}`);
  const item = page.items.find(candidate => candidate.title === title);
  if (!item) throw new Error(`No item titled ${title}; found ${page.items.map(i => i.title).join(', ')}`);
  return item;
}

/** Runs `popcornstash admin ...` against the test server's folders, as `docker exec` would. */
export function runAdmin(args: string[], publicUrl: string): string {
  const childEnv: NodeJS.ProcessEnv = {...process.env};
  for (const key of Object.keys(childEnv)) {
    if (key.startsWith('STASH_')) delete childEnv[key];
  }
  Object.assign(childEnv, {STASH_ROOT: env.stash, STASH_CONFIG: env.config, STASH_PUBLIC_URL: publicUrl, NO_COLOR: '1'});
  return execFileSync(env.serverBin, ['admin', ...args], {env: childEnv, encoding: 'utf8'});
}

/** The element is visible and its box (padding included) fits inside its parent, so nothing is cut off. */
export async function expectNotClipped(locator: Locator) {
  await expect(locator).toBeVisible();
  const boxes = await locator.evaluate((element: HTMLElement) => {
    const own = element.getBoundingClientRect();
    const parent = (element.parentElement ?? element).getBoundingClientRect();
    return {top: own.top, bottom: own.bottom, parentTop: parent.top, parentBottom: parent.bottom};
  });
  expect(boxes.top, 'top edge inside its panel').toBeGreaterThanOrEqual(boxes.parentTop - 0.5);
  expect(boxes.bottom, 'bottom edge inside its panel').toBeLessThanOrEqual(boxes.parentBottom + 0.5);
}
