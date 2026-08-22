# AGENTS.md

This repository is part of the **WrightKit** multi-repository workspace. Apply
the workspace-level `AGENTS.md` first, then this repository's local ownership,
architecture, validation, and delivery rules.

Wright is WrightKit's user-facing **tooling and integration product**. It gives
users one surface across independently usable implementations of OverPy,
DEL/OSTW, and raw Workshop, then adds linting, static analysis, semantic query,
validated source edits, agent tooling, CI/embedding, and language services.

Wright is not the durable owner of those language implementations.

## Repository ownership

- **`workshop-rs`** — standalone raw Workshop implementation and canonical
  Workshop semantics, parser, WIR, catalog, settings/localization, validation,
  emission, and Workshop-owned gameplay/query data.
- **`opy-rs`** — standalone OverPy implementation: syntax, preprocessing,
  macros, semantics, diagnostics/provenance, OPY-specific compiler behavior,
  standalone tooling, compatibility evidence, and Workshop→OPY reconstruction.
- **`del-rs`** — standalone DEL/OSTW implementation: source/project model,
  semantics/types, runtime/compiler lowering, diagnostics/provenance,
  standalone tooling, compatibility evidence, and Workshop→DEL reconstruction.
- **`language-provider-protocol`** — versioned LPP process/data contract.
- **Wright** — unified CLI/service orchestration, cross-language diagnostics,
  lint/static analysis, semantic query, source-edit transaction safety,
  refactoring, agent/embedding APIs, CI presentation, editor-neutral language
  services, LSP, and integration adapters.

Terminology:

- **frontend** is an internal stage inside a language implementation; do not use
  it as shorthand for the product identity of `opy-rs` or `del-rs`;
- **provider** is an integration role/process exposed through LPP or another
  reviewed boundary; it does not make a language implementation subordinate to
  Wright;
- Wright may integrate through native Rust APIs and/or LPP depending on the
  product boundary, but must not pull language ownership back into this repo.

See [`docs/adr/0010-independent-implementations-and-wright-integration.md`](docs/adr/0010-independent-implementations-and-wright-integration.md).

## Dependency direction

Durable dependencies point toward the owning implementation, never back from a
language/core repository into Wright tooling internals:

```text
Wright ─────► opy-rs
Wright ─────► del-rs
Wright ─────► workshop-rs
opy-rs ─────► workshop-rs
del-rs ─────► workshop-rs
```

LPP is a process boundary and does not imply a cross-repository Rust dependency.

Do not bypass ownership boundaries merely to make a Wright command appear to
work. If a real OPY project fails because `opy-rs` lacks semantic support, fix
`opy-rs`; if a canonical Workshop primitive is missing, fix `workshop-rs`; if
the implementation is correct and Wright integrates it incorrectly, fix Wright.

## Product priority

Wright is tooling-first. Prioritize:

1. real `check` / diagnostics;
2. lint / static analysis;
3. inspect / semantic query;
4. safe source edits / refactoring;
5. agent and embedding workflows;
6. CI and language-service integration;
7. Workshop stability/cost analysis;
8. compilation/conversion needed by real workflows.

Architecture and cleanup matter when they protect a public/versioned contract,
repository ownership, dependency direction, provenance/source-edit correctness,
licensing boundaries, or an observed maintenance risk. Internal layout, helper
abstractions, temporary adapters, and code organization are revisable and must
not displace user-visible functionality without evidence.

## Real-project execution rule

A passing unit-test count is not sufficient when Wright is unusable on the real
projects that motivated the work. For product regressions:

1. reproduce the failure with the released/current Wright path and the owning
   standalone implementation where possible;
2. classify the owner before changing code;
3. keep the full-project regression or pinned project evidence;
4. add a minimized regression where practical;
5. rerun the full user workflow (`check`, `lint`, `analyze`, `inspect`, compile
   when relevant) before claiming the blocker resolved.

Issue trees and support matrices are tracking structures, not mandatory
execution algorithms. Prefer coherent implementation waves over unnecessary
per-construct issue/PR fragmentation when ownership and review boundaries remain
clear.

## Architecture boundaries

- Preserve source provenance and structured diagnostics across integration
  boundaries.
- Never silently fall back to upstream OverPy/OSTW runtimes for a declared
  first-party workflow.
- Never duplicate canonical Workshop data or semantics in Wright.
- Never invent Wright-only OPY/DEL syntax.
- Compatibility targets observable semantics, not output-text identity,
  formatting, optimizer shape, temporary variables, or upstream internals.
- Source-oriented validated edits are preferred over full-file regeneration.

Durable architecture and ADRs live under [`docs/`](docs/README.md). Current
support claims must be grounded in the owning repositories and executable
evidence, not in historical Wright monolith behavior.

## Validation

Use the smallest focused test set while iterating, then run the affected product
and workspace gates before delivery. At minimum for broad Wright changes:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features </dev/null
git diff --check
```

Run relevant scenario/compatibility gates for the changed surface. Real-project
support claims require real-project validation in addition to focused tests.

## Delivery

- Never push directly to `main`; use independent branches/worktrees and PRs.
- Keep commits focused and do not mix unrelated repository changes.
- Review-time verification results, including hashes, residual counts, and
  pass/fail status, must come from the test/CI run under review. Never hand-write
  or manually refresh a committed evidence/result file; put results in the PR
  description and CI logs/artifacts. Committed fixtures and provenance/input
  manifests are allowed only as reproducible, machine-validated inputs.
- Do not use repository changes or commits for GitHub metadata-only operations.
- Do not publish, rewrite history, delete data, or modify unrelated remote state
  unless explicitly authorized.
- Never commit credentials, private runtime data, or unreviewed third-party
  material.
