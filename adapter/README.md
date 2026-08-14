# Wright OverPy Compatibility Adapter

Status: accepted baseline for v0.1
Scope: translates pinned OverPy frontend output into the Wright-owned
`wright/opy-hir` protocol (v1)

This directory contains the temporary frontend adapter described by
[`docs/adr/0005-opy-hir-v1.md`](../docs/adr/0005-opy-hir-v1.md). It is the
only component allowed to know how an OverPy AST maps onto Opy HIR v1; the
Rust core never imports it and never depends on OverPy types.

## Ownership and licensing boundary

* The adapter is Wright-owned and AGPL-3.0-or-later, like the rest of the
  repository.
* It depends on the pinned `overpy@9.7.10` npm package (GPL-3.0-only per the
  oracle metadata) purely as an external, separately installed frontend. It
  does not copy OverPy source and it does not make OverPy types part of a
  Wright API.
* The adapter is an optional development/CI component: it is not bundled into
  the Rust core, and `wright-core` builds, tests, and runs without it. See
  [`docs/licensing.md`](../docs/licensing.md) for the component policy and
  the open licensing questions that remain.

## Invocation

```sh
pnpm install --dir adapter
node adapter/bin/wright-adapter.js \
  --input source.opy \
  --root <directory-containing-the-main-file> \
  --main-file source.opy \
  --output out.json
```

* `--root` is the include base for `#!include` directives and must contain
  the main file.
* On success the Opy HIR v1 payload is written to `--output` (or stdout) and
  the exit code is 0.
* On a frontend parse failure or an unsupported construct, a structured error
  record is written to stderr and the exit code is 1.

### Error records

Error records are adapter diagnostics, not protocol payloads:

```jsonc
{ "code": "parse" | "unsupported", "message": "...", "span": { "file": "source.opy", "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 22 } } }
```

`code` is `parse` for frontend failures and `unsupported` for constructs the
adapter cannot map to Opy HIR v1. `span.file` is the source file name.

## Fixtures and tests

* Corpus conversion: every success fixture under
  `compatibility/fixtures/**` converts to a checked-in snapshot under
  `adapter/fixtures/<fixture-id>.json`. The `synthetic/diagnostics` fixture
  must fail with a structured `parse` error.
* Mini-fixtures under `adapter/test/fixtures/` cover mapping edges outside
  the corpus: source-level constants (`macro x = ...`), function macros with
  parameter references, and explicit rejection of labels/gotos, `@Team`-style
  annotations, and custom game settings.

Run the tests:

```sh
pnpm --dir adapter test
```

Regenerate snapshots after an intentional, reviewed adapter change:

```sh
pnpm --dir adapter update-fixtures
```

Snapshots are determinism evidence: the same source, frontend version, and
adapter version must produce byte-identical HIR. Review the snapshot diff
with the adapter change.

## Known boundaries

The adapter intentionally drops the frontend's include bookkeeping rules
(`__pushRulePrefixStack__` / `__popRulePrefixStack__`) and stops before the
frontend's optimization and Workshop lowering passes. Constructs outside the
v0.1 corpus boundary fail explicitly rather than degrading silently. See
`docs/hir/opy-hir-v1.md` §11 for the out-of-scope list.
