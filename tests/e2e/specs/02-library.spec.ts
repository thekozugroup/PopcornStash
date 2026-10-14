/**
 * Browsing the sample stash through its four starting libraries (Movies, Shows,
 * Home Videos, Photos), Home's shelf per library, Other files, and the old
 * fixed-category addresses that now lead to the library that takes that kind.
 */
import {execFileSync} from 'node:child_process';
import {expect, test, type Locator} from '@playwright/test';
import {LIBRARY, env, libraryPath, showPath, stashPath} from '../helpers/env';

/** Whether exiftool can rewrite a photo's date (the sample photos carry EXIF only with it). */
function hasExiftool(): boolean {
  try {
    execFileSync('exiftool', ['-ver'], {stdio: 'ignore'});
    return true;
  } catch {
    return false;
  }
}

/** Rewrite a sample photo's EXIF date, as a camera or an editing app would. */
function setPhotoDate(rel: string, when: string): void {
  execFileSync('exiftool', ['-q', '-overwrite_original', `-DateTimeOriginal=${when}`, `-CreateDate=${when}`, stashPath(rel)]);
}

/** WCAG contrast ratio of two computed CSS colours (rgb() or oklch()). */
function contrast(a: string, b: string): number {
  const luminance = (color: string) => {
    const oklch = /oklch\(([\d.]+)(%?)/.exec(color);
    if (oklch?.[1]) {
      // Lightness cubed approximates relative luminance for the neutral zinc palette.
      const l = Number(oklch[1]) / (oklch[2] ? 100 : 1);
      return l ** 3;
    }
    const [r, g, bl] = (color.match(/[\d.]+/g) ?? ['0', '0', '0']).slice(0, 3).map(v => {
      const c = Number(v) / 255;
      return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * (r ?? 0) + 0.7152 * (g ?? 0) + 0.0722 * (bl ?? 0);
  };
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return ((hi ?? 0) + 0.05) / ((lo ?? 0) + 0.05);
}

test.use({storageState: async ({}, use) => use(env.ownerState)});

test('Home has a shelf for each library, in order, each with its icon', async ({page}) => {
  await page.goto('/');
  await expect(page.getByRole('heading', {level: 1, name: 'Home'})).toBeVisible();
  // The ambiguous Inbox file is waiting, and the sidebar says so.
  await expect(page.getByRole('link', {name: /^1 file needs a quick look before it is sorted\./})).toBeVisible();
  const nav = page.getByRole('navigation', {name: 'Main navigation'});
  await expect(nav.getByRole('link', {name: 'Inbox 1'})).toBeVisible();
  // Home, then every library with the icon its owner chose, then the fixed places with theirs.
  const icons: [RegExp, string][] = [
    [/^Home$/, 'house'], [/^Movies$/, 'film'], [/^Shows$/, 'tv'], [/^Home Videos$/, 'clapperboard'],
    [/^Photos$/, 'image'], [/^Other files$/, 'file-text'], [/^Inbox/, 'inbox'], [/^Downloads$/, 'arrow-down-to-line'],
    [/^Settings$/, 'settings'],
  ];
  for (const [name, icon] of icons) {
    await expect(nav.getByRole('link', {name}).locator(`svg.lucide-${icon}`)).toBeVisible();
  }
  // The brand (a link to Home) leads the sidebar; the places follow.
  const order = await nav.getByRole('link').filter({hasNotText: 'Popcorn'}).allInnerTexts();
  expect(order.map(text => text.trim().split('\n')[0])).toEqual([
    'Home', 'Movies', 'Shows', 'Home Videos', 'Photos', 'Other files', 'Inbox', 'Downloads', 'Settings',
  ]);

  // One shelf per library in the same order, its icon before its name, its count beside it.
  const names = await page.getByRole('main').getByRole('heading', {level: 2}).allInnerTexts();
  expect(names.filter(name => ['Movies', 'Shows', 'Home Videos', 'Photos'].includes(name))).toEqual([
    'Movies', 'Shows', 'Home Videos', 'Photos',
  ]);
  const movies = page.getByRole('region', {name: 'Movies', exact: true});
  await expect(movies.locator('svg.lucide-film').first()).toBeVisible();
  await expect(movies.getByText('4 videos')).toBeVisible();
  await expect(movies.getByRole('link', {name: /^Harbor Lights 2019/})).toBeVisible();
  const shows = page.getByRole('region', {name: 'Shows', exact: true});
  await expect(shows.getByText('2 shows, 4 episodes')).toBeVisible();
  await expect(shows.getByRole('link', {name: /^Quiet Meadow/})).toBeVisible();
  // Every library keeps its shelf (its own newest items, not the newest overall), photos included.
  await expect(page.getByRole('region', {name: 'Photos', exact: true}).getByRole('listitem')).toHaveCount(3);
  const homeVideos = page.getByRole('region', {name: 'Home Videos', exact: true});
  await expect(homeVideos.getByRole('listitem')).toHaveCount(2);
  await expect(homeVideos.getByRole('link', {name: /^IMG_1234 /})).toBeVisible();
  // Other files is not a library: a quiet heading with its count and the way in.
  const other = page.getByRole('region', {name: 'Other files', exact: true});
  await expect(other.getByText('2 files')).toBeVisible();

  // "See all" opens the library.
  await movies.getByRole('link', {name: 'See all'}).click();
  await expect(page).toHaveURL(new RegExp(`${libraryPath(LIBRARY.movies)}$`));
  await expect(page.getByRole('heading', {level: 1, name: 'Movies'})).toBeVisible();
  await expect(page.getByText('4 videos', {exact: true})).toBeVisible();
});

test('the old addresses lead to the library that takes that kind', async ({page}) => {
  const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  for (const [old, target] of [
    ['/movies?sort=title', `${libraryPath(LIBRARY.movies)}?sort=title`],
    ['/shows', libraryPath(LIBRARY.shows)],
    ['/shows/The%20Lantern%20Keepers', showPath(LIBRARY.shows, 'The Lantern Keepers')],
    ['/home-videos', libraryPath(LIBRARY.homeVideos)],
    ['/photos', libraryPath(LIBRARY.photos)],
    ['/settings/library', '/settings/libraries'],
  ] as const) {
    await page.goto(old);
    await expect(page, old).toHaveURL(new RegExp(`${escape(target)}$`));
    await expect(page.getByRole('heading', {level: 1}).first()).toBeVisible();
  }
  // Replaced, not pushed: Back does not bounce through the old address.
  await page.goto('/');
  await page.goto('/photos');
  await expect(page).toHaveURL(new RegExp(`${libraryPath(LIBRARY.photos)}$`));
  await page.goBack();
  await expect(page).toHaveURL(/\/$/);
});

test('Movies lists the films, with thumbnails, and opens a movie', async ({page}) => {
  await page.goto(libraryPath(LIBRARY.movies));
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

  // The subtitle does not promise an order the sort menu can change.
  await page.goto(`${libraryPath(LIBRARY.movies)}?sort=title`);
  await expect(grid.getByRole('link', {name: /^Glass Orchard/})).toBeVisible();
  await expect(page.getByText(/most recently added first/)).toHaveCount(0);

  await grid.getByRole('link', {name: /^Paper Boats 2011/}).click();
  await expect(page.getByRole('heading', {level: 1, name: 'Paper Boats'})).toBeVisible();
  await expect(page.getByRole('button', {name: 'Play'})).toBeVisible();
});

test('Shows lists each show and the show page lists its seasons', async ({page}) => {
  await page.goto(libraryPath(LIBRARY.shows));
  await expect(page.getByText('2 shows, 4 episodes', {exact: true})).toBeVisible();
  const shows = page.getByRole('list', {name: 'Shows'});
  await expect(shows.getByRole('link', {name: 'The Lantern Keepers 2 seasons · 3 episodes'})).toBeVisible();
  await expect(shows.getByRole('link', {name: /^Quiet Meadow 1 episode/})).toBeVisible();

  await shows.getByRole('link', {name: /^The Lantern Keepers/}).click();
  await expect(page).toHaveURL(new RegExp(`${showPath(LIBRARY.shows, 'The Lantern Keepers')}$`));
  // The back link names the library.
  await expect(page.getByRole('main').getByRole('link', {name: 'Shows', exact: true})).toBeVisible();
  await expect(page.getByRole('heading', {level: 1, name: 'The Lantern Keepers'})).toBeVisible();
  await expect(page.getByText('2 seasons · 3 episodes')).toBeVisible();
  const episodes = page.getByRole('region', {name: 'Episodes'});
  await expect(episodes.getByRole('tab', {name: 'Season 1'})).toBeVisible();
  await expect(episodes.getByRole('tab', {name: 'Season 2'})).toBeVisible();
  await expect(episodes.getByRole('list', {name: 'Season 1'}).getByRole('listitem')).toHaveCount(2);

  await episodes.getByRole('tab', {name: 'Season 2'}).click();
  const season2 = episodes.getByRole('list', {name: 'Season 2'});
  await expect(season2.getByRole('listitem')).toHaveCount(1);
  // Inside the show an untitled episode is "Episode 1", not the show name and code again.
  await expect(season2.getByRole('link', {name: /^Episode 1\b/})).toBeVisible();
});

test('a show has the same picture on Home, on Shows and on its own page', async ({page}) => {
  const show = `/api/libraries/${LIBRARY.shows}/shows/The%20Lantern%20Keepers`;
  // Wait until every episode has a thumbnail, so the poster choice is settled.
  await expect
    .poll(async () => {
      const detail = (await (await page.request.get(show)).json()) as {seasons: {episodes: {thumb: string | null}[]}[]};
      return detail.seasons.flatMap(season => season.episodes).every(episode => episode.thumb);
    }, {timeout: 20_000})
    .toBe(true);
  const poster = ((await (await page.request.get(show)).json()) as {poster_item_id: string}).poster_item_id;
  expect(poster).toBeTruthy();
  const pictured = async (image: Locator) => /\/api\/items\/([^/]+)\/thumb/.exec((await image.getAttribute('src')) ?? '')?.[1];

  await page.goto('/');
  const shelf = page.getByRole('region', {name: 'Shows', exact: true});
  expect(await pictured(shelf.getByRole('link', {name: /^The Lantern Keepers/}).locator('img'))).toBe(poster);

  await page.goto(libraryPath(LIBRARY.shows));
  const grid = page.getByRole('list', {name: 'Shows'});
  expect(await pictured(grid.getByRole('link', {name: /^The Lantern Keepers/}).locator('img'))).toBe(poster);

  await page.goto(showPath(LIBRARY.shows, 'The Lantern Keepers'));
  await expect(page.getByRole('heading', {level: 1, name: 'The Lantern Keepers'})).toBeVisible();
  // The poster is the first picture on the page, ahead of the episode frames.
  expect(await pictured(page.getByRole('main').locator('img').first())).toBe(poster);
});

test('Home Videos shows the phone clip with the date it was filmed', async ({page}) => {
  await page.goto(libraryPath(LIBRARY.homeVideos));
  const grid = page.getByRole('list', {name: 'Home Videos'});
  // Dated from the QuickTime creation time, not the file's modified time.
  await expect(grid.getByRole('link', {name: /^IMG_1234 Jun 13, 2024/})).toBeVisible();
  await expect(grid.getByRole('link', {name: /^IMG_0042 Jul 14, 2023/})).toBeVisible();

  await grid.getByRole('link', {name: /^IMG_1234/}).click();
  await expect(page.getByRole('heading', {level: 1, name: 'IMG_1234'})).toBeVisible();
  await expect(page.getByText('Apple iPhone 15').first()).toBeVisible();
});

test('the Photos timeline opens the viewer and the arrow keys move between photos', async ({page}) => {
  await page.goto(libraryPath(LIBRARY.photos));
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
  // The viewer is a black stage in light mode too: its buttons are light on dark, not
  // white-on-white (the light-dark() tokens must follow the stage's color-scheme).
  const download = viewer.getByRole('link', {name: 'Download original'});
  const colors = await download.evaluate((element: HTMLElement) => {
    const style = getComputedStyle(element);
    return {background: style.backgroundColor, text: style.color};
  });
  expect(contrast(colors.text, colors.background), JSON.stringify(colors)).toBeGreaterThan(4.5);

  // The arrows and close button sit on a dark disc that reads on light and dark photos,
  // with 44px targets, and the photo keeps the same inset at the top as at the sides.
  for (const name of ['Previous', 'Next', 'Close']) {
    const control = await viewer.getByRole('button', {name}).evaluate((element: HTMLElement) => {
      const box = element.getBoundingClientRect();
      return {width: box.width, height: box.height, background: getComputedStyle(element).backgroundColor};
    });
    expect(control.width, name).toBeGreaterThanOrEqual(44);
    expect(control.height, name).toBeGreaterThanOrEqual(44);
    expect(control.background, name).not.toBe('rgba(0, 0, 0, 0)');
  }
  const inset = await viewer.evaluate((dialog: HTMLElement) => {
    const style = getComputedStyle(dialog.firstElementChild as HTMLElement);
    return {top: parseFloat(style.paddingTop), side: parseFloat(style.paddingLeft)};
  });
  expect(inset.top).toBeGreaterThanOrEqual(56);
  expect(inset.top).toBe(inset.side);

  await page.keyboard.press('ArrowRight');
  await expect(viewer.locator('img[alt^="Photo taken"]').first()).not.toHaveAttribute('alt', first ?? '');
  const second = await viewer.locator('img[alt^="Photo taken"]').first().getAttribute('alt');
  expect(second).toContain('May 21, 2022');

  await page.keyboard.press('ArrowLeft');
  await expect(viewer.locator('img[alt^="Photo taken"]').first()).toHaveAttribute('alt', first ?? '');

  await page.keyboard.press('Escape');
  await expect(viewer).toBeHidden();
});

test('Other files lists the audio file and the document, each with its own icon', async ({page}) => {
  await page.goto('/other');
  await expect(page.getByRole('heading', {level: 1, name: 'Other files'})).toBeVisible();
  await expect(page.getByText('Each file is seen by the person who added it and the owners.', {exact: false})).toBeVisible();
  const grid = page.getByRole('list', {name: 'Other files'});
  await expect(grid.getByRole('link', {name: 'Morning Notes MP3 · 3 s'})).toBeVisible();
  // Files without a picture show what they are, not a film or a library icon.
  await expect(grid.getByRole('link', {name: /^Recipe Card PDF/}).locator('svg.lucide-file-text')).toBeVisible();
  await expect(grid.getByRole('link', {name: /^Morning Notes MP3/}).locator('svg.lucide-music')).toBeVisible();
});

test("an item's page marks its library in the sidebar, and a show's page marks the show's library", async ({page}) => {
  const nav = page.getByRole('navigation', {name: 'Main navigation'});
  await page.goto(libraryPath(LIBRARY.movies));
  await page.getByRole('list', {name: 'Movies'}).getByRole('link', {name: /^Paper Boats 2011/}).click();
  await expect(page.getByRole('heading', {level: 1, name: 'Paper Boats'})).toBeVisible();
  // /item/:id is not under the library's address; the page names its library for the sidebar.
  await expect(nav.getByRole('link', {name: 'Movies', exact: true})).toHaveAttribute('aria-current', 'page');
  await expect(nav.locator('[aria-current="page"]')).toHaveCount(1);
  // Leaving the item page follows the address again.
  await page.goto(showPath(LIBRARY.shows, 'The Lantern Keepers'));
  await expect(nav.getByRole('link', {name: 'Shows', exact: true})).toHaveAttribute('aria-current', 'page');
  await expect(nav.locator('[aria-current="page"]')).toHaveCount(1);
});

test.describe('for a viewer west of UTC', () => {
  test.use({timezoneId: 'America/Los_Angeles'});

  test("months and capture times read as the camera's clock wrote them", async ({page}) => {
    await page.goto(libraryPath(LIBRARY.photos));
    // The server's month as written: "2022-05" is May, not April, in Los Angeles.
    const month = page.getByRole('region', {name: 'May 2022'});
    await expect(month.getByRole('heading', {name: 'May 2022'})).toBeVisible();
    await expect(page.getByRole('heading', {name: 'April 2022'})).toHaveCount(0);
    const tiles = month.getByRole('list', {name: 'May 2022'}).getByRole('link');
    await expect(tiles.first()).toHaveAccessibleName('Photo from May 22, 2022, Fujifilm X-T30');
    // The camera wrote 09:12:10 with no offset: that is the time shown, wherever the viewer is.
    await tiles.first().click();
    const viewer = page.getByRole('dialog');
    await expect(viewer.getByText('May 22, 2022, 9:12 AM')).toBeVisible();
  });
});

test('a photo whose date changes moves to its month on the open page', async ({page}) => {
  test.skip(!hasExiftool(), 'the sample photos carry EXIF dates only when exiftool is installed');
  const rel = 'Photos/2022/05/DSC_0101.jpg';
  await page.goto(libraryPath(LIBRARY.photos));
  const may = page.getByRole('region', {name: 'May 2022'});
  await expect(may.getByText('3 photos')).toBeVisible();
  await expect(page.getByRole('region', {name: 'April 2022'})).toHaveCount(0);
  // A reload would forget this.
  await page.evaluate(() => Object.assign(window, {stillOpen: true}));
  try {
    // A minute before May: the server reads the new EXIF and the open page follows.
    setPhotoDate(rel, '2022:04:30 23:59:00');
    const april = page.getByRole('region', {name: 'April 2022'});
    await expect(april.getByRole('link', {name: 'Photo from Apr 30, 2022, Fujifilm X-T30'})).toBeVisible({
      timeout: 20_000,
    });
    await expect(april.getByText('1 photo', {exact: true})).toBeVisible();
    await expect(may.getByText('2 photos')).toBeVisible();
    await expect(may.getByRole('list', {name: 'May 2022'}).getByRole('link')).toHaveCount(2);
    // Months stay newest first.
    await expect(page.locator('section[aria-labelledby^="month-"] h2')).toHaveText(['May 2022', 'April 2022']);
    expect(await page.evaluate(() => (window as unknown as {stillOpen?: boolean}).stillOpen)).toBe(true);
  } finally {
    setPhotoDate(rel, '2022:05:21 14:03:00');
  }
  // And back, in its place: the oldest of May, after 14:05:30 on May 21.
  await expect(may.getByText('3 photos')).toBeVisible({timeout: 20_000});
  await expect(page.getByRole('region', {name: 'April 2022'})).toHaveCount(0);
  const found = await page.request.get(`/api/items?library_id=${LIBRARY.photos}&q=DSC_0101`);
  const [moved] = ((await found.json()) as {items: {id: string}[]}).items;
  const tiles = may.getByRole('list', {name: 'May 2022'}).getByRole('link');
  await expect(tiles).toHaveCount(3);
  await expect(tiles.last()).toHaveAttribute('href', new RegExp(`${moved?.id ?? 'missing'}$`));
  expect(await page.evaluate(() => (window as unknown as {stillOpen?: boolean}).stillOpen)).toBe(true);
});

/**
 * Runs in the page: the contrast of an unchecked checkbox's or radio's edge (its border)
 * against the page and against the panel it sits on, from the colours the browser
 * paints, read back through a canvas (sRGB). Self-contained, so Playwright can send it.
 */
function indicatorEdge(indicator: Element): {page: number; panel: number} {
  const ctx = document.createElement('canvas').getContext('2d', {willReadFrequently: true}) as CanvasRenderingContext2D;
  const rgb = (color: string) => {
    ctx.clearRect(0, 0, 1, 1);
    ctx.fillStyle = color;
    ctx.fillRect(0, 0, 1, 1);
    return Array.from(ctx.getImageData(0, 0, 1, 1).data.slice(0, 4));
  };
  const lum = (c: number[]) =>
    c
      .slice(0, 3)
      .map(v => v / 255)
      .map(v => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4))
      .reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i]!, 0);
  const ratio = (a: number[], b: number[]) => {
    const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x) as [number, number];
    return (hi + 0.05) / (lo + 0.05);
  };
  const edge = rgb(getComputedStyle(indicator).borderTopColor);
  const page = rgb(getComputedStyle(document.body).backgroundColor);
  // The first opaque background behind the control: the panel it sits on, else the page.
  let panel = page;
  for (let node = indicator.parentElement; node && node !== document.body; node = node.parentElement) {
    const fill = rgb(getComputedStyle(node).backgroundColor);
    if (fill[3] === 255) {
      panel = fill;
      break;
    }
  }
  return {page: ratio(edge, page), panel: ratio(edge, panel)};
}

test('off and unchosen controls read in light and dark: 3:1 or more', async ({page}) => {
  // Shown off without saving anything: this browser's settings answer says auto-sort is off.
  await page.route('**/api/settings', async route => {
    if (route.request().method() !== 'GET') return route.continue();
    const response = await route.fetch();
    await route.fulfill({response, json: {...(await response.json()), auto_sort: false}});
  });
  for (const colorScheme of ['light', 'dark'] as const) {
    await page.emulateMedia({colorScheme});
    await page.goto('/settings/libraries');
    await expect(page.getByRole('switch', {name: /^Sort new files automatically/})).not.toBeChecked();
    // The colours the browser paints, read back through a canvas (sRGB), against the page.
    const ratios = await page.locator('.astryx-switch').first().evaluate(track => {
      const ctx = document.createElement('canvas').getContext('2d', {willReadFrequently: true}) as CanvasRenderingContext2D;
      const rgb = (color: string) => {
        ctx.clearRect(0, 0, 1, 1);
        ctx.fillStyle = color;
        ctx.fillRect(0, 0, 1, 1);
        return Array.from(ctx.getImageData(0, 0, 1, 1).data.slice(0, 3));
      };
      const lum = (c: number[]) =>
        c.map(v => v / 255).map(v => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4)).reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i]!, 0);
      const ratio = (a: number[], b: number[]) => {
        const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x) as [number, number];
        return (hi + 0.05) / (lo + 0.05);
      };
      const style = getComputedStyle(track);
      const fill = rgb(style.backgroundColor);
      // The control's edge: its ring when it has one, else the track itself.
      const ringColor = /(oklch|rgba?|color)\([^)]*\)/.exec(style.boxShadow)?.[0];
      const ring = ringColor ? rgb(ringColor) : fill;
      const knob = rgb(getComputedStyle(track.firstElementChild as Element).backgroundColor);
      const page = rgb(getComputedStyle(document.body).backgroundColor);
      return {ring: ratio(ring, page), knob: ratio(knob, fill), knobLighter: lum(knob) > lum(fill)};
    });
    expect(ratios.ring, `${colorScheme}: the edge against the page`).toBeGreaterThanOrEqual(3);
    expect(ratios.knob, `${colorScheme}: knob against its track`).toBeGreaterThanOrEqual(3);
    if (colorScheme === 'dark') expect(ratios.knobLighter, 'dark: the knob is lighter than its track').toBe(true);

    // An unchosen radio and an unchecked checkbox beside it have the same edge, as strong.
    await page.goto('/settings/libraries/new');
    const radio = page.getByRole('radio', {checked: false}).first();
    const checkbox = page.getByRole('checkbox', {checked: false}).first();
    await expect(radio).toBeAttached();
    await expect(checkbox).toBeAttached();
    for (const [name, control, indicator] of [
      ['radio', radio, '.astryx-radio-indicator'],
      ['checkbox', checkbox, '.astryx-checkbox-indicator'],
    ] as const) {
      const edge = await control.locator('xpath=..').locator(indicator).evaluate(indicatorEdge);
      expect(edge.page, `${colorScheme}: an unchecked ${name} against the page`).toBeGreaterThanOrEqual(3);
      expect(edge.panel, `${colorScheme}: an unchecked ${name} against its panel`).toBeGreaterThanOrEqual(3);
    }
  }
});

test.describe('on a phone', () => {
  test.use({viewport: {width: 390, height: 844}, isMobile: true, hasTouch: true});

  test('every main page fits the screen without sideways scrolling', async ({page}) => {
    await page.goto('/');
    // Home, the first three libraries and Downloads stay one tap away.
    const quick = page.getByRole('navigation', {name: 'Quick links'});
    await expect(quick.getByRole('link')).toHaveCount(5);
    for (const [label, icon] of [['Home', 'house'], ['Movies', 'film'], ['Shows', 'tv'], ['Home Videos', 'clapperboard'], ['Downloads', 'arrow-down-to-line']]) {
      // The same icon as in the sidebar, above the label.
      await expect(quick.getByRole('link', {name: label, exact: true}).locator(`svg.lucide-${icon}`)).toBeVisible();
    }
    for (const path of [
      '/',
      libraryPath(LIBRARY.movies),
      libraryPath(LIBRARY.shows),
      showPath(LIBRARY.shows, 'The Lantern Keepers'),
      libraryPath(LIBRARY.homeVideos),
      libraryPath(LIBRARY.photos),
      '/other',
      '/inbox',
      '/downloads',
      '/settings/vpn',
      '/settings/security',
      '/settings/libraries',
      '/settings/libraries/new',
      `/settings/libraries/${LIBRARY.movies}`,
      '/pair',
    ]) {
      await page.goto(path);
      await expect(page.getByRole('heading', {level: 1}).first()).toBeVisible();
      // Let lazy content (thumbnails, lists) settle; the event stream keeps the network busy.
      await page.waitForTimeout(400);
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
      expect(overflow, `${path} scrolls sideways by ${overflow}px`).toBeLessThanOrEqual(0);
    }
  });

  test('a date stays whole in a sentence, and a folder name is shown whole', async ({page, baseURL}) => {
    const origin = {Origin: baseURL ?? ''};
    const created = (await (
      await page.request.post('/api/tokens', {data: {name: 'Living room TV', scopes: ['library:read']}, headers: origin})
    ).json()) as {id: string};
    try {
      await page.goto('/settings/devices');
      const row = page.getByRole('listitem').filter({hasText: 'Living room TV'});
      // "Added Oct 14, 2026." breaks before or after the date, never inside it.
      const date = row.getByText(/^[A-Z][a-z]{2} \d{1,2}, \d{4}$/);
      await expect(date).toBeVisible();
      expect(await date.evaluate(el => el.getClientRects().length)).toBe(1);
    } finally {
      await page.request.delete(`/api/tokens/${created.id}`, {headers: origin});
    }
    // A name longer than the row, beside the sample's own folder.
    const long = "Grandma's Slides from the 1970s";
    await page.route('**/api/libraries/folders', async route => {
      const response = await route.fetch();
      const body = (await response.json()) as {folders: {name: string}[]};
      await route.fulfill({response, json: {...body, folders: [...body.folders, {...body.folders[0], name: long}]}});
    });
    await page.goto('/settings/libraries');
    const region = page.getByRole('region', {name: 'Folders not in a library'});
    const folder = region.getByText('Family 2019/');
    await expect(folder).toBeVisible();
    // A short name keeps one line.
    const lines = await folder.evaluate(el => el.getBoundingClientRect().height / parseFloat(getComputedStyle(el).lineHeight));
    expect(lines).toBeLessThan(1.5);
    // A long one wraps at its spaces and is never cut off: nothing a phone could not show.
    const longName = region.getByText(`${long}/`);
    await expect(longName).toBeVisible();
    const cut = await longName.evaluate(el => ({
      overflow: el.scrollWidth - el.clientWidth,
      ellipsis: getComputedStyle(el).textOverflow === 'ellipsis' && getComputedStyle(el).whiteSpace === 'nowrap',
    }));
    expect(cut).toEqual({overflow: 0, ellipsis: false});
    // The icon stays with the name and its count; the button sits under all of them.
    const row = region.getByRole('listitem').filter({hasText: 'Family 2019/'});
    const icon = await row.locator('svg').first().boundingBox();
    const name = await folder.boundingBox();
    const button = await row.getByRole('link', {name: /^Make a library from/}).boundingBox();
    expect(icon && name && button).toBeTruthy();
    if (icon && name && button) {
      expect(Math.abs(icon.y + icon.height / 2 - (name.y + name.height))).toBeLessThan(12);
      expect(icon.y + icon.height).toBeLessThanOrEqual(button.y);
    }
  });

  test('Settings offers every section without sideways scrolling, each at its own address', async ({page}) => {
    await page.goto('/settings/vpn');
    const picker = page.getByRole('navigation', {name: 'Settings sections'}).getByRole('combobox', {name: 'Section'});
    await expect(picker).toHaveText('VPN');
    await picker.click();
    for (const name of ['VPN', 'Devices', 'People', 'Security', 'Libraries', 'System']) {
      await expect(page.getByRole('option', {name})).toBeVisible();
    }
    await page.getByRole('option', {name: 'System'}).click();
    await expect(page).toHaveURL(/\/settings\/system$/);
    await expect(picker).toHaveText('System');
    await page.goBack();
    await expect(page).toHaveURL(/\/settings\/vpn$/);
  });
});
