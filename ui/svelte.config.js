import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

// ADR-0032 decision 7: runes mode is project-wide, so the compiler rejects
// Svelte 3/4 component syntax (no `export let`, no `$:`) everywhere.
export default {
  preprocess: vitePreprocess(),
  compilerOptions: { runes: true },
};
