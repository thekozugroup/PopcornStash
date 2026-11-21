import {expect, test, type Locator} from '@playwright/test';
import {env, findItem} from '../helpers/env';

test.use({storageState: async ({}, use) => use(env.ownerState)});

async function center(locator: Locator) {
  const box = await locator.boundingBox();
  if (!box) throw new Error('Expected a visible player control');
  return {x: box.x + box.width / 2, y: box.y + box.height / 2};
}

for (const width of [1180, 390]) {
  test.describe(`volume hover at ${width}px`, () => {
    test.use({viewport: {width, height: 844}, hasTouch: width === 390});

    test('keeps the panel reachable and restores ordinary timeline hover', async ({page}) => {
      const item = await findItem(page.request, 'Tin Lighthouse');
      await page.goto(`/item/${item.id}`);
      await page.getByRole('button', {name: /^(Play|Resume from)/}).first().click();
      const video = page.locator('video');
      await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.readyState)).toBeGreaterThanOrEqual(2);
      await video.evaluate((v: HTMLVideoElement) => v.pause());
      const controls = page.getByRole('group', {name: 'Playback controls'});
      await controls.scrollIntoViewIfNeeded();
      const mute = controls.getByRole('button', {name: 'Mute', exact: true});
      const volume = controls.getByRole('slider', {name: 'Volume', exact: true});
      const track = await center(controls.locator('[data-player-timeline] .astryx-slider-control'));
      const preview = controls.getByTestId('seek-preview');
      await mute.hover();
      await expect(volume).toBeVisible();

      if (width === 390) {
        const button = await center(mute);
        await page.mouse.move(button.x, track.y, {steps: 8});
        await expect(volume).toBeVisible();
        await expect(preview).toHaveCount(0);
        const panel = await center(controls.getByRole('group', {name: 'Volume controls'}));
        await page.mouse.move(button.x, panel.y, {steps: 8});
        await expect(volume).toBeVisible();
        await page.mouse.move(button.x, track.y, {steps: 8});
      }
      await page.mouse.move(track.x, track.y, {steps: 6});
      await expect(preview).toBeVisible();
      await expect(volume).toHaveCount(0);

      await mute.focus();
      await expect(volume).toBeVisible();
      await page.keyboard.press('Shift+Tab');
      await expect(volume).toBeFocused();
      await page.keyboard.press('ArrowLeft');
      await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.volume)).toBeLessThan(1);
      if (width === 390) {
        const button = await center(mute);
        await page.touchscreen.tap(button.x, track.y);
        await expect(volume).toHaveCount(0);
        await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime / v.duration)).toBeGreaterThan(.5);
      }
    });
  });
}
