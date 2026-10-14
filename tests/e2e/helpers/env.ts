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
  /** The server's public address (the tests' baseURL). */
  get publicUrl() {
    return `http://localhost:${process.env.E2E_PORT ?? 8788}`;
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

/** The four libraries every stash starts with (docs/architecture.md section 6, fixed ids). */
export const LIBRARY = {
  movies: '0192a000-0000-7000-8000-000000000001',
  shows: '0192a000-0000-7000-8000-000000000002',
  homeVideos: '0192a000-0000-7000-8000-000000000003',
  photos: '0192a000-0000-7000-8000-000000000004',
} as const;

/** A library's page. */
export function libraryPath(id: string): string {
  return `/libraries/${id}`;
}

/** A show's page inside a series library. */
export function showPath(libraryId: string, show: string): string {
  return `${libraryPath(libraryId)}/shows/${encodeURIComponent(show)}`;
}

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

/**
 * Invites a member (as the owner, through the API) who joins with a passkey on a
 * fresh browser context; answers that signed-in context and the member's id.
 */
export async function joinAsMember(
  browser: Browser,
  name: string,
): Promise<{context: BrowserContext; page: Page; id: string}> {
  const owner = await ownerContext(browser);
  const ownerPage = await owner.newPage();
  const created = await ownerPage.request.post('/api/invites', {
    data: {role: 'member', expires_in_hours: 24},
    headers: {Origin: new URL(env.publicUrl).origin},
  });
  expect(created.status(), await created.text()).toBe(200);
  const invite = (await created.json()) as {url: string};
  await owner.close();

  // A device of their own: nobody signed in (a spec's storageState would sign in the owner).
  const context = await browser.newContext({storageState: {cookies: [], origins: []}});
  const page = await context.newPage();
  const authenticator = await VirtualAuthenticator.attach(page);
  await page.goto(new URL(invite.url).pathname);
  await page.getByLabel('Your name').fill(name);
  await page.getByRole('button', {name: 'Create passkey and join'}).click();
  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByRole('heading', {level: 1, name: 'Home'})).toBeVisible();
  expect(await authenticator.credentials()).toHaveLength(1);
  const me = (await (await page.request.get('/api/me')).json()) as {id: string; role: string};
  expect(me.role).toBe('member');
  return {context, page, id: me.id};
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
