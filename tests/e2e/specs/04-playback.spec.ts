/**
 * Playback: direct play of browser-friendly files, HLS transcoding for the
 * rest, and saved progress feeding "Continue watching".
 *
 * Playwright's Chromium is an open-source build without H.264/AAC decoders.
 * Where a check needs the browser to decode H.264 (an H.264 file, or any HLS
 * stream, which is always H.264), the test proves everything up to decoding:
 * the server's decision, the bytes or segments the player fetched, and (for
 * HLS) that ffprobe reads the served segment as H.264/AAC. Set E2E_CHROMIUM to a
 * Chrome binary with proprietary codecs to run the full decode checks too.
 * The VP9/Opus MP4 in the sample stash plays fully in every Chromium.
 */
import {execFileSync} from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import {expect, test, type Page} from '@playwright/test';
import {env, findItem} from '../helpers/env';

test.use({storageState: async ({}, use) => use(env.ownerState)});

async function canDecodeH264(page: Page): Promise<boolean> {
  return page.evaluate(
    () =>
      document.createElement('video').canPlayType('video/mp4; codecs="avc1.42E01E, mp4a.40.2"') !== '' &&
      typeof MediaSource !== 'undefined' &&
      MediaSource.isTypeSupported('video/mp4; codecs="avc1.42E01E, mp4a.40.2"'),
  );
}

function video(page: Page) {
  return page.locator('video');
}

async function currentTime(page: Page): Promise<number> {
  return video(page).evaluate((element: HTMLVideoElement) => element.currentTime);
}

async function openAndPlay(page: Page, id: string) {
  await page.goto(`/item/${id}`);
  const decision = page.waitForResponse(
    response => response.url().endsWith(`/api/items/${id}/playback`) && response.request().method() === 'POST',
  );
  await page.getByRole('button', {name: /^(Play|Resume from)/}).first().click();
  const response = await decision;
  expect(response.status()).toBe(200);
  return (await response.json()) as {mode: 'direct' | 'hls'; url: string; session_id?: string};
}

test('a browser-friendly MP4 plays directly from the original file', async ({page}) => {
  const paperBoats = await findItem(page.request, 'Paper Boats');
  await page.goto('/');
  const h264 = await canDecodeH264(page);

  const fileResponse = page.waitForResponse(response => response.url().includes(`/api/items/${paperBoats.id}/file`));
  const decision = await openAndPlay(page, paperBoats.id);
  expect(decision.mode).toBe('direct');
  expect(decision.url).toContain(`/api/items/${paperBoats.id}/file`);
  // The browser streams the original with range requests.
  const file = await fileResponse;
  expect([200, 206]).toContain(file.status());
  expect(file.headers()['content-type']).toBe('video/mp4');
  expect(file.headers()['accept-ranges']).toBe('bytes');
  const range = await page.request.get(decision.url, {headers: {Range: 'bytes=100-199'}});
  expect(range.status()).toBe(206);
  expect(range.headers()['content-range']).toMatch(/^bytes 100-199\/\d+$/);
  expect((await range.body()).length).toBe(100);

  if (h264) {
    await expect.poll(() => currentTime(page), {timeout: 15_000}).toBeGreaterThan(0.5);
  } else {
    test.info().annotations.push({
      type: 'note',
      description: 'This Chromium cannot decode H.264; the H.264 MP4 was checked up to the bytes served.',
    });
    // The player says so plainly instead of spinning forever.
    await expect(page.getByText('This video could not be played in this browser.', {exact: false})).toBeVisible({
      timeout: 20_000,
    });
  }

  // A VP9/Opus MP4 is direct-playable everywhere: it really plays.
  const tin = await findItem(page.request, 'Tin Lighthouse');
  const tinFile = page.waitForResponse(response => response.url().includes(`/api/items/${tin.id}/file`));
  const tinDecision = await openAndPlay(page, tin.id);
  expect(tinDecision.mode).toBe('direct');
  expect([200, 206]).toContain((await tinFile).status());
  await expect.poll(() => currentTime(page), {timeout: 15_000}).toBeGreaterThan(1);
  expect(await video(page).evaluate((element: HTMLVideoElement) => element.videoWidth)).toBe(320);
});

test('an HEVC/AC-3 MKV is transcoded to HLS on the fly', async ({page}) => {
  const orchard = await findItem(page.request, 'Glass Orchard');
  await page.goto('/');
  const h264 = await canDecodeH264(page);

  // The player ends the server's stream session when it stops (a playback error, or leaving the page).
  const ended = page.waitForRequest(
    request => /\/api\/stream\/[0-9a-f]{32}$/.test(request.url()) && request.method() === 'DELETE',
    {timeout: 50_000},
  );
  const playlist = page.waitForResponse(response => /\/api\/stream\/[0-9a-f]{32}\/index\.m3u8$/.test(response.url()));
  const segment = page.waitForResponse(response => /\/api\/stream\/[0-9a-f]{32}\/\d+\.ts$/.test(response.url()), {
    timeout: 40_000,
  });
  const decision = await openAndPlay(page, orchard.id);
  expect(decision.mode).toBe('hls');
  expect(decision.session_id).toMatch(/^[0-9a-f]{32}$/);

  const playlistResponse = await playlist;
  expect(playlistResponse.status()).toBe(200);
  const m3u8 = await playlistResponse.text();
  expect(m3u8).toContain('#EXTM3U');
  expect(m3u8).toContain('#EXT-X-ENDLIST');
  expect(m3u8).toMatch(/#EXTINF:6(\.0+)?,/);

  const segmentResponse = await segment;
  expect(segmentResponse.status()).toBe(200);
  const body = await segmentResponse.body();
  expect(body.length).toBeGreaterThan(1000);
  // MPEG-TS packets start with the 0x47 sync byte every 188 bytes.
  expect(body[0]).toBe(0x47);
  expect(body[188]).toBe(0x47);

  if (h264) {
    await expect.poll(() => currentTime(page), {timeout: 30_000}).toBeGreaterThan(0.5);
  } else {
    test.info().annotations.push({
      type: 'note',
      description: 'This Chromium cannot decode H.264; the HLS segment was verified with ffprobe instead.',
    });
    const file = path.join(test.info().outputDir, 'segment.ts');
    fs.mkdirSync(path.dirname(file), {recursive: true});
    fs.writeFileSync(file, body);
    const probe = JSON.parse(
      execFileSync('ffprobe', ['-v', 'error', '-show_entries', 'stream=codec_name,width,height', '-of', 'json', file], {
        encoding: 'utf8',
      }),
    ) as {streams: {codec_name: string; width?: number; height?: number}[]};
    expect(probe.streams.map(stream => stream.codec_name).sort()).toEqual(['aac', 'h264']);
    expect(probe.streams.find(stream => stream.codec_name === 'h264')).toMatchObject({width: 320, height: 180});
    await expect(page.getByText('This video could not be played in this browser.', {exact: false})).toBeVisible({
      timeout: 20_000,
    });
  }

  await page.goto('/');
  expect((await ended).url()).toContain(`/api/stream/${decision.session_id}`);
});

test('progress is saved while watching and Home offers to continue', async ({page}) => {
  const tin = await findItem(page.request, 'Tin Lighthouse');
  const decision = await openAndPlay(page, tin.id);
  expect(decision.mode).toBe('direct');
  await expect.poll(() => currentTime(page), {timeout: 15_000}).toBeGreaterThan(0.2);

  // Skip ahead past the 30 s mark, watch a little, then pause: pausing saves progress.
  await video(page).evaluate((element: HTMLVideoElement) => {
    element.currentTime = 31;
  });
  await expect.poll(() => currentTime(page), {timeout: 15_000}).toBeGreaterThan(32);
  const saved = page.waitForRequest(
    request => request.url().endsWith(`/api/items/${tin.id}/progress`) && request.method() === 'PUT',
  );
  await video(page).evaluate((element: HTMLVideoElement) => element.pause());
  const body = (await saved).postDataJSON() as {position_s: number; duration_s: number};
  expect(body.position_s).toBeGreaterThan(32);
  expect(body.duration_s).toBeCloseTo(45, 0);

  await page.goto('/');
  const shelf = page.getByRole('region', {name: 'Continue watching'});
  await expect(shelf).toBeVisible();
  const tile = shelf.getByRole('link', {name: /^Tin Lighthouse/});
  await expect(tile).toBeVisible();

  // Opening it from the shelf offers to resume where it stopped (a short clip too: 33 s of 45).
  await tile.click();
  const resume = page.getByRole('button', {name: /^Resume from 0:3\d/});
  await expect(resume).toBeVisible();
  await resume.click();
  await expect.poll(() => currentTime(page), {timeout: 15_000}).toBeGreaterThan(33);
});

test('a show offers Resume only when the player will pick up part-way', async ({page, baseURL}) => {
  // A few seconds into the first episode: the player starts it over (under 15 s), so the
  // show page must not promise to resume it.
  const first = await findItem(page.request, 'The Lantern Keepers S01E01');
  const saved = await page.request.put(`/api/items/${first.id}/progress`, {
    data: {position_s: 2, duration_s: 4},
    headers: {Origin: baseURL ?? ''},
  });
  expect(saved.status()).toBe(204);
  await page.goto('/shows/The%20Lantern%20Keepers');
  await expect(page.getByRole('button', {name: 'Play S01E01'})).toBeVisible();
  await expect(page.getByRole('button', {name: /^Resume/})).toHaveCount(0);
  // The line under the button names the episode without promising time left.
  await expect(page.getByText('The Lantern Keepers S01E01', {exact: true}).first()).toBeVisible();
  await expect(page.getByText(/S01E01(,| ·) .*left$/)).toHaveCount(0);
  // The episode's own page agrees: it plays from the start.
  await page.goto(`/item/${first.id}`);
  await expect(page.getByRole('button', {name: 'Play', exact: true})).toBeVisible();
});

test('HLS segments line up and a stream can start part way through', async ({page, baseURL}) => {
  const orchard = await findItem(page.request, 'Glass Orchard');
  const caps = {video_codecs: ['h264'], audio_codecs: ['aac'], hdr: false};
  const origin = {Origin: baseURL ?? ''};
  const probeSegment = (body: Buffer, name: string) => {
    const file = path.join(test.info().outputDir, name);
    fs.mkdirSync(path.dirname(file), {recursive: true});
    fs.writeFileSync(file, body);
    const out = JSON.parse(
      execFileSync('ffprobe', ['-v', 'error', '-show_entries', 'format=start_time,duration:stream=codec_name', '-of', 'json', file], {
        encoding: 'utf8',
      }),
    ) as {format: {start_time: string; duration: string}; streams: {codec_name: string}[]};
    return {start: Number(out.format.start_time), duration: Number(out.format.duration), codecs: out.streams.map(s => s.codec_name).sort()};
  };

  // From the start: two 6 s segments for the 8 s clip, timestamps at 10 + 6n.
  const started = await page.request.post(`/api/items/${orchard.id}/playback`, {data: caps, headers: origin});
  const session = (await started.json()) as {mode: string; url: string; session_id: string};
  expect(session.mode).toBe('hls');
  const playlist = await (await page.request.get(session.url)).text();
  const segments = playlist.split('\n').filter(line => line && !line.startsWith('#'));
  expect(segments).toEqual(['0.ts', '1.ts']);
  for (const [index, name] of segments.entries()) {
    const response = await page.request.get(new URL(name, new URL(session.url, baseURL)).toString());
    expect(response.status(), name).toBe(200);
    const info = probeSegment(await response.body(), `full-${name}`);
    expect(info.codecs).toEqual(['aac', 'h264']);
    expect(info.start).toBeCloseTo(10 + 6 * index, 0);
  }
  expect((await page.request.delete(`/api/stream/${session.session_id}`, {headers: origin})).status()).toBe(204);
  expect((await page.request.get(session.url)).status()).toBe(404);

  // Resuming at 6 s starts ffmpeg at the second segment; it still lines up with the first run.
  const resumed = await page.request.post(`/api/items/${orchard.id}/playback`, {data: {...caps, start_s: 6}, headers: origin});
  const second = (await resumed.json()) as {mode: string; url: string; session_id: string};
  expect(second.mode).toBe('hls');
  const segment = await page.request.get(new URL('1.ts', new URL(second.url, baseURL)).toString());
  expect(segment.status()).toBe(200);
  const info = probeSegment(await segment.body(), 'resumed-1.ts');
  expect(info.codecs).toEqual(['aac', 'h264']);
  expect(info.start).toBeCloseTo(16, 0);
  expect(info.duration).toBeGreaterThan(1);
  await page.request.delete(`/api/stream/${second.session_id}`, {headers: origin});
});
