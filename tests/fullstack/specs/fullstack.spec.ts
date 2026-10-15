/**
 * The whole journey in the real UI, against the Docker stack that
 * scripts/fullstack-e2e.sh starts: first-run setup with the code from the
 * container log, a WireGuard file uploaded in Settings, a magnet pasted on
 * Downloads, real progress from a seeder that is only reachable through the
 * VPN, the film sorted into Movies and played. Then the proof outside the
 * browser (who the tracker saw, who owns the files and where they are), and
 * the VPN peer going away and coming back while the web app keeps working.
 *
 * One browser context and one passkey for the whole run; the steps depend on
 * each other and run in order.
 */
import fs from 'node:fs';
import {expect, test, type BrowserContext, type Locator, type Page} from '@playwright/test';
import {VirtualAuthenticator} from '../../e2e/helpers/passkeys';
import {
  OWNER_NAME,
  announces,
  docker,
  engineHealth,
  engineGet,
  ownedBy,
  ownersUnder,
  saveEvidence,
  screenshotPath,
  seederPeers,
  stack,
  stashPath,
} from '../helpers/stack';

test.describe.configure({mode: 'serial'});

let context: BrowserContext;
let page: Page;

test.beforeAll(async ({browser}) => {
  context = await browser.newContext();
  page = await context.newPage();
  await VirtualAuthenticator.attach(page);
});

test.afterAll(async () => {
  await context?.close();
});

/** The VPN tag in the sidebar ("Protected · WireGuard · …", "Torrents paused: …"). */
function sidebarVpnTag(): Locator {
  return page
    .getByRole('navigation', {name: 'Main navigation'})
    .getByRole('status')
    .filter({hasText: /Protected|Torrents|VPN/});
}

/** The status line of the VPN settings page. */
function vpnStatusLine(): Locator {
  return page.getByRole('region', {name: 'VPN status'}).getByRole('status').first();
}

async function shoot(name: string, colorScheme: 'light' | 'dark' = 'light') {
  await page.emulateMedia({colorScheme});
  // What a person sees at 1440x900, from the top of the page.
  await page.evaluate(() => window.scrollTo(0, 0));
  // Let fonts, images and the theme change settle before the picture. (Never wait for
  // "networkidle": the live event stream keeps a request open for as long as the page.)
  await page.evaluate(async () => {
    const images = [...document.images].filter(image => !image.complete);
    const loaded = Promise.all(
      images.map(
        image =>
          new Promise(resolve => {
            image.addEventListener('load', resolve, {once: true});
            image.addEventListener('error', resolve, {once: true});
          }),
      ),
    );
    await Promise.race([Promise.all([document.fonts.ready, loaded]), new Promise(resolve => setTimeout(resolve, 3_000))]);
  });
  await page.waitForTimeout(400);
  await page.screenshot({path: screenshotPath(name)});
  await page.emulateMedia({colorScheme: 'light'});
}

function movieRel(): string {
  const folder = `${stack.title} (${stack.year})`;
  return `Movies/${folder}`;
}

test('the owner sets up the server with the code from the container log', async () => {
  await page.goto('/');
  await expect(page).toHaveURL(/\/setup$/);
  await page.getByLabel('Setup code').fill(stack.setupCode);
  await page.getByLabel('Your name').fill(OWNER_NAME);
  await page.getByRole('button', {name: 'Create owner passkey'}).click();
  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByRole('heading', {level: 1, name: 'Home'})).toBeVisible();
  // No VPN yet: the sidebar says torrents are blocked.
  await expect(sidebarVpnTag()).toContainText('Torrents blocked');
});

test('a WireGuard file uploaded in Settings connects the VPN', async () => {
  await page.goto('/settings/vpn');
  await expect(vpnStatusLine()).toContainText('Torrents blocked: no VPN set up');

  await page.getByText('Use a WireGuard or OpenVPN file instead').click();
  const chooser = page.waitForEvent('filechooser');
  // The visible "Choose file" field opens the system file picker, as a person would use it.
  await page.getByText('Choose file', {exact: true}).click();
  await (await chooser).setFiles(stack.wireguardFile);
  const uploaded = page.waitForResponse(
    response => response.url().endsWith('/api/vpn/wireguard') && response.request().method() === 'POST',
  );
  await page.getByRole('button', {name: 'Use this file'}).click();
  // The answer (and the event every open page gets) says the saved VPN is connecting,
  // never "not set up" while the supervisor has yet to apply it.
  const answer = await (await uploaded).json();
  expect(answer).toMatchObject({configured: true, provider: 'wireguard', kill_switch: true});
  expect(answer.state).not.toBe('not_configured');

  // The supervisor brings the tunnel up (boringtun here) and the engine starts its session.
  await expect(vpnStatusLine()).toContainText('Protected', {timeout: 120_000});
  await expect(page.getByRole('region', {name: 'VPN status'})).toContainText('Kill switch');
  await expect(sidebarVpnTag()).toContainText('Protected');
  await expect
    .poll(() => engineHealth().engine.state, {timeout: 90_000, message: 'the torrent engine starts its session'})
    .toBe('running');
  const health = engineHealth();
  expect(health.vpn).toMatchObject({state: 'connected', interface: 'psw0', kill_switch: true});
  saveEvidence('engine-health-connected.json', JSON.stringify(health, null, 2));

  await shoot('vpn-connected-1440-light.png');
  await shoot('vpn-connected-1440-dark.png', 'dark');
});

test('a magnet pasted on Downloads comes in through the VPN and is sorted into Movies', async () => {
  await page.goto('/downloads');
  const panel = page.getByRole('region', {name: 'Add to downloads'});
  await panel.getByRole('textbox', {name: 'Paste a link or magnet'}).fill(stack.magnet);
  await panel.getByRole('button', {name: 'Add', exact: true}).click();

  const inProgress = page.getByRole('list', {name: 'In progress'});
  const row = inProgress.getByRole('listitem').filter({hasText: stack.torrentName});
  await expect(row).toBeVisible();
  const bar = row.getByRole('progressbar');

  // Real progress: the seeder sends 16 KiB/s, so the bar moves through several values.
  const seen = new Set<number>();
  let shot = false;
  let peersDuring: string[] = [];
  await expect
    .poll(
      async () => {
        if (!(await bar.isVisible().catch(() => false))) return seen.size;
        const now = Number(await bar.getAttribute('aria-valuenow').catch(() => null));
        if (Number.isFinite(now) && now > 0 && now < 100) {
          seen.add(Math.round(now));
          if (!shot && now >= 15 && now <= 85) {
            shot = true;
            await expect(row).toContainText('Downloading');
            peersDuring = seederPeers();
            await shoot('downloads-mid-1440-light.png');
          }
        }
        return shot ? seen.size : 0;
      },
      {timeout: 180_000, intervals: [500], message: 'the download shows progress between 15% and 85%'},
    )
    .toBeGreaterThanOrEqual(3);
  saveEvidence('progress-seen.txt', [...seen].sort((a, b) => a - b).join(' ') + '\n');
  saveEvidence('seeder-peers-mid-download.txt', peersDuring.join('\n') + '\n');
  // The seeder's peer connections came from the VPN peer's address, never from the container itself.
  expect(peersDuring, 'the seeder sees the leecher at the VPN address').toContain(stack.wgIp);
  expect(peersDuring).not.toContain(stack.mainIp);

  const finished = page.getByRole('list', {name: 'Finished'});
  const done = finished.getByRole('listitem').filter({hasText: stack.torrentName});
  // The finished row names the library it went to, as a link to it.
  await expect(done).toContainText('In Movies', {timeout: 180_000});
  await expect(done.getByRole('link', {name: 'Movies', exact: true})).toBeVisible();
  await expect(inProgress.getByRole('listitem').filter({hasText: stack.torrentName})).toHaveCount(0);
  await expect.poll(() => engineGet<{info_hash: string}[]>('/torrents')
    .some(torrent => torrent.info_hash === stack.infoHash)).toBe(false);
  await shoot('downloads-finished-1440-light.png');
});

test('Movies shows the film with a thumbnail, and it plays', async () => {
  // The old address leads to the library that takes movies.
  await page.goto('/movies');
  await expect(page).toHaveURL(/\/libraries\/[0-9a-f-]+$/);
  const tile = page.getByRole('link', {name: new RegExp(`^${stack.title}`)});
  await expect(tile).toBeVisible();
  // The thumbnail is made in the background; the page shows it once it exists.
  await expect
    .poll(
      async () => {
        const image = tile.locator('img');
        if ((await image.count()) === 0) {
          await page.reload();
          return 0;
        }
        return image.evaluate((element: HTMLImageElement) => (element.complete ? element.naturalWidth : 0));
      },
      {timeout: 60_000, intervals: [2_000], message: 'the film has a thumbnail'},
    )
    .toBeGreaterThan(0);
  await shoot('movies-1440-light.png');

  await page.goto('/');
  const recent = page.getByRole('link', {name: new RegExp(`^${stack.title}`)}).first();
  await expect(recent).toBeVisible();
  await expect(recent.locator('img')).toBeVisible();
  await shoot('home-1440-light.png');

  await page.goto('/movies');
  await tile.click();
  await expect(page.getByRole('heading', {level: 1, name: stack.title})).toBeVisible();
  const decision = page.waitForResponse(
    response => /\/api\/items\/[^/]+\/playback$/.test(response.url()) && response.request().method() === 'POST',
  );
  await page.getByRole('button', {name: /^(Play|Resume from)/}).first().click();
  const answer = await decision;
  expect(answer.status()).toBe(200);
  expect((await answer.json()).mode, 'a VP9/Opus WebM plays directly').toBe('direct');
  const currentTime = () => page.locator('video').evaluate((video: HTMLVideoElement) => video.currentTime);
  await expect.poll(currentTime, {timeout: 20_000, message: 'the video plays'}).toBeGreaterThan(1.5);
});

test('outside the browser: the leecher only announced from the VPN address, and the files belong to PUID in Movies', async () => {
  const seen = announces();
  saveEvidence('tracker-announces.jsonl', seen.map(entry => JSON.stringify(entry)).join('\n') + '\n');
  const leecher = seen.filter(entry => entry.ip !== stack.seederIp);
  expect(leecher.length, 'the leecher announced to the tracker').toBeGreaterThan(0);
  expect(new Set(leecher.map(entry => entry.ip)), 'every leecher announce came from the VPN peer').toEqual(
    new Set([stack.wgIp]),
  );
  expect(seen.filter(entry => entry.ip === stack.mainIp), 'nothing came from the container itself').toEqual([]);
  // The tracker watched it download through the VPN: announces with part of the film still to come.
  expect(leecher.some(entry => entry.left > 0 && entry.left < stack.torrentBytes)).toBe(true);

  const rel = movieRel();
  const file = `${rel}/${stack.title} (${stack.year}).webm`;
  const entries = ownersUnder(rel);
  saveEvidence('movie-owners.txt', entries.map(entry => `${entry.uid}:${entry.gid} ${entry.path}`).join('\n') + '\n');
  expect(entries.map(entry => entry.path)).toEqual([rel, file]);
  for (const entry of entries) {
    expect({path: entry.path, uid: entry.uid, gid: entry.gid}).toEqual({path: entry.path, uid: stack.puid, gid: stack.pgid});
  }
  // Nothing of the release is left behind in Downloads, and nothing in the stash belongs to the engine user.
  for (const zone of ['Downloads/In Progress', 'Downloads/Completed']) {
    const left = ownersUnder(zone).filter(entry => entry.path !== zone && !entry.path.startsWith(`${zone}/.uploads`));
    expect(left.map(entry => entry.path), `${zone} is empty again`).toEqual([]);
  }
  expect(ownedBy(stack.engineUid), 'no file in the stash belongs to the engine user').toEqual([]);
  expect(fs.statSync(stashPath(file)).size).toBe(stack.torrentBytes);

  await page.goto('/');
  await expect(sidebarVpnTag()).toContainText('Protected');
});

test('with the VPN peer gone torrents stop and the web app keeps working; it recovers on its own', async () => {
  test.setTimeout(10 * 60_000);
  docker(['stop', stack.wgContainer]);
  const stoppedAt = Date.now();
  // WireGuard has no "down" signal: the supervisor notices when the handshake goes stale.
  await expect
    .poll(() => engineHealth().vpn.state, {timeout: 240_000, intervals: [2_000], message: 'the engine reports the VPN down'})
    .not.toBe('connected');
  await expect.poll(() => engineHealth().engine.state, {timeout: 30_000}).toBe('blocked');
  saveEvidence(
    'engine-health-vpn-down.json',
    JSON.stringify({seconds_after_stop: Math.round((Date.now() - stoppedAt) / 1000), ...engineHealth()}, null, 2),
  );

  // The web app still answers and plays from the library.
  await page.goto('/settings/vpn');
  await expect(vpnStatusLine()).not.toContainText('Protected');
  await expect(sidebarVpnTag()).toContainText('Torrents');
  await shoot('vpn-down-1440-light.png');
  await page.goto('/movies');
  await page.getByRole('link', {name: new RegExp(`^${stack.title}`)}).click();
  await page.getByRole('button', {name: /^(Play|Resume from)/}).first().click();
  const start = await page.locator('video').evaluate((video: HTMLVideoElement) => video.currentTime);
  await expect
    .poll(() => page.locator('video').evaluate((video: HTMLVideoElement) => video.currentTime), {timeout: 20_000})
    .toBeGreaterThan(start + 1);

  docker(['start', stack.wgContainer]);
  await expect
    .poll(() => engineHealth().vpn.state, {timeout: 180_000, intervals: [2_000], message: 'the tunnel reconnects'})
    .toBe('connected');
  await expect.poll(() => engineHealth().engine.state, {timeout: 90_000, intervals: [2_000]}).toBe('running');
  await page.goto('/settings/vpn');
  await expect(vpnStatusLine()).toContainText('Protected', {timeout: 30_000});
  await expect(sidebarVpnTag()).toContainText('Protected');
});
