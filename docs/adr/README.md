# Architecture Decision Records

ADRs record point-in-time decisions that affected Wright's observable product/compiler contract or component boundaries. They preserve rationale and consequences; they are not a database of current implementation reality.

Current architecture contracts are routed from [`docs/architecture/README.md`](../architecture/README.md). Source, Cargo metadata, tests, CI, integrations, and real-project evidence establish implementation reality. An `Accepted` ADR means the decision was approved at that point in project history; it does not by itself prove that current code still conforms to the decision.

## Conventions

* Files use a zero-padded sequence and a short title, for example `0003-ir-boundary.md`.
* An ADR starts as `Proposed` and becomes `Accepted` when the decision is approved.
* `Superseded` ADRs remain for history and link to the decision that replaces them.
* An accepted ADR is not rewritten to hide history. A material change gets a new ADR when the rationale/decision history is worth preserving, while the current invariant is reflected in `docs/architecture/` or another focused current contract.
* Decisions state their scope, consequences, compatibility impact, and open questions. Historical implementation mechanisms may remain in the record without becoming current architecture authority.
* Do not use ADR status to encode current release versions, feature counts, migration progress, or Issue/PR state.

The [post-ADR-0010 decision inventory](post-0010-inventory.md) records the
candidate classification and evidence for the latest backfill set.

## Index

* [ADR template](0000-template.md)
* [ADR-0001: Project scope](0001-project-scope.md) _(superseded by ADR-0008)_
* [ADR-0002: Compatibility strategy](0002-compatibility-strategy.md)
* [ADR-0003: IR boundary](0003-ir-boundary.md)
* [ADR-0004: OverPy licensing and clean-room boundary](0004-overpy-licensing-boundary.md)
* [ADR-0005: Opy HIR v1 frontend protocol](0005-opy-hir-v1.md)
* [ADR-0006: Rust IR core: typed IDs, arenas, and two-layer models](0006-rust-ir-core.md)
* [ADR-0007: OverPy reference pinning policy](0007-reference-pinning-policy.md)
* [ADR-0008: Tooling-first semantic platform rebaseline](0008-tooling-first-semantic-platform.md)
* [ADR-0009: Language ownership and licensing boundaries](0009-language-ownership-licensing-boundaries.md)
* [ADR-0010: Independent language implementations and Wright integration](0010-independent-implementations-and-wright-integration.md)
* [ADR-0011: Distinct check, lint, and analyze workflows](0011-distinct-tooling-workflows.md)
* [ADR-0012: First-party provider distribution and local activation](0012-first-party-provider-distribution.md)
* [ADR-0013: Entry-based source-provider integration seam](0013-entry-based-source-provider-seam.md)
* [ADR-0014: Repository-namespaced immutable R2 release artifacts](0014-namespaced-immutable-r2-artifacts.md)
* [ADR-0015: Canonical facts, declarative lint rules, and project policy](0015-canonical-facts-and-declarative-lint-policy.md)
* [Post-ADR-0010 decision inventory](post-0010-inventory.md)
