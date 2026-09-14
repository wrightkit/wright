# ADR-0016: Domain-intelligence query contract

- Status: Proposed
- Date: 2026-09-14
- Related: [Issue #323](https://github.com/wrightkit/wright/issues/323), [ADR-0010](0010-independent-implementations-and-wright-integration.md), [ADR-0015](0015-canonical-facts-and-declarative-lint-policy.md)

## Context

Wright already exposes a session-aware tool service for CLI, embedding, and transport consumers. Its `targetMetadata` operation is a broad catalog summary, while ADR-0015 separates owner-backed canonical facts from Wright lint/policy and evidence classification.

Agent workflows need a narrower capability: given the semantic construct currently being worked on, retrieve only the relevant Workshop facts, source-owner context, evidence-backed guidance, and project policy. Serving that through static prompts or copied metadata would duplicate semantic authority, make provenance ambiguous, and force each consumer to implement its own lookup behavior.

The design must also support human-authored best practices without allowing community guidance to become an implicit correctness specification.

## Decision

Wright adds one product-level domain-intelligence composition contract with these rules:

1. **The service is a query, not a semantic store.** Canonical Workshop facts and identities are resolved from `workshop-rs`; source-language meaning is resolved by the source owner. Wright owns composition and presentation.
2. **Selection is semantic and bounded.** A request selects canonical owner identities directly, or supplies a source location that an owning implementation/provider resolves to semantic subjects. Wright does not fall back to textual guessing when source resolution is unavailable.
3. **Responses keep information classes separate.** The shared result has subject, fact, source-context, guidance, project-policy, and unavailable sections. Wright standardizes their envelope and provenance but does not normalize source-language internals into a new cross-language semantic model.
4. **Evidence is explicit.** Returned items distinguish `exact`, `static_metric`, `runtime_evidence`, `heuristic`, and `community_guidance`. Evidence class plus authority is the confidence contract; no synthetic numeric confidence score is introduced.
5. **Human guidance stays separate from canonical facts.** Guidance is authored as content plus stable identity, applicability, evidence/provenance, and limitations. Wright may curate and distribute first-party guidance, but guidance cannot self-promote to canonical semantics.
6. **Project policy is an overlay.** It may select, suppress, or rank guidance for a consumer/project, but it cannot rewrite facts, provenance, or evidence classification.
7. **All product consumers reuse the same operation.** `domainIntelligence` belongs on the existing session-aware Wright tool/query service. CLI, embedding, stdio/JSON-RPC, and future agent adapters remain thin consumers of that contract.
8. **The change is additive to the existing v1 embedding/result compatibility rules.** A new major contract version is not required solely to add the operation.
9. **LPP is not extended pre-emptively.** Provider-backed source selection may expose a future protocol gap; until evidence requires a wire capability, unsupported source selection is reported explicitly rather than forcing a speculative LPP extension.

`targetMetadata` remains a broad target/catalog discovery summary and is not repurposed as the domain-intelligence operation.

## Alternatives considered

- **Reuse `targetMetadata` and add more fields:** rejected because a bulk catalog response does not express semantic selection, source-owner resolution, guidance provenance, or project policy, and would trend toward a monolithic agent payload.
- **Copy Workshop facts into Wright guidance assets:** rejected because it creates a shadow semantic authority that can drift from `workshop-rs`.
- **Put best-practice guidance in `workshop-rs`:** rejected because community heuristics and product guidance are not canonical Workshop semantics.
- **Create a general knowledge database, plugin runtime, or package manager first:** rejected because no current consumer evidence requires those mechanisms and they add independent versioning/distribution concerns.
- **Extend LPP first:** deferred because canonical identity queries and in-process owner integrations do not require a new wire method; a provider protocol change should follow an observed provider-backed source-selection need.
- **Embed prompt-ready text in Wright core:** rejected because prompt formatting is consumer presentation, while the durable contract should remain structured and reusable across CLI, embedding, and other agents.

## Consequences

- Agents can request small, context-specific payloads instead of loading the entire Workshop knowledge surface.
- Canonical facts remain owned and updated where their semantics live.
- Community guidance can evolve independently while retaining explicit evidence and limitations.
- Source-owner capability gaps remain visible rather than being hidden by Wright text heuristics.
- Implementation must add a public operation/result shape and provide contract tests across the shared tool service and transport adapters.
- A provider-backed source selector may later require a separate LPP decision, but that work is not required by this ADR.

## Compatibility impact

The decision adds a new public query operation without changing source-language syntax, Workshop semantics, compiler lowering, diagnostics, or existing query behavior. Under the current embedding/result versioning rules, adding the operation is compatible within major version 1.

Implementation evidence must show that direct/in-process and transport consumers observe the same structured result, owner facts are not duplicated as Wright authority, and unavailable owner capabilities are surfaced explicitly.

## Scope boundaries

This ADR does not decide:

- the storage path or serialization syntax used to author first-party guidance;
- remote guidance distribution, registries, or executable plugins;
- prompt templates or token-cache optimization;
- a new LPP method for semantic selection;
- automatic source edits based on guidance;
- whether every source-language construct can map to a canonical Workshop identity.

Those decisions require separate concrete consumer or provider evidence.