/// <reference types="vitest/config" />
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [svelte()],
  // Vitest resolves imports through the SSR pipeline, whose defaults pick
  // the server build of `svelte` (`mount` is client-only). The `resolve` key
  // itself is gated on VITEST so the production bundle's resolver stays at
  // its defaults: `conditions` fully replaces the defaults, and `[]` dropped
  // `browser`, bundling the server build.
  resolve: process.env.VITEST ? { conditions: ['browser'] } : {},
})
