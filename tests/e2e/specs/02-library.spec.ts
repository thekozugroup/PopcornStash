/**
 * Browsing the sample library: Home, Movies, Shows, Home Videos, Photos and Other.
 */
import {expect, test} from '@playwright/test';
import {env} from '../helpers/env';

test.use({storageState: async ({}, use) => use(env.ownerState)});

test('Home shows the library counts and what was recently added', async ({page}) => {
  await page.goto('/');
  await expect(page.getByRole('heading', {level: 1, name: 'Home'})).toBeVisible();
  const counts = page.getByRole('list', {name: 'Your library'});
  for (const label of ['4 movies', '2 shows', '2 home videos', '3 photos', '3 other files']) {
    await expect(counts.getByRole('link', {name: label})).toBeVisible();
  }
  // The ambiguous Inbox file is waiting, and the sidebar says so.
  await expect(page.getByRole('link', {name: /^1 file needs a quick look before it is sorted\./})).toBeVisible();
  await expect(page.getByRole('navigation', {name: 'Main navigation'}).getByRole('link', {name: 'Inbox 1'})).toBeVisible();

  const recent = page.getByRole('region', {name: 'Recently added'});
  await expect(recent.getByRole('link', {name: /^Harbor Lights 2019/})).toBeVisible();
  await expect(recent.getByRole('link', {name: /^IMG_1234 /})).toBeVisible();
  await expect(recent.getByRole('link', {name: /^Quiet Meadow/})).toBeVisible();

  // Counts link to their pages.
  await counts.getByRole('link', {name: '4 movies'}).click();
  await expect(page).toHaveURL(/\/movies$/);
});

test('Movies lists the films, with thumbnails, and opens a movie', async ({page}) => {
  await page.goto('/movies');
  const grid = page.getByRole('list', {name: 'Movies'});
  for (const title of ['Paper Boats', 'Glass Orchard', 'Tin Lighthouse', 'Harbor Lights']) {
    await expect(grid.getByRole('link', {name: new RegExp(`^${title} \\d{4}`)})).toBeVisible();
  }
  const paperBoats = grid.getByRole('link', {name: /^Paper Boats 2011/});
  // The thumbnail job made a real image for the tile.
  await expect
    .poll(async () => paperBoats.locator('img').evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth), {
      timeout: 20_000,
    })
    .toBeGreaterThan(0);

  await paperBoats.click();
  await expect(page.getByRole('heading', {level: 1, name: 'Paper Boats'})).toBeVisible();
  await expect(page.getByRole('button', {name: 'Play'})).toBeVisible();
});

test('Shows lists each show and the show page lists its seasons', async ({page}) => {
  await page.goto('/shows');
  const shows = page.getByRole('list', {name: 'Shows'});
  await expect(shows.getByRole('link', {name: 'The Lantern Keepers 2 seasons · 3 episodes'})).toBeVisible();
  await expect(shows.getByRole('link', {name: /^Quiet Meadow 1 episode/})).toBeVisible();

  await shows.getByRole('link', {name: /^The Lantern Keepers/}).click();
  await expect(page).toHaveURL(/\/shows\/The%20Lantern%20Keepers$/);
  await expect(page.getByRole('heading', {level: 1, name: 'The Lantern Keepers'})).toBeVisible();
  await expect(page.getByText('2 seasons · 3 episodes')).toBeVisible();
  const episodes = page.getByRole('region', {name: 'Episodes'});
  await expect(episodes.getByRole('tab', {name: 'Season 1'})).toBeVisible();
  await expect(episodes.getByRole('tab', {name: 'Season 2'})).toBeVisible();
  await expect(episodes.getByRole('list', {name: 'Season 1'}).getByRole('listitem')).toHaveCount(2);

  await episodes.getByRole('tab', {name: 'Season 2'}).click();
  const season2 = episodes.getByRole('list', {name: 'Season 2'});
  await expect(season2.getByRole('listitem')).toHaveCount(1);
  await expect(season2.getByRole('link', {name: /S02E01/})).toBeVisible();
});

test('Home Videos shows the phone clip with the date it was filmed', async ({page}) => {
  await page.goto('/home-videos');
  const grid = page.getByRole('list', {name: 'Home Videos'});
  // Dated from the QuickTime creation time, not the file's modified time.
  await expect(grid.getByRole('link', {name: /^IMG_1234 Jun 13, 2024/})).toBeVisible();
  await expect(grid.getByRole('link', {name: /^IMG_0042 Jul 14, 2023/})).toBeVisible();

  await grid.getByRole('link', {name: /^IMG_1234/}).click();
  await expect(page.getByRole('heading', {level: 1, name: 'IMG_1234'})).toBeVisible();
  await expect(page.getByText('Apple iPhone 15').first()).toBeVisible();
});

test('the Photos timeline opens the viewer and the arrow keys move between photos', async ({page}) => {
  await page.goto('/photos');
  const month = page.getByRole('region', {name: 'May 2022'});
  await expect(month.getByRole('heading', {name: 'May 2022'})).toBeVisible();
  await expect(month.getByText('3 photos')).toBeVisible();
  const tiles = month.getByRole('list', {name: 'May 2022'}).getByRole('link');
  await expect(tiles).toHaveCount(3);
  await expect(tiles.first()).toHaveAccessibleName('Photo from May 22, 2022, Fujifilm X-T30');

  await tiles.first().click();
  const viewer = page.getByRole('dialog');
  await expect(viewer).toBeVisible();
  const photo = viewer.locator('img[alt^="Photo taken"]').first();
  await expect(photo).toBeVisible();
  await expect(viewer.getByText('Fujifilm X-T30')).toBeVisible();
  const first = await photo.getAttribute('alt');
  expect(first).toContain('May 22, 2022');
  // The full-size image really loads.
  await expect.poll(() => photo.evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth)).toBeGreaterThan(0);

  await page.keyboard.press('ArrowRight');
  await expect(viewer.locator('img[alt^="Photo taken"]').first()).not.toHaveAttribute('alt', first ?? '');
  const second = await viewer.locator('img[alt^="Photo taken"]').first().getAttribute('alt');
  expect(second).toContain('May 21, 2022');

  await page.keyboard.press('ArrowLeft');
  await expect(viewer.locator('img[alt^="Photo taken"]').first()).toHaveAttribute('alt', first ?? '');

  await page.keyboard.press('Escape');
  await expect(viewer).toBeHidden();
});

test('Other lists the audio file and the document', async ({page}) => {
  await page.goto('/other');
  const grid = page.getByRole('list', {name: 'Other'});
  await expect(grid.getByRole('link', {name: 'Morning Notes MP3 · 3 s'})).toBeVisible();
  await expect(grid.getByRole('link', {name: /^Recipe Card PDF/})).toBeVisible();
});

test.describe('on a phone', () => {
  test.use({viewport: {width: 390, height: 844}, isMobile: true, hasTouch: true});

  test('every main page fits the screen without sideways scrolling', async ({page}) => {
    await page.goto('/');
    // The five most used destinations stay one tap away.
    const quick = page.getByRole('navigation', {name: 'Quick links'});
    for (const label of ['Home', 'Movies', 'Shows', 'Photos', 'Downloads']) {
      await expect(quick.getByRole('link', {name: label, exact: true})).toBeVisible();
    }
    for (const path of ['/', '/movies', '/shows', '/shows/The%20Lantern%20Keepers', '/home-videos', '/photos', '/other', '/inbox', '/downloads', '/settings/vpn', '/settings/security', '/pair']) {
      await page.goto(path);
      await expect(page.getByRole('heading', {level: 1}).first()).toBeVisible();
      // Let lazy content (thumbnails, lists) settle; the event stream keeps the network busy.
      await page.waitForTimeout(400);
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
      expect(overflow, `${path} scrolls sideways by ${overflow}px`).toBeLessThanOrEqual(0);
    }
  });
});
