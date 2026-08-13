# AGENTS.md

Wright is a Rust compiler and tooling workspace for the Overwatch Workshop /
OverPy ecosystem. This is a routing guide, not a project status page or a
replacement for architecture, compatibility, contribution, issue, or test
documentation.

## Coordination and scope

For multi-role work, read the [Agent Team contract](docs/agent-team.md) for
role authority, workflow, blocked routes, spec/QA-plan schemas, and evidence
traceability. GitHub issues remain the product scope and roadmap; do not copy
their state into `docs/`.

Before editing, read the active issue and acceptance criteria, inspect the
branch and working-tree status, and identify the owning crate, contract,
fixture, or document. Preserve unrelated or concurrent changes. Read the
applicable authority: [`ARCHITECTURE.md`](ARCHITECTURE.md),
[`COMPATIBILITY.md`](COMPATIBILITY.md), an accepted [`docs/adr/`](docs/adr/README.md)
decision, or the relevant package/test contract.

Keep public interfaces Wright-owned. Do not infer scope from roadmap language
or an external OverPy checkout. Put a new durable rule in its smallest
authoritative document rather than duplicating it here.

## Architecture boundaries

The stable compiler direction is:

```text
.opy frontend -> bridge/adapter -> Wright HIR -> Workshop IR -> backend/output
                                      \-> diagnostics and source provenance
compatibility oracle/harness -> evaluation evidence only
```

The native `.opy` path is owned by `wright-opy`; HIR, Workshop IR, lowering,
diagnostics, and backend contracts are Wright-owned. Frontends do not leak
external AST types, HIR does not depend on a frontend/backend, and backends do
not reparse source. Unsupported behavior is an explicit diagnostic or
documented rejection. See [`ARCHITECTURE.md`](ARCHITECTURE.md) and its ADRs.

OverPy is an isolated frontend/oracle dependency, not a Rust-core dependency.
Do not copy its source/types, expose its artifacts, or commit third-party
fixtures without provenance and redistribution review. Follow
[`LICENSE-BOUNDARY.md`](LICENSE-BOUNDARY.md).

## Verified extension paths

Use the shortest applicable path below and add a regression at the lowest layer
that proves the behavior.

### `.opy` syntax or frontend behavior

1. Check [`docs/opy/support-matrix.md`](docs/opy/support-matrix.md) and relevant
   compatibility fixtures.
2. Change `crates/wright-opy/`; keep semantic data in Wright-owned HIR and
   reject unsupported constructs explicitly.
3. Add frontend/differential coverage; run `cargo test -p wright-opy` and, when
   needed, `cargo test -p wright-opy --test differential`.

### Structured diagnostics

1. Read the typed result contract in [`docs/cli.md`](docs/cli.md) and the
   diagnostic types in `wright-driver`/`wright-opy`.
2. Produce structured, source-located data in the owning crate; keep human
   rendering in CLI/LSP presentation layers.
3. Add a narrow malformed/unsupported regression and run affected crate plus
   CLI/service tests.

### Compatibility or oracle regression

1. Read [`COMPATIBILITY.md`](COMPATIBILITY.md) and
   [`compatibility/README.md`](compatibility/README.md); establish the S, D, N,
   or E claim and provenance first.
2. Add the existing fixture manifest/source/snapshot, or a generator/hash when
   redistribution is not allowed. Do not update snapshots incidentally.
3. Run `python3 -m unittest discover -s compatibility/tests` and the relevant
   oracle/differential command; use `--update` only for a reviewed oracle change.

### Language-service capability

1. Read [`docs/language-services.md`](docs/language-services.md); keep
   editor-neutral behavior in `wright-language` and protocol mapping in
   `wright-lsp`.
2. Extend the service contract and source-aware/versioned tests before LSP DTOs.
3. Run `cargo test -p wright-language` and, for protocol changes,
   `cargo test -p wright-lsp --test lsp`.

### Scenario or release-gate case

1. Add source and expectation manifest under `scenarios/`, or extend the gate
   described in [`docs/release.md`](docs/release.md).
2. Build the CLI when needed, then run `python3 scripts/run-scenarios.py` or
   `python3 scripts/v1-gates.py`; keep reports in local `target/` evidence.
3. For a release claim, also run [`CONTRIBUTING.md`](CONTRIBUTING.md) and the
   affected benchmark/release path.

## Validation and delivery

The Rust baseline and MSRV policy are in [`CONTRIBUTING.md`](CONTRIBUTING.md);
CI runs them on stable and Rust 1.85.0. Oracle/adapter checks require pinned
pnpm/Node dependencies, and v1 gates write reports under `target/`. A build,
health check, or single test is not proof of compatibility or runtime
behavior; state the evidence level and boundary.

For implementation work, review the complete diff, run `git diff --check`,
stage only task-owned files, and make a focused commit by default. Preserve
unrelated dirt. Do not push, rewrite history, delete data, publish artifacts,
or modify remote issues unless explicitly requested. Never commit credentials,
private runtime data, or unreviewed generated/third-party material.
