import {defineConfig} from 'vitest/config';

// Unit tests for the web app's plain logic (src/**/*.test.ts): no StyleX or React
// rendering here; pages and components are covered by the Playwright suite (tests/e2e).
// The theme's contrast test reads the built stylesheet as text (`popcorn.css?raw`),
// which Vitest would otherwise replace with an empty string.
export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
    css: {include: [/popcorn\.css/]},
  },
});
