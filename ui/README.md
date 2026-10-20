# wright ui

The browser view for `wright ui` — the read-only structured Workshop rule
view over the `wright-agent/v1` contract (ADR-0032); editing is a later
layer. Svelte 5, TypeScript strict, Vite. The only runtime dependency is
`svelte`; no component library, CSS framework, router, or state library.

The page comes alive only under `wright ui`: it reads `rules`
(`content: true`), `lookup`, `symbols`, and `findings` through `POST /api`,
carrying `Authorization: Bearer <token>` with the token from the session
URL fragment. `vite dev`/`preview` serve the bundle but have no `/api`
backend, so they render the loading state only.

## Build and test

Node `^22.12 || ^24 || >=26` and npm:

```sh
npm ci                  # install the locked dependencies
npm run dev             # dev server
npm run build           # production build -> dist/ (ignored, never committed)
npm run check           # svelte-check + tsc
npm test                # vitest unit tests
npm run test:e2e        # browser E2E: wright ui + headless Chromium-family
npm run check:size      # dist/ JS+CSS must be < 100 KiB gzipped (fails over)
```

`test:e2e` is a local check and does not run in CI. It builds nothing itself: it needs a `wright` binary built with
`--features ui` after `npm run build` (debug builds embed `dist/` at
compile time), and a Chromium-family browser. Overrides:

```sh
WRIGHT_BIN=target/debug/wright    # binary under test
WRIGHT_UI_BROWSER=brave           # executable path or its name on PATH
E2E_TRACE=1                       # echo browser stderr while diagnosing
```

The script spawns `wright ui --no-open tests/fixtures/workshop/synthetic/ui-view.ws`,
launches the browser headless, and drives the real DOM over the DevTools
protocol — no driver dependency.

## Generated types

`src/lib/types/generated/` is generated from the committed schemas — never edited by
hand:

```sh
npm run generate-types  # regenerate src/lib/types/generated/*.ts
npm run check-types     # fail when the committed files are stale (CI)
```

`wright-agent-v1.ts` comes from `schemas/wright-agent-v1.schema.json`;
`rule-content-v1.ts` comes from the `rule-content-v1.schema.json` shipped in
the published `workshop-rs` crate at the version `Cargo.lock` pins.

## Rules

- **Runes only.** Runes mode is set project-wide in `svelte.config.js`, so
  `export let` and other Svelte 3/4 syntax are compile errors. Props come from
  `$props()`, state from `$state`, derived values from `$derived`.
- **Snapshots at the boundary** (ADR-0032 decision 7). `$state` values are
  reactive proxies, not plain objects. Every value that leaves the reactive
  tree — request bodies, copies, anything handed to `structuredClone` — passes
  through `$state.snapshot` first.
