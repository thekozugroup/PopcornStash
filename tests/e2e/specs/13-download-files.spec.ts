import {expect, test} from '@playwright/test';
import {env} from '../helpers/env';

test.use({storageState: async ({}, use) => use(env.ownerState)});

test('review choices survive polling, Skip affects one file, and Finish requires confirmation', async ({page}) => {
  const id = 'transfer-fixture';
  const download = {
    id, kind: 'torrent', name: 'Transfer fixture', source: '', state: 'paused', error: 'Ready to choose files.' as string | null,
    progress: 0, total_bytes: 300, done_bytes: 0, down_bps: 0, up_bps: 0, peers: 0, eta_s: null,
    created_at: 1, completed_at: null, item_ids: [], destinations: [],
  };
  const files = ['a.bin', 'b.bin', 'c.bin'].map((path, index) => ({
    index, path, kind: 'other', size: 100, selected: true, verified_bytes: 0, complete: false,
  }));
  let review = true;
  let dropProgress = false;
  let failRead = false;
  let reads = 0;
  let finishAttempts = 0;
  const mutations: string[] = [];
  await page.route('**/api/downloads?**', route => {
    const active = new URL(route.request().url()).searchParams.get('state') === 'active';
    return route.fulfill({json: {items: active === (download.state !== 'completed') ? [download] : []}});
  });
  await page.route(`**/api/downloads/${id}/**`, async route => {
    const path = new URL(route.request().url()).pathname.slice(`/api/downloads/${id}/`.length);
    if (route.request().method() === 'GET' && path === 'files') {
      reads++;
      if (failRead) return route.fulfill({status: 503});
      const snapshot = dropProgress ? files.map(file => file.index === 1 ? {...file, verified_bytes: null} : file) : files;
      return route.fulfill({json: {ready: true, review_required: review, files: snapshot}});
    }
    expect(route.request().method()).toBe('POST');
    mutations.push(path);
    if (path === 'start') {
      expect(route.request().postDataJSON()).toEqual({indices: [0, 1, 2]});
      review = false;
      download.state = 'downloading';
      download.error = null;
      Object.assign(files[0]!, {complete: true, verified_bytes: 100});
      files[1]!.verified_bytes = 15;
      return route.fulfill({json: download});
    }
    if (path === 'files/2/skip') {
      files[2]!.selected = false;
      return route.fulfill({status: 204});
    }
    expect(path).toBe('finish-completed');
    const expected = route.request().postDataJSON() as {expected_selected: number[]; expected_complete: number[]};
    // Change the engine snapshot after the browser's final GET on each attempt.
    finishAttempts++;
    if (finishAttempts === 1) {
      expect(expected).toEqual({expected_selected: [0, 1], expected_complete: [0]});
      files[2]!.selected = true;
    } else if (finishAttempts === 2) {
      expect(expected).toEqual({expected_selected: [0, 1, 2], expected_complete: [0, 1]});
      Object.assign(files[1]!, {complete: false, verified_bytes: 32});
      Object.assign(files[2]!, {complete: true, verified_bytes: 100});
    } else {
      expect(finishAttempts).toBe(3);
      expect(expected).toEqual({expected_selected: [0, 1, 2], expected_complete: [0, 2]});
      Object.assign(files[1]!, {complete: true, verified_bytes: 100});
    }
    const current = files.filter(file => file.selected).map(file => file.index);
    if (JSON.stringify(expected.expected_selected) !== JSON.stringify(current)) {
      return route.fulfill({status: 409, json: {error: {code: 'conflict', message: 'The selection changed. Review the files again.'}}});
    }
    const verified = files.filter(file => file.selected && file.complete && file.verified_bytes === file.size).map(file => file.index);
    if (expected.expected_complete.some(index => !verified.includes(index))) {
      return route.fulfill({status: 409, json: {error: {code: 'conflict', message: 'A reviewed file is no longer complete. Review the files again.'}}});
    }
    expect(verified).toEqual([0, 1, 2]);
    download.state = 'completed';
    return route.fulfill({json: download});
  });

  await page.setViewportSize({width: 390, height: 844});
  await page.goto('/downloads');
  await page.getByRole('button', {name: 'Review files for Transfer fixture'}).click();
  const picker = page.getByRole('dialog');
  const choice = picker.getByRole('checkbox', {name: 'a.bin', exact: true});
  await choice.uncheck();
  const search = picker.getByRole('textbox', {name: 'Find files in this torrent'});
  await search.fill('a.bin');
  await page.evaluate(() => {
    Object.defineProperty(document, 'hidden', {configurable: true, value: true});
    document.dispatchEvent(new Event('visibilitychange'));
  });
  const firstRead = reads;
  await page.evaluate(() => {
    Reflect.deleteProperty(document, 'hidden');
    document.dispatchEvent(new Event('visibilitychange'));
  });
  await expect.poll(() => reads).toBeGreaterThan(firstRead);
  await expect(search).toHaveValue('a.bin');
  await expect(choice).not.toBeChecked();
  await search.fill('');
  await choice.check();
  await picker.getByRole('button', {name: 'Start download'}).click();
  const fileRow = (path: string) => picker.getByRole('listitem').filter({has: page.getByText(path, {exact: true})});
  await expect(fileRow('b.bin')).toContainText('15%');
  const first = fileRow('a.bin').getByRole('group', {name: 'a.bin', exact: true});
  await expect(first).toHaveAccessibleDescription('Complete · 100 B');
  const nameBox = await first.getByText('a.bin', {exact: true}).boundingBox();
  const statusBox = await first.getByText('Complete · 100 B', {exact: true}).boundingBox();
  expect(nameBox).not.toBeNull();
  expect(statusBox).not.toBeNull();
  expect(statusBox!.y).toBeGreaterThanOrEqual(nameBox!.y + nameBox!.height);
  files[1]!.verified_bytes = 32;
  await expect(fileRow('b.bin')).toContainText('32%');
  await picker.getByRole('button', {name: 'Skip c.bin'}).click();
  await expect(fileRow('c.bin')).toContainText('Skipped');

  await picker.getByRole('button', {name: 'Finish completed files…'}).click();
  const confirm = page.getByRole('alertdialog', {name: 'Finish completed files?'});
  await expect(confirm).toBeVisible();
  // Losing one selected file's status must retire even an already-open confirmation.
  dropProgress = true;
  await expect(confirm).toHaveCount(0);
  const finishButton = picker.getByRole('button', {name: 'Finish completed files…'});
  await expect(finishButton).toHaveCount(0);
  dropProgress = false;
  await expect(finishButton).toBeVisible();
  failRead = true;
  await expect(finishButton).toHaveCount(0);
  failRead = false;
  await expect(finishButton).toBeVisible();
  await picker.getByRole('button', {name: 'Edit selection'}).click();
  await expect(finishButton).toHaveCount(0);
  await picker.getByRole('button', {name: 'Cancel edits'}).click();
  await finishButton.click();
  await expect(confirm).toContainText('1 of 2 selected files');
  await confirm.getByRole('button', {name: 'Keep downloading'}).click();
  expect(mutations).toEqual(['start', 'files/2/skip']);
  await picker.getByRole('button', {name: 'Finish completed files…'}).click();
  await confirm.getByRole('button', {name: 'Finish completed files', exact: true}).click();
  await expect(picker.getByText('The selection changed. Review the files again.')).toBeVisible();
  await expect(fileRow('c.bin').getByRole('button', {name: 'Skip c.bin'})).toBeVisible();
  expect(mutations).toEqual(['start', 'files/2/skip', 'finish-completed']);
  await finishButton.click();
  await expect(confirm).toContainText('1 of 3 selected files');
  Object.assign(files[1]!, {complete: true, verified_bytes: 100});
  await expect(confirm).toContainText('2 of 3 selected files');
  await confirm.getByRole('button', {name: 'Finish completed files', exact: true}).click();
  await expect(picker.getByText('A reviewed file is no longer complete. Review the files again.')).toBeVisible();
  await expect(confirm).toHaveCount(0);
  await expect(fileRow('b.bin')).toContainText('32%');
  await expect(fileRow('c.bin')).toContainText('Complete');
  expect(mutations).toEqual(['start', 'files/2/skip', 'finish-completed', 'finish-completed']);
  await finishButton.click();
  await expect(confirm).toContainText('2 of 3 selected files');
  await confirm.getByRole('button', {name: 'Finish completed files', exact: true}).click();
  await expect(picker).toHaveCount(0);
  expect(finishAttempts).toBe(3);
});
