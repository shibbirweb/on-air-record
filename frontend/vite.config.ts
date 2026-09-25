import { fileURLToPath, URL } from 'node:url';

import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

/**
 * The UI is served by the Rust binary in production, so every request is same origin and the API client
 * only ever uses relative `/api` paths. In development Vite proxies those paths to the backend, including
 * the WebSocket upgrade, so the same code runs unchanged in both places.
 */
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:8080',
        // Keep the browser's Host header. The backend refuses state changes and the stream when the
        // page's Origin does not match Host, and rewriting Host to the target would make every request
        // from this dev server look like it came from another site.
        changeOrigin: false,
        ws: true,
      },
    },
  },
  build: {
    outDir: 'dist',
    sourcemap: false,
  },
});
