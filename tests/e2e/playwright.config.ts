import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {defineConfig} from '@playwright/test';

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '../..');
const port = Number(process.env.E2E_PORT ?? 8788);

// Playwright's own Chromium lives in /opt/pw-browsers in the dev container; elsewhere the
// default browser cache (or PLAYWRIGHT_BROWSERS_PATH) is used. E2E_CHROMIUM overrides the binary.
process.env.PLAYWRIGHT_BROWSERS_PATH ??= '/opt/pw-browsers';

export default defineConfig({
  testDir: './specs',
  // One server and one library for the whole run: tests share state, so they run in order.
  workers: 1,
  fullyParallel: false,
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  timeout: 60_000,
  expect: {timeout: 10_000},
  globalSetup: './global-setup.ts',
  // Kept under the repository's ignored output folders.
  outputDir: path.join(repoRoot, 'test-results', 'e2e'),
  reporter: [['list'], ['html', {outputFolder: path.join(repoRoot, 'playwright-report'), open: 'never'}]],
  use: {
    // Passkeys need a secure context: http://localhost is one; a LAN IP is not.
    baseURL: `http://localhost:${port}`,
    // The full Chromium build (not the headless shell), so WebAuthn and media behave as in the browser,
    // or another Chromium-based binary (for example Chrome, which can decode H.264).
    ...(process.env.E2E_CHROMIUM
      ? {launchOptions: {executablePath: process.env.E2E_CHROMIUM}}
      : {channel: 'chromium'}),
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    viewport: {width: 1280, height: 860},
  },
  projects: [
    {name: 'first-run', testMatch: /first-run\.spec\.ts/},
    {name: 'app', testIgnore: /first-run\.spec\.ts/, dependencies: ['first-run']},
  ],
});
