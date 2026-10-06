# Product contract: the path to 1.0

Moved from `wrightkit/wright#134` on 2026-10-06, where it was maintained as an issue body, then deduplicated against the documents that already own parts of it (the first commit of that change is the unchanged move). This document is the current contract; the execution state is tracked in `wrightkit/wright#537` and the live issues it links, never here.

## Goal

Define Wright's path to 1.0 by stable product capabilities, semantic ownership, and verified user workflows rather than internal migration phases, feature counts, or a manually maintained progress ledger.

## Product direction

The product priority order is in [`ownership.md`](ownership.md#product-priority). Compiler and conversion work are product-enabling infrastructure, not the sole progress metric ([ADR-0008](../adr/0008-tooling-first-semantic-platform.md)).

## Ownership and dependency rules

Repository ownership and dependency direction are in [`ownership.md`](ownership.md). Missing semantics are fixed in the owning repository, never approximated or duplicated in Wright.

Editor-facing LSP and agent-facing native-tool adapters are consumer surfaces over shared Wright/owner capabilities; neither protocol owns source-language semantics.

Cross-repository work follows:

`owning repo -> owner contract/tests -> consumable release or revision -> consumer integration -> representative workflow verification`

## Readiness model

Surface area is not support ([`ownership.md`](ownership.md#capability-ceiling)): a command, adapter, parser entry point, README claim, or isolated passing test does not establish a capability by itself.

Readiness depends on the workflow:

- `check` / `inspect` require correct loading, parsing, semantic resolution, diagnostics, and source locations for the declared scope; they do not require a complete compiler.
- `lint` / `analyze` require the semantic information their claims depend on, but not necessarily complete emission.
- `compile` requires an end-to-end owner path from source semantics through canonical Workshop validation/emission.
- conversion/reconstruction is a separate capability and must be validated independently from parsing, checking, or forward compilation.
- agent/native-tool readiness requires semantic equivalence with the underlying Wright contract, discoverable project setup for supported harnesses, and representative workflow evidence for task effectiveness and context/tool overhead; shipping a transport adapter alone is not sufficient.
- language-service readiness requires correct document lifecycle/current-buffer behavior and advertises only editor capabilities backed by the owning implementation; agent-native tools do not substitute for editor document synchronization.

Compatibility is capability-specific. Observable semantic correctness is always required. An owning language engine may additionally define a stricter compiler-output contract, such as canonical structural convergence with a pinned reference implementation; Wright consumes that owner contract rather than redefining it.

## Source transformation

The mutation model (semantic understanding, validated edits, the original source, explicit refusal of unsafe edits) is in [`tooling.md`](tooling.md#source-edits).

- Workshop -> OPY/DEL reconstruction targets semantic equivalence and useful recoverable structure, with explicit information-loss boundaries rather than literal source recovery.

## Verification

Verification follows [ADR-0018](../adr/0018-tests-first-integration-verification.md) and the WrightKit testing policy: tests are the primary durable mechanism, and fixtures, corpora, snapshots, and reference outputs are test support, not parallel status or verification databases. Current support claims must reflect the current implementation and tests.

## Wright 1.0 product contract

Wright 1.0 is ready when users can rely on it as a stable Workshop tooling platform with:

- cross-platform installation and update;
- stable declared `check`, `lint`, `analyze`, `inspect`, source-edit/fix, agent/embedding, and native-tool contracts;
- canonical raw Workshop parsing, validation, analysis inputs, and emission through `workshop-rs`;
- independently usable OPY and DEL/OSTW owner implementations for the source-language scopes Wright declares supported;
- compilation/conversion only where the owning implementation and Wright integration have actually established the declared scope;
- validated source-oriented mutation with explicit refusal for unsafe operations;
- stable CI and machine-readable diagnostic/tool contracts;
- a bounded project setup path that can register Wright's own native-tool server with explicitly supported agent harnesses without requiring a second semantic adapter or a Wright-owned model runtime;
- source locations/mapping preserved where a product claim depends on authored-source attribution;
- reviewed licensing and distribution boundaries;
- user documentation that describes supported workflows without requiring knowledge of internal compiler architecture.

1.0 does not require every possible OPY/DEL feature, reconstruction path, seasonal live-client run, or speculative future tool to be complete. It requires declared public contracts and supported workflows to be intentionally stable and truthful.

## Roadmap dependency order

The durable dependency order is:

`real workflow need -> root owner capability -> tests -> integration when needed -> representative workflow validation -> supported Wright product capability`

At the ecosystem level:

1. **Engine readiness** — make `workshop-rs`, `opy-rs`, and `deltin-rs` correct for the real workflows their declared scopes depend on.
2. **Wright convergence** — expose those owner capabilities consistently through check/lint/analyze/inspect/edit/CLI/LSP/provider surfaces without semantic fallbacks.
3. **Advanced tooling** — expand agent contracts and native-tool integration/bootstrap, richer validated edits, stability/cost analysis, reconstruction/conversion, and additional language-service breadth as their dependencies become ready.

This is dependency-driven, not a rigid release-phase sequence. Higher-level work may proceed earlier when its actual prerequisites already exist.

## Planning rules

The general decision priorities are in the WrightKit `docs/goal.md`. The rules specific to Wright's roadmap:

- Keep near-term executable work in owner Issues. Keep this document focused on durable product, ownership, readiness, and prioritization contracts.
- Do not split catalog-scale work into per-symbol Issues.
- Keep agent integration thin: Wright may expose and register its own capabilities, while the harness owns model runtime, tool-exposure/discovery policy, and process startup. Do not turn Wright into a generic agent configuration framework without a separate approved requirement.

## Non-goals

- Maintaining a current-progress dashboard in this document.
- Encoding transient release versions, active blockers, current child-Issue state, or near-term execution ordering here.
- Treating roadmap completion, Issue closure, README claims, or historical CI results as proof that a capability is currently supported.
- Replacing owner-specific language or compatibility contracts with Wright-side policy.

Current execution state must be rebuilt from live Issues/PRs, current code/tests, CI/releases, and representative workflow validation.
