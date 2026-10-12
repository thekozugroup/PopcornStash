import {defineConfig} from 'vite';
import react from '@vitejs/plugin-react';
import {astryxStylex} from '@astryxdesign/build/vite';

// The Rust server (`popcornstash serve`) listens on 8788 by default.
// When developing against it, add the dev origin to the server's allowed
// origins so CSRF and WebAuthn checks accept it, for example:
//   STASH_PUBLIC_URL=http://localhost:8788
//   STASH_EXTRA_ORIGINS=http://localhost:5173
const backend = process.env.POPCORN_BACKEND ?? 'http://localhost:8788';

export default defineConfig({
  plugins: [...astryxStylex(), react()],
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': {target: backend, ws: false},
      '/.well-known': {target: backend},
    },
  },
  preview: {
    port: 4173,
    proxy: {
      '/api': {target: backend},
      '/.well-known': {target: backend},
    },
  },
  build: {
    outDir: 'dist',
    assetsDir: 'assets',
    emptyOutDir: true,
    sourcemap: false,
    chunkSizeWarningLimit: 900,
  },
});
