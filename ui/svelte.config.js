/** @type {import("@sveltejs/vite-plugin-svelte").SvelteConfig} */
export default {
  // ADR-0032 decision 7: runes mode is set project-wide. The compiler rejects
  // Svelte 3/4 syntax such as `export let`.
  compilerOptions: {
    runes: true,
  },
}
