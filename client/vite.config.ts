import { defineConfig } from 'vite';

// The wasm-pack `--target web` output is plain ESM: the generated `init()`
// fetches the .wasm by URL, which Vite serves and bundles as an asset.
export default defineConfig({
  build: { target: 'es2022' },
  server: { fs: { strict: true } },
});
