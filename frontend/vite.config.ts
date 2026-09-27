/// <reference types="vitest/config" />

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
  test: {
    // Opt in through `npm run coverage`; `npm test`, which CI and `make check` run, stays as fast as it was.
    coverage: {
      provider: 'v8',
      include: ['src/**/*.{ts,tsx}'],
      exclude: [
        'src/**/__tests__/**',
        'src/**/*.test.{ts,tsx}',
        'src/test/**',
        'src/**/*.d.ts',
        // Types only, so there is nothing in it to execute.
        'src/api/types.ts',
        'src/main.tsx',
        'src/vite-env.d.ts',
      ],
      reporter: ['text-summary', 'html', 'lcov'],
      reportsDirectory: './coverage',
      // A floor, not a target: the measured totals rounded down, less two points, so a real regression
      // fails the run while generated property inputs moving a branch or two cannot.
      thresholds: {
        statements: 96,
        branches: 93,
        functions: 95,
        lines: 96,
      },
    },
  },
});
