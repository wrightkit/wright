# AGENTS.md

This repository is part of the **WrightKit** multi-repository workspace.
Apply the workspace-level `AGENTS.md` when available, then follow this
repository's local ownership, architecture, validation, and delivery rules.

Within WrightKit, Wright is the user-facing **tooling and orchestration**
repository for Overwatch Workshop development. It owns CLI/service
orchestration, diagnostics, static analysis, generic source-edit transaction
safety, semantic refactoring, agent tooling and embedding APIs
([`docs/embedding.md`](docs/embedding.md)), and protocol integration (Language
Server Protocol, Language Provider Protocol).

Wright does **not** claim durable ownership of source-language compiler
internals (owned by language-specific provider repositories) or canonical
Workshop semantics/catalog/IR (owned by `workshop-rs`).

This file is a concise routing and engineering guide, not a project status page
or a duplicate of architecture contracts.

## Coordination and multi-repository routing

Read the [Agent Team contract](docs/agent-team.md) for role authority (PM,
Architect, Engineer, QA), workflow, blocked routes, and evidence traceability.
GitHub issues remain the product scope and roadmap (normative pre-1.0 roadmap:
[#134](https://github.com/wrightkit/wright/issues/134)); do not copy issue
state into `docs/`.

### Repository ownership

Before implementing a task, identify the semantic and product owner:

* **Wright (`wright`)**: Owns user-facing CLI and service binaries
  (`wright-cli`, `wright-tool`, `wright-serve`), compiler session orchestration
  (`wright-driver`), static analysis and query (`wright-analyzer`), transform
  pipelines (`wright-transform`), generic source-edit transaction safety and
  semantic refactoring (`wright-driver`, `wright-language`), editor-neutral
  language services (`wright-language`), LSP protocol mapping (`wright-lsp`),
  agent/embedding APIs (`wright-consumer`, [`docs/embedding.md`](docs/embedding.md)),
  and protocol adapters.
* **`workshop-rs`**: Owns canonical Workshop semantics, actions, values, AST,
  parser, emitter, multi-locale action/value catalogs, and Workshop IR (WIR).
  Route canonical Workshop language rules, catalogs, or AST/WIR contract changes
  to `workshop-rs`.
* **Language Providers** (e.g. OPY, OSTW providers): Own source-language
  syntax, lexers, parsers, ASTs, source-level symbol resolution, and compiler
  internals.

Do not bypass ownership boundaries or move responsibilities across repositories
merely to simplify an implementation.

### Migration boundary

During the ecosystem migration period, in-repo crates (`crates/wright-opy`,
`crates/wright-ostw`, `crates/wright-workshop`, `crates/wright-ir`) coexist
within this workspace until repository extraction is complete. Agents must not
conflate temporary in-repo co-location with target architecture:

* Keep language-specific code decoupled from Wright tooling internals.
* Do not introduce new dependencies from source-language frontends back into
  Wright tooling internals.
* Route durable contract changes to their target repository owners.

## Architecture boundaries and licensing

The tooling and orchestration layer coordinates language providers and Workshop
core via explicit protocol and data contracts:

```text
Client / Agent / LSP / CLI
    ↓
Wright Tooling & Session Orchestration (wright-driver, wright-language, wright-analyzer)
    ↓                                    ↓
Language Providers (LPP / native)   Workshop Core (workshop-rs / WIR)
    ↓                                    ↓
Source AST & Semantic Diagnostics    Canonical Workshop AST & Output
```

Durable architecture and decision records live in [`docs/architecture.md`](docs/architecture.md)
and [`docs/adr/`](docs/adr/README.md).

### Upstream references and licensing

Pinned upstream compilers (e.g. OverPy, OSTW) are external compatibility
oracles and behavior references, never runtime dependencies.

* Do not link to upstream references, copy their source, import internal ASTs,
  or commit unreviewed third-party fixtures.
* Clean-room development, provenance verification, and redistribution review
  follow [`docs/licensing.md`](docs/licensing.md) and [`docs/compatibility.md`](docs/compatibility.md).

## Verified routing and extension paths

Add regressions at the lowest layer that proves the behavior. Use the shortest
applicable path:

### 1. Wright tooling, diagnostics, and source edits

1. Read [`docs/cli.md`](docs/cli.md), [`docs/architecture.md`](docs/architecture.md),
   and the diagnostic/edit types in `wright-driver`, `wright-analyzer`, or
   `wright-transform`.
2. Implement transaction-safe edits, structured diagnostics, or CLI/tool
   commands in the owning crate; preserve source provenance and determinism.
3. Run `cargo test -p wright-driver`, `cargo test -p wright-analyzer`, and
   `cargo test -p wright-cli`.

### 2. Language services and protocol integration (LSP / LPP)

1. Read [`docs/language-services.md`](docs/language-services.md).
2. Keep editor-neutral language intelligence in `wright-language` and protocol
   DTO mapping in `wright-lsp` (or LPP provider adapters).
3. Run `cargo test -p wright-language` and `cargo test -p wright-lsp --test lsp`.

### 3. Canonical Workshop semantics and catalog (`workshop-rs`)

1. Route canonical Workshop AST, parser, emitter, and catalog changes to
   `workshop-rs`.
2. In-repo migration path: verify `crates/wright-workshop` and `crates/wright-ir`.
3. Run `cargo test -p wright-workshop` and `cargo test -p wright-ir`.

### 4. Language-provider semantics (OPY / OSTW)

1. Check the relevant support matrix (e.g. [`docs/opy/support-matrix.md`](docs/opy/support-matrix.md),
   [`docs/ostw/compatibility-baseline.md`](docs/ostw/compatibility-baseline.md)).
2. In-repo migration path: implement in `crates/wright-opy` or
   `crates/wright-ostw`; keep semantic data in frontend-neutral HIR/WIR and reject
   unsupported constructs explicitly.
3. Run `cargo test -p wright-opy` (and `cargo test -p wright-opy --test differential`),
   or `cargo test -p wright-ostw` (and `cargo test -p wright-ostw --test differential`).

### 5. Compatibility, oracles, and conformance evidence

1. Read [`docs/compatibility.md`](docs/compatibility.md) and
   [`compatibility/README.md`](compatibility/README.md); establish the claim
   class (S, D, N, E) and provenance first.
2. Add or update fixture manifests/snapshots under `compatibility/` only with
   reviewed provenance; do not update snapshots incidentally.
3. Run `python3 -m unittest discover -s compatibility/tests` and the relevant
   oracle harness.

### 6. Agent tooling, embedding, and scenarios

1. Read [`docs/embedding.md`](docs/embedding.md) and [`docs/release.md`](docs/release.md).
2. For agent/consumer APIs: test `crates/wright-consumer` via
   `cargo test -p wright-consumer`.
3. For scenario and release gates: run `python3 scripts/run-scenarios.py` and
   `python3 scripts/v1-gates.py`.

## Validation and delivery

The Rust baseline and MSRV policy are in [`CONTRIBUTING.md`](CONTRIBUTING.md);
CI runs on stable and Rust 1.85.0. External oracle checks require pinned
pnpm/Node dependencies when invoked, and release gates record evidence under
`target/`. A single test or build pass is not proof of compatibility; state the
evidence level and boundary.

For implementation work:

* Review the complete diff and run `git diff --check`.
* Stage only task-owned files; preserve unrelated dirt.
* Use Conventional Commits (`type(scope): subject`).
* Do not push, rewrite history, delete data, publish artifacts, or modify remote
  issues unless explicitly authorized.
* Never commit credentials, private runtime data, or unreviewed third-party
  material.
