import {defineConfig} from '@playwright/test';

// Extensions need a persistent Chromium context (see test/e2e/extension.spec.ts).
// Browsers come from PLAYWRIGHT_BROWSERS_PATH; this config never downloads them.
export default defineConfig({
  testDir: 'test/e2e',
  timeout: 90_000,
  workers: 1,
  reporter: [['list']],
  outputDir: '.e2e/results',
  use: {trace: 'retain-on-failure'},
});
