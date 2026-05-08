import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vitest/config';

// Separate vitest config so the prod `vite.config.ts` doesn't have to
// pull in the testing-library deps. SvelteKit-aware so `$lib`, `$app`,
// and the Svelte 5 compiler all resolve identically to dev / build.
export default defineConfig({
  plugins: [sveltekit()],
  // Svelte ships separate server/browser entrypoints; vitest defaults
  // to the SSR one which throws on `mount(...)`. Pin to the browser
  // condition so component tests use the runtime that actually works
  // in jsdom.
  resolve: {
    conditions: ['browser']
  },
  test: {
    environment: 'jsdom',
    globals: true,
    include: ['src/**/*.{test,spec}.{ts,js}'],
    setupFiles: ['./vitest.setup.ts']
  }
});
