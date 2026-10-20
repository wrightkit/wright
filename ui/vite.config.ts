/// <reference types="vitest/config" />
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [svelte()],
  // Vitest resolves imports through the SSR pipeline, whose defaults pick
  // the server build of `svelte` (`mount` is client-only). `browser` is
  // already a client-build default, so merging it only changes test runs.
  resolve: {
    conditions: ['browser'],
  },
})
