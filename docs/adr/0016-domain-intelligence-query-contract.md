# ADR-0016: Domain-intelligence query contract

- Status: Proposed
- Date: 2026-09-14
- Related: [Issue #323](https://github.com/wrightkit/wright/issues/323), [ADR-0010](0010-independent-implementations-and-wright-integration.md), [ADR-0015](0015-canonical-facts-and-declarative-lint-policy.md)

## Context

Wright already exposes shared query/tooling services for CLI, embedding, and transport consumers. Its `targetMetadata` operation is a broad catalog summary, while ADR-0015 separates owner-backed canonical facts from Wright lint/policy behavior.

Agent workflows need a narrower capability: given the semantic construct currently being worked on, retrieve only the relevant Workshop facts, source-owner context, and practical guidance. Serving that through static prompts or copied metadata would duplicate semantic authority, make provenance ambiguous, and force each consumer to implement its own lookup behavior.

The design must also support human-authored best practices without allowing guidance to become an implicit correctness specification.

## Decision

Wright adds one product-level domain-intelligence composition contract with these rules:

1. **The service is a query, not a semantic store.** Canonical Workshop facts and identities are resolved from `workshop-rs`; source-language meaning is resolved by the source owner. Wright owns composition and presentation.
2. **Selection is semantic and bounded.** A request selects canonical owner identities directly, or supplies a source location that an owning implementation/provider resolves to semantic subjects. Wright does not fall back to textual guessing when source resolution is unavailable.
3. **Canonical lookup is project-independent.** Direct canonical selection must work without an eagerly loaded source project. Source-position selection may require project/provider state because source resolution is owner-dependent.
4. **The public information model stays simple.** Returned domain-intelligence items use `fact`, `warning`, or `info`. `fact` is owner-backed factual information; `warning` and `info` are Wright-curated guidance at different levels of attention. These are domain-intelligence presentation classes, not lint/diagnostic severity.
5. **Provenance is explicit without imposing a public evidence taxonomy.** Each item identifies its responsible owner or guidance source and carries traceable references where available. Consumers are not required to understand a separate enum for runtime evidence, heuristics, or community guidance, and Wright does not synthesize numeric confidence scores.
6. **Built-in guidance is curated by Wright.** The initial corpus is first-party Wright guidance with stable identity, semantic applicability, provenance, and limitations where needed. Human guidance cannot self-promote to canonical semantics.
7. **External guidance sources are deferred.** Community or project-configured guidance may be added later only after a concrete contract defines trust, provenance, conflict, and update behavior. The initial design does not add a project-policy overlay or arbitrary guidance loading.
8. **All product consumers reuse the same operation.** `domainIntelligence` belongs on Wright's common query/tooling surface. CLI, embedding, stdio/JSON-RPC, and future agent adapters remain thin consumers of that contract.
9. **The change is additive to the existing v1 embedding/result compatibility rules.** A new major contract version is not required solely to add the operation.
10. **LPP is not extended pre-emptively.** Provider-backed source selection may expose a future protocol gap; until evidence requires a wire capability, unsupported source selection is reported explicitly rather than forcing a speculative LPP extension.

`targetMetadata` remains a broad target/catalog discovery summary and is not repurposed as the domain-intelligence operation.

## Alternatives considered

- **Reuse `targetMetadata` and add more fields:** rejected because a bulk catalog response does not express semantic selection, source-owner resolution, or provenance-bearing guidance, and would trend toward a monolithic agent payload.
- **Require a loaded project/session for every query:** rejected because canonical Workshop facts and built-in guidance are useful without source-project context; only source-position resolution inherently needs owner/project state.
- **Expose a detailed public evidence taxonomy:** rejected because consumers primarily need a clear fact-versus-guidance distinction and provenance. Runtime/community/heuristic detail can remain source metadata where useful without becoming the main product model.
- **Copy Workshop facts into Wright guidance assets:** rejected because it creates a shadow semantic authority that can drift from `workshop-rs`.
- **Put best-practice guidance in `workshop-rs`:** rejected because product guidance is not canonical Workshop semantics.
- **Accept arbitrary community/project guidance in v1:** deferred because the system does not yet define trust, provenance, conflict, or update behavior for external sources. Wright curates the initial built-in corpus while keeping the item contract open to future sources.
- **Add project-policy suppression/ranking in v1:** rejected because there is no current consumer requirement for a domain-intelligence policy overlay; lint policy should not be generalized into this product surface without evidence.
- **Create a general knowledge database, plugin runtime, or package manager first:** rejected because no current consumer evidence requires those mechanisms and they add independent versioning/distribution concerns.
- **Extend LPP first:** deferred because canonical identity queries and in-process owner integrations do not require a new wire method; a provider protocol change should follow an observed provider-backed source-selection need.
- **Embed prompt-ready text in Wright core:** rejected because prompt formatting is consumer presentation, while the durable contract should remain structured and reusable across CLI, embedding, and other agents.

## Consequences

- Agents can request small, context-specific payloads instead of loading the entire Workshop knowledge surface.
- Canonical facts remain owned and updated where their semantics live.
- Simple `fact` / `warning` / `info` presentation keeps the agent-facing contract understandable while provenance preserves traceability.
- Canonical lookups can serve reference-style workflows without requiring a source project.
- Wright can evolve a curated guidance corpus without creating a second Workshop semantic authority.
- Source-owner capability gaps remain visible rather than being hidden by Wright text heuristics.
- Implementation must add a public operation/result shape and provide contract tests across the shared query surface and transport adapters.
- A provider-backed source selector may later require a separate LPP decision, but that work is not required by this ADR.

## Compatibility impact

The decision adds a new public query operation without changing source-language syntax, Workshop semantics, compiler lowering, diagnostics, or existing query behavior. Under the current embedding/result versioning rules, adding the operation is compatible within major version 1.

Implementation evidence must show that direct/in-process and transport consumers observe the same structured result, direct canonical queries do not require unnecessary project loading, owner facts are not duplicated as Wright authority, and unavailable owner capabilities are surfaced explicitly.

## Scope boundaries

This ADR does not decide:

- the storage path or serialization syntax used to author first-party guidance;
- community/project guidance source loading or remote guidance distribution;
- registries or executable plugins;
- project-specific suppression/ranking policy for guidance;
- prompt templates or token-cache optimization;
- a new LPP method for semantic selection;
- automatic source edits based on guidance;
- whether every source-language construct can map to a canonical Workshop identity.

Those decisions require separate concrete consumer or provider evidence.
