# Wright Domain Intelligence Contract

Wright exposes Workshop domain intelligence as a composed query over existing semantic owners. It does not own a second Workshop catalog or a source-language semantic model.

Decision history: [ADR-0016](../adr/0016-domain-intelligence-query-contract.md).

## Purpose

Agent, CLI, embedding, and transport consumers need bounded context about the semantic construct they are working with: canonical Workshop facts, relevant source-language context, evidence-backed guidance, and effective project policy. They obtain that context through one Wright-owned query contract instead of loading a monolithic prompt or reimplementing owner lookups.

## Ownership

| Information class | Authority |
| --- | --- |
| Canonical Workshop identities, signatures, settings, localization, gameplay facts, and other Workshop semantics | `workshop-rs` |
| OverPy syntax/semantic context and mapping from OverPy source constructs | `opy-rs` |
| DEL/OSTW syntax/semantic context and mapping from DEL/OSTW source constructs | `deltin-rs` |
| Domain-intelligence query composition, evidence envelope, first-party guidance policy, project-policy application, and product presentation | Wright |
| Human-authored guidance content | Its declared author/source; Wright curates and serves it but does not promote it to semantic authority |
| Project-specific enablement/preferences | The consuming project/configuration; Wright applies them without changing underlying evidence |

A guidance document may refer to a canonical Workshop identity, but it must not restate copied catalog data as an independent authority. If a canonical fact is needed, the query resolves it from its owner.

## Selection

A domain-intelligence request selects a bounded semantic subject through one of two forms:

1. **Canonical selection** supplies one or more opaque canonical identities issued by the owning Workshop contract. Localized display names are not identities.
2. **Source selection** supplies a language ID plus source/document identity and position. Wright delegates semantic resolution to the owning implementation or configured provider and composes the returned canonical subject references with any owner-supplied source context.

Wright must not infer a source selection with textual matching when the owner cannot resolve it. Missing owner capability is an explicit unavailable result.

The logical request shape is:

```text
DomainIntelligenceRequest
  selector = Canonical([SemanticRef...])
           | Source(language_id, document, position)
  include  = facts | source_context | guidance | policy
```

`include` is a projection over the selected subjects. The service does not return the complete guidance corpus or catalog by default.

## Response model

The shared result is structured rather than prompt text:

```text
DomainIntelligenceResult
  subjects[]
  facts[]
  source_context[]
  guidance[]
  policy[]
  unavailable[]
```

Each subject carries its owner, semantic kind, canonical or owner-specific identity, and presentation metadata when available. Wright may join information by stable owner identity, but it does not invent a cross-language semantic identity when no owner mapping exists.

`facts` contain machine-readable owner-backed data. `source_context` contains source-owner information needed to explain how the selected source construct relates to the subject. Wright standardizes the envelope and provenance, not the source language's internal semantic model.

`guidance` contains human-authored recommendations, caveats, or usage patterns. Guidance has a stable guidance ID, applicability to semantic subjects, provenance/evidence references, limitations, and human-readable content. Guidance remains independently editable from canonical facts.

`policy` reports only the effective project/consumer policy relevant to the selected guidance or query. Policy may select, suppress, or rank guidance, but it cannot rewrite canonical facts, provenance, or evidence classification.

`unavailable` reports missing owner capability or unavailable evidence explicitly. Wright does not guess a replacement fact or silently downgrade to text matching.

## Evidence and provenance

Every returned fact or guidance item declares its evidence class and provenance. The durable evidence classes are:

- `exact`: owner-backed semantic fact or invariant;
- `static_metric`: deterministic measurement of the current program/artifact;
- `runtime_evidence`: observation from an identified Workshop/runtime experiment;
- `heuristic`: bounded inference with documented limitations and false-positive risk;
- `community_guidance`: experience-backed recommendation that is not semantic authority.

Evidence class and authority are the confidence contract. Wright does not assign a synthetic numeric confidence score. Human-authored metadata cannot self-promote a heuristic or community claim to `exact`.

Provenance must identify the responsible owner/source and a traceable locator or evidence reference where one exists. Runtime and community claims require enough provenance to distinguish them from canonical semantics.

## Public service boundary

The shared product operation is `domainIntelligence` on Wright's existing session-aware tool/query service. CLI, embedding, stdio/JSON-RPC, and future agent adapters reuse that operation rather than creating agent-specific semantics.

The operation is additive within the existing `wright-embedding/v1` / `wright-result/v1` compatibility rules. Adding it does not require a new major contract version; removing or incompatibly changing its declared fields does.

Existing `targetMetadata` remains a catalog/discovery summary. It is not the domain-intelligence contract because it exposes broad target metadata rather than a selected, provenance-bearing composition of facts and guidance.

## Provider boundary

This contract does not require LPP to grow a source-selection method before evidence demands it. An in-process owner or provider may support source selection through an owner-backed semantic query. If a configured provider cannot resolve a source position to semantic subjects, Wright returns the capability gap explicitly.

A future LPP source-selection capability is a separate protocol decision and must preserve the same ownership rule: the provider resolves source-language meaning; Wright composes product context.

## Guidance boundary

First-party guidance is distributed as human-authored content plus machine-readable identity, applicability, evidence, provenance, and limitation metadata. Its storage syntax and distribution mechanism are not semantic contracts and may evolve without moving canonical Workshop facts into Wright.

Wright does not require a package manager, executable plugin runtime, remote knowledge database, or prompt template to serve this contract. Those mechanisms require separate evidence and decisions.