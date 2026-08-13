# Architecture Decision Records

ADRs record decisions that affect Wright's observable compiler contract or
component boundaries. They complement, rather than replace, the normative
contracts in [`ARCHITECTURE.md`](../../ARCHITECTURE.md) and
[`COMPATIBILITY.md`](../../COMPATIBILITY.md).

## Conventions

* Files use a zero-padded sequence and a short title, for example
  `0003-ir-boundary.md`.
* An ADR starts as `Proposed` and becomes `Accepted` when the decision is the
  repository's current contract. `Superseded` ADRs remain for history and link
  to the decision that replaces them.
* An accepted ADR is not rewritten to hide history. A material change gets a
  new ADR, and the old record links to it.
* Decisions state their scope, consequences, compatibility impact, and open
  questions. They describe observable constraints rather than implementation
  guesses.
* New ADRs should link to the affected architecture or compatibility document
  and to any superseded or related decision.

## Index

* [ADR template](0000-template.md)
* [ADR-0001: Project scope](0001-project-scope.md)
* [ADR-0002: Compatibility strategy](0002-compatibility-strategy.md)
* [ADR-0003: IR boundary](0003-ir-boundary.md)
* [ADR-0004: OverPy licensing and clean-room boundary](0004-overpy-licensing-boundary.md)
* [ADR-0005: Opy HIR v1 frontend protocol](0005-opy-hir-v1.md)
* [ADR-0006: Rust IR core — typed IDs, arenas, and two-layer models](0006-rust-ir-core.md)
* [ADR-0007: OverPy reference pinning policy](0007-reference-pinning-policy.md)
