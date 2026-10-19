# wright ui

The browser UI that `wright ui` serves for a bound Workshop project
(ADR-0032). Svelte 5 + TypeScript (strict) + Vite; the built assets are
embedded in the `wright` binary behind its `ui` Cargo feature.

## Build and test

```sh
npm ci            # install the locked toolchain
npm run generate  # regenerate src/generated/ from the committed schemas
npm run check     # svelte-check
npm test          # unit tests (vitest)
npm run build     # production build into dist/ (ignored, never committed)
npm run check-size  # fail when dist JS+CSS exceeds 100 KiB gzipped
```

`npm run generate` reads `Cargo.lock` for the pinned `workshop-rs` version,
downloads that crate from crates.io, and compiles both committed schemas
(`schemas/wright-agent-v1.schema.json`, `schemas/rule-content-v1.schema.json`
in the crate) to TypeScript. CI regenerates them and fails on any diff, so
the committed copies always match the schemas in force.

## Rules

- **Runes only.** `svelte.config.js` sets `compilerOptions.runes: true`
  project-wide. Svelte 3/4 syntax (`export let`, `$:`) is a compile error —
  use `$state`, `$derived`, `$props`, `$effect`.
- **Snapshots at the boundary** (ADR-0032 decision 7). `$state` values are
  proxies, not plain objects. Anything that leaves the reactive tree —
  request bodies, copies, values passed to `structuredClone` — goes through
  `$state.snapshot` first.
- **The only runtime dependency is `svelte`.** No component library, CSS
  framework, router, or state library.
- **No Workshop semantics here.** Names, spellings, and validity come from
  `wright-agent/v1` responses; this project implements no emission, name
  matching, or validation of its own.
