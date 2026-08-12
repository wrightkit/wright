# Wright Architecture

Status: accepted baseline for v0.1
Scope: the Wright compiler core and its compatibility boundary

This document defines the responsibilities and observable contracts that later
milestones may rely on. It does not promise that every component named here is
implemented in v0.1.

## Project boundary

Wright v1 is an OverPy-compatible Rust compiler core. Existing OverPy remains
the `.opy` frontend/parser and compatibility oracle until a later native
frontend milestone. Wright owns the representations and transformations after
the frontend boundary; it does not reproduce OverPy internals for the sake of
implementation parity.

The intended flow is:

```text
`.opy` source
    -> OverPy frontend/parser (external compatibility component)
    -> Wright adapter/bridge
    -> Wright HIR
    -> Wright Workshop IR
    -> Wright backend
    -> Workshop-oriented output
```

Compatibility evaluation is a side path, not a core dependency:

```text
the same input and produced artifact
    -> compatibility harness
    -> documented OverPy oracle and/or behavior runner
```

The harness may invoke an installed OverPy tool or consume its documented
outputs. It must not make the Rust core depend on OverPy implementation types.

## Component responsibilities

### Frontend

The v1 frontend is the existing OverPy `.opy` parser. It owns source-language
parsing, frontend syntax rules, and frontend-specific parse structures. Those
structures are external to Wright's public core boundary.

A future native Rust frontend is an explicitly separate milestone. Nothing in
this baseline requires or implies one.

### Adapter and bridge

The adapter/bridge is the narrow translation boundary between a frontend and
Wright's owned representations. It is responsible for:

* accepting a documented frontend result or an explicitly supported interchange
  format;
* validating identifiers, references, source spans, and unsupported constructs;
* preserving source provenance where it is available; and
* producing typed Wright data or structured diagnostics.

The bridge must not expose frontend-internal types through a Wright API. A
frontend-specific change should be contained at this boundary unless it changes
the Wright contract itself.

### HIR

HIR is Wright's frontend-independent, semantic representation. It describes
the meaning needed by later compiler stages: declarations, references,
expressions, control flow, source provenance, and explicit unsupported cases as
the supported subset grows.

HIR must not encode a particular Workshop text layout or depend on an OverPy
AST type. Its contract is semantic identity and validated relationships, not
source-text preservation.

### Workshop IR

Workshop IR is Wright's target-oriented representation. It contains the
validated operations, values, control-flow constructs, and metadata required by
Workshop backends. Lowering from HIR to Workshop IR is explicit so that target
constraints and unsupported semantics are observable at a named boundary.

Workshop IR must be deterministic for the same validated HIR and compiler
configuration. It may carry source provenance for diagnostics, but it must not
become a catch-all container for frontend implementation details.

### Backend

A backend consumes Workshop IR and produces a documented Workshop-oriented
artifact or a structured diagnostic. It owns target serialization, formatting,
and target-specific validation. It does not reparse `.opy` source or consult
OverPy internals as part of normal compilation.

The initial bootstrap does not include a parser, lowering implementation, or
backend. Adding one requires a focused contract and tests at the corresponding
boundary.

### Compiler driver and CLI (M6)

The compiler/session driver (`wright-driver`) is the single orchestration path
shared by the CLI, library consumers, and later tool/LSP adapters: input
discovery → frontend selection → validation → lowering → analysis → emission.
Frontends are selected behind one contract (the temporary `.opy` bridge, the
native Workshop frontend, or Opy HIR v1 protocol JSON), so the M7 native
`.opy` frontend replaces the bridge without changing callers. The `wright`
CLI (`wright-cli`) is a thin argv/presentation layer; human text and
machine-readable JSON (`wright-result/v1`) are two renderings of the same
typed result envelope. The normative contract is `docs/cli.md`.

### Compatibility harness and oracle

Compatibility tooling is an evaluation boundary around the core. It records
the reference version, corpus identity, normalization rules, and comparison
result for each claim. It may call OverPy, compare normalized artifacts, or run
behavioral scenarios according to [`COMPATIBILITY.md`](COMPATIBILITY.md).

The harness is not evidence that the core may link to or copy OverPy code. The
engineering and licensing boundary is defined in
[`LICENSE-BOUNDARY.md`](LICENSE-BOUNDARY.md).

## Dependency direction

Dependencies point toward owned, stable contracts:

```text
frontend-specific adapter -> HIR -> Workshop IR -> backend
                                      ^
                          compatibility harness (evaluation only)
```

More precisely:

* frontend adapters may depend on the HIR and bridge contracts;
* HIR must not depend on a frontend or backend;
* Workshop IR may depend on HIR concepts during lowering, but HIR must not
  depend on Workshop IR;
* backends may depend on Workshop IR, not on parser internals;
* compatibility tooling may depend on public artifacts and documented test
  interfaces, but the core must not require the oracle to compile or run.

The workspace layout is allowed to grow around these boundaries. A package is
not justified merely because a future component is named here; its public
contract should be introduced with the milestone that needs it.

## Cross-cutting contracts

The following are normative for v1 work:

* **Explicit unsupported behavior:** unsupported syntax or semantics produces a
  structured diagnostic or a documented rejection. Silent fallback is not a
  compatibility strategy.
* **Provenance:** source spans and relevant origin information are preserved
  across transformations where the input provides them.
* **Determinism:** equal validated inputs, configuration, and toolchain produce
  stable observable IR and output, subject to explicitly documented volatile
  fields.
* **Semantic identity:** compatibility claims are about accepted syntax,
  diagnostics, normalized artifacts, or behavior at a named level. Textual
  similarity alone is not semantic evidence.
* **Owned public interfaces:** Wright APIs expose Wright-owned types and
  contracts, not convenient aliases for external implementation details.

## v1 non-goals

The following are outside the v1 contract:

* a native Rust `.opy` parser;
* a full decompiler rewrite;
* a full LSP rewrite;
* a new language or intentionally incompatible `.opy` semantics; and
* reproducing OverPy internals merely for implementation parity.

These are scope boundaries, not promises about future roadmap priority.

## Open questions

These questions are intentionally left for the milestone that has enough
implementation evidence to answer them:

1. Which OverPy versions and extensions form the supported v1 input set?
2. The HIR schema and versioning policy for the v0.1 bridge is defined by
   [ADR-0005](docs/adr/0005-opy-hir-v1.md) and
   [`docs/hir/opy-hir-v1.md`](docs/hir/opy-hir-v1.md); the internal IR data
   model and the first lowering boundary are defined by
   [ADR-0006](docs/adr/0006-rust-ir-core.md). Workshop IR text emission and
   its schema/versioning as an output artifact remain open until an emitter
   milestone exists.
3. Which diagnostic codes and machine-readable fields are stable enough for
   clients?
4. Which Workshop output targets and runtime versions are covered by semantic
   tests?
5. Which compatibility corpus entries can be redistributed, and which must be
   generated locally?
6. Which licensing questions require advice from qualified counsel?

Decisions that answer or materially revise these questions belong in an ADR.

## Related decisions

* [ADR-0001: Project scope](docs/adr/0001-project-scope.md)
* [ADR-0002: Compatibility strategy](docs/adr/0002-compatibility-strategy.md)
* [ADR-0003: IR boundary](docs/adr/0003-ir-boundary.md)
* [ADR-0004: OverPy licensing and clean-room boundary](docs/adr/0004-overpy-licensing-boundary.md)
* [ADR-0005: Opy HIR v1 frontend protocol](docs/adr/0005-opy-hir-v1.md)
* [ADR-0006: Rust IR core — typed IDs, arenas, and two-layer models](docs/adr/0006-rust-ir-core.md)
