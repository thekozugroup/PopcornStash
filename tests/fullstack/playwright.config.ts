/**
 * Playwright settings for the full-stack run. scripts/fullstack-e2e.sh starts
 * the containers and passes everything the spec needs in FS_* variables; run
 * the spec through that script, not on its own.
 */
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {defineConfig} from '@playwright/test';

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '../..');

// Playwright's own Chromium lives in /opt/pw-browsers in the dev container; elsewhere the
// default browser cache (or PLAYWRIGHT_BROWSERS_PATH) is used. FS_CHROMIUM overrides the binary.
process.env.PLAYWRIGHT_BROWSERS_PATH ??= '/opt/pw-browsers';

export default defineConfig({
  testDir: './specs',
  // One stack, one journey: the steps depend on each other and run in order.
  workers: 1,
  fullyParallel: false,
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  timeout: 6 * 60_000,
  expect: {timeout: 15_000},
  outputDir: path.join(repoRoot, 'test-results', 'fullstack'),
  reporter: [['list']],
  use: {
    // Passkeys need a secure context: http://localhost is one. STASH_PUBLIC_URL matches it.
    baseURL: process.env.FS_BASE_URL ?? 'http://localhost:8788',
    ...(process.env.FS_CHROMIUM ? {launchOptions: {executablePath: process.env.FS_CHROMIUM}} : {channel: 'chromium'}),
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    viewport: {width: 1440, height: 900},
    colorScheme: 'light',
  },
});
