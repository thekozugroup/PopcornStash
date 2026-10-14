import {defineConfig} from 'vitest/config';

// Unit tests for the web app's plain logic (src/**/*.test.ts): no StyleX or React
// rendering here; pages and components are covered by the Playwright suite (tests/e2e).
export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
});
