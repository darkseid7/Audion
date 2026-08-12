import { defineConfig } from 'vitest/config';
import path from 'path';

export default defineConfig({
  test: {
    // Pure-function unit tests — no Svelte component rendering needed.
    // We use node environment to avoid DOM dependencies.
    environment: 'node',
    include: ['src/**/*.test.ts'],
  },
  resolve: {
    alias: {
      // Match SvelteKit's $lib alias so imports resolve
      '$lib': path.resolve(__dirname, 'src/lib'),
    },
  },
});
