# Product contract: the path to 1.0

Moved unchanged from `wrightkit/wright#134` on 2026-10-06, where it was maintained as an issue body. This document is the current contract; the execution state is tracked in `wrightkit/wright#537` and the live issues it links, never here.

## Goal

Define Wright's path to 1.0 by stable product capabilities, semantic ownership, and verified user workflows rather than internal migration phases, feature counts, or a manually maintained progress ledger.

## Product direction

Wright is the unified tooling/product layer over independently owned Workshop language engines. Product priority is:

1. check / diagnostics
2. lint / static analysis
3. inspect / semantic query
4. validated source edits / refactoring
5. agent / embedding / native-tool workflows
6. CI / language services
7. Workshop stability / cost analysis
8. compilation / conversion where required by real workflows

Compiler and conversion work are product-enabling infrastructure, not the sole progress metric. A Wright surface must not claim language behavior beyond what its owning implementation actually supports.

## Ownership and dependency rules

- Raw Workshop semantics, canonical Program/WIR behavior, catalog, settings, localization, validation, emission, and Workshop-domain facts belong to `workshop-rs`.
- OPY syntax, preprocessing/macros, project loading, semantics, HIR, diagnostics/source mapping, lowering, compiler behavior, and scoped reconstruction belong to `opy-rs`.
- DEL/OSTW project loading, syntax/type/runtime semantics, HIR, diagnostics/source mapping, lowering, compiler behavior, and scoped reconstruction belong to `deltin-rs`.
- Wright owns cross-language diagnostics, lint/analyze/inspect, validated-edit product behavior, CLI/LSP/agent surfaces, embedding, CI, and orchestration. Editor-facing LSP and agent-facing native-tool adapters are consumer surfaces over shared Wright/owner capabilities; neither protocol owns source-language semantics.
- `language-provider-protocol` owns the provider process/data contract, not source-language or Workshop semantics.
- Missing semantics must be fixed in the owning repository rather than approximated or duplicated in Wright.

Cross-repository work follows:

`owning repo -> owner contract/tests -> consumable release or revision -> consumer integration -> representative workflow verification`

## Readiness model

Surface area is not support. A command, adapter, parser entry point, README claim, or isolated passing test does not establish a capability by itself.

Readiness depends on the workflow:

- `check` / `inspect` require correct loading, parsing, semantic resolution, diagnostics, and source locations for the declared scope; they do not require a complete compiler.
- `lint` / `analyze` require the semantic information their claims depend on, but not necessarily complete emission.
- `compile` requires an end-to-end owner path from source semantics through canonical Workshop validation/emission.
- conversion/reconstruction is a separate capability and must be validated independently from parsing, checking, or forward compilation.
- agent/native-tool readiness requires semantic equivalence with the underlying Wright contract, discoverable project setup for supported harnesses, and representative workflow evidence for task effectiveness and context/tool overhead; shipping a transport adapter alone is not sufficient.
- language-service readiness requires correct document lifecycle/current-buffer behavior and advertises only editor capabilities backed by the owning implementation; agent-native tools do not substitute for editor document synchronization.

Compatibility is capability-specific. Observable semantic correctness is always required. An owning language engine may additionally define a stricter compiler-output contract, such as canonical structural convergence with a pinned reference implementation; Wright consumes that owner contract rather than redefining it.

## Source transformation

- Source mutation follows `semantic understanding -> validated source edits -> original source`; full-file regeneration is not the default editing model.
- Preserve comments, trivia, formatting, and unrelated source whenever practical.
- Unsafe or unsupported edits must fail explicitly rather than fall back to destructive textual rewriting.
- Workshop -> OPY/DEL reconstruction targets semantic equivalence and useful recoverable structure, with explicit information-loss boundaries rather than literal source recovery.

## Verification

Tests are the primary durable verification mechanism.

Use the smallest verification surface appropriate to the claim:

- focused unit/regression tests for local behavior;
- compatibility/differential tests against pinned reference implementations where external behavior matters;
- integration/real-project tests for interactions not economically covered by focused tests;
- runtime/client validation only for claims that cannot be established statically;
- source mapping/source-location checks for diagnostics and validated editing.

Fixtures, corpora, snapshots, goldens, and reference outputs are test-support mechanisms, not parallel project-status or verification databases. Current support claims must reflect current implementation and tests.

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

- Real user workflows outrank roadmap neatness, support-matrix completion, documentation volume, feature counts, or architecture cleanup.
- Fix root capabilities in the owning implementation before integration-layer workarounds.
- Inspect existing tests before adding new ones; protect observable behavior and meaningful failure modes rather than incidental implementation details.
- Do not weaken diagnostics, validation, compatibility expectations, or tests to make CI pass.
- Keep near-term executable work in owner Issues. Keep this roadmap focused on durable product, ownership, readiness, and prioritization contracts.
- Do not split catalog-scale work into per-symbol Issues.
- Infrastructure, protocol, release, or cleanup work is justified when it unlocks or protects a concrete workflow.
- Keep agent integration thin: Wright may expose and register its own capabilities, while the harness owns model runtime, tool-exposure/discovery policy, and process startup. Do not turn Wright into a generic agent configuration framework without a separate approved requirement.

## Non-goals

- Maintaining a current-progress dashboard in this Issue body.
- Encoding transient release versions, active blockers, current child-Issue state, or near-term execution ordering here.
- Treating roadmap completion, Issue closure, README claims, or historical CI results as proof that a capability is currently supported.
- Replacing owner-specific language or compatibility contracts with Wright-side policy.

Current execution state must be rebuilt from live Issues/PRs, current code/tests, CI/releases, and representative workflow validation.
