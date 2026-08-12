# ADR-0005: Opy HIR v1 frontend protocol

- Status: Accepted
- Date: 2026-08-12
- Related: [Issue #3](https://github.com/wrightkit/wright/issues/3),
  [Issue #17](https://github.com/wrightkit/wright/issues/17),
  [`docs/hir/opy-hir-v1.md`](../hir/opy-hir-v1.md),
  [ADR-0003](0003-ir-boundary.md),
  [ADR-0004](0004-overpy-licensing-boundary.md)

## Context

Wright's v0.1 flow uses existing OverPy as the `.opy` frontend. The parsed
frontend AST is an OverPy-internal representation that the Rust core must not
depend on, and `JSON.stringify()` of that AST is not a protocol design. The
first concrete compiler slice now exists to consume: the compatibility corpus
must convert reproducibly from `.opy` through a Wright-owned interchange
format into the Rust core.

Issue #3 requires a stable, versioned protocol between the temporary OverPy
frontend and the Rust core. The evidence gathered from the pinned OverPy
frontend (9.7.10) shows that its public compile API does not expose parsed rule
ASTs, that its exported compiler class can be driven to produce parsed ASTs
with source provenance, and that the corpus needs declarations, rules, events,
conditions, statements, and expressions with spans.

## Decision

Wright adopts `wright/opy-hir` version `1.0.0` as the v0.1 frontend protocol,
specified in [`docs/hir/opy-hir-v1.md`](../hir/opy-hir-v1.md).

The protocol:

* models program declarations, rules, events, conditions, statements, and
  expressions with Wright-owned node kinds and operator spellings;
* preserves file/line/column provenance through `files` and `span` fields;
* keeps source-level macro calls explicit (`macroCall`) and records macro,
  constant, and preprocessor-define declarations so the payload is
  self-contained;
* treats the semver major version as the compatibility boundary: consumers
  reject an unknown name or major version before parsing, reject unknown node
  kinds as structured unsupported-node errors, and never silently ignore
  semantic content;
* defines validation order (envelope, shape, provenance, identifiers,
  references, unsupported nodes) and stable debug output for tests and issue
  reports.

## Consequences

The adapter (`adapter/`) becomes the only component that maps OverPy internals
onto the protocol; the Rust core consumes the protocol through serde types and
validation in `crates/wright-core/src/hir/`. Macro calls remain explicit in
HIR rather than being expanded by the adapter, so expansion semantics stay
with a later Wright-owned stage.

Constructs outside the v0.1 corpus boundary (labels, relative gotos,
custom game settings) are rejected explicitly by the adapter and reported as
unsupported by the consumer. This keeps the first protocol slice small while
making unsupported behavior observable, per the architecture's explicit
unsupported-behavior contract.

## Compatibility impact

This ADR defines the interchange contract for S/D-level compatibility work:
the corpus converts reproducibly (S), and failures carry structured
diagnostics with spans (D). It does not claim N/E-level parity with OverPy
output. Protocol equality alone is not semantic evidence, per
[ADR-0003](0003-ir-boundary.md) and [`COMPATIBILITY.md`](../../COMPATIBILITY.md).

## Open questions

* Whether macro calls should be expanded inside HIR once a Wright-owned
  macro-expansion stage exists. v1 keeps them explicit.
* Whether `settings` blocks and labels/gotos need HIR modeling when the
  corpus grows beyond v0.1.
* The adapter's exact OverPy driver setup is an implementation detail of the
  adapter boundary; any change to the pinned frontend version must be reviewed
  with the adapter and this protocol together.
