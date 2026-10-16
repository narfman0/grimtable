import { defineConfig } from 'vite';

// The wasm-pack `--target web` output is plain ESM: the generated `init()`
// fetches the .wasm by URL, which Vite serves and bundles as an asset.
// `/api` (HTTP and the game WebSocket) is proxied to the server, real or mock.
export default defineConfig({
  build: { target: 'es2022' },
  server: {
    fs: { strict: true },
    proxy: {
      '/api': { target: 'http://localhost:8080', changeOrigin: false, ws: true },
    },
  },
});
