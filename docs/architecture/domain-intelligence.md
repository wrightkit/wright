# Wright Domain Intelligence Contract

Wright exposes Workshop domain intelligence as a composed query over existing semantic owners. It does not own a second Workshop catalog or a source-language semantic model.

Decision history: [ADR-0017](../adr/0017-domain-intelligence-query-contract.md).

## Purpose

Agent, CLI, embedding, and transport consumers need bounded context about the semantic construct they are working with: canonical Workshop facts, relevant source-language context, and practical guidance. They obtain that context through one Wright-owned query contract instead of loading a monolithic prompt or reimplementing owner lookups.

## Ownership

| Information class | Authority |
| --- | --- |
| Canonical Workshop identities, signatures, settings, localization, gameplay facts, and other Workshop semantics | `workshop-rs` |
| OverPy syntax/semantic context and mapping from OverPy source constructs | `opy-rs` |
| DEL/OSTW syntax/semantic context and mapping from DEL/OSTW source constructs | `deltin-rs` |
| Domain-intelligence query composition, public result contract, built-in guidance curation, and product presentation | Wright |
| Built-in human-authored guidance | Wright-curated first-party corpus; it remains guidance rather than semantic authority |

A guidance item may refer to a canonical Workshop identity, but it must not restate copied catalog data as an independent authority. If a canonical fact is needed, the query resolves it from its owner.

External or project-configured guidance sources are not part of the initial contract. A future source/distribution contract may add them without changing canonical ownership, but Wright does not accept arbitrary guidance sources until trust, provenance, conflict, and update behavior are defined.

## Selection

A domain-intelligence request selects a bounded semantic subject through one of two forms:

1. **Canonical selection** supplies one or more opaque canonical identities issued by the owning Workshop contract. Localized display names are not identities. Canonical selection does not require a loaded source project.
2. **Source selection** supplies a language ID plus source/document identity and position. Wright delegates semantic resolution to the owning implementation or configured provider and composes the returned canonical subject references with any owner-supplied source context. Source selection requires the relevant project/provider context.

Wright must not infer a source selection with textual matching when the owner cannot resolve it. Missing owner capability is an explicit unavailable result.

The logical request shape is:

```text
DomainIntelligenceRequest
  selector = Canonical([SemanticRef...])
           | Source(language_id, document, position)
  include  = items | source_context
```

`include` is a projection over the selected subjects. The service does not return the complete guidance corpus or catalog by default.

## Response model

The shared result is structured rather than prompt text:

```text
DomainIntelligenceResult
  subjects[]
  items[]
  source_context[]
  unavailable[]
```

Each subject carries its owner, semantic kind, canonical or owner-specific identity, and presentation metadata when available. Wright may join information by stable owner identity, but it does not invent a cross-language semantic identity when no owner mapping exists.

Each `item` has one simple public classification:

- `fact`: owner-backed factual information about the selected semantic subject;
- `warning`: Wright-curated guidance about a meaningful risk, cost, or behavior that commonly deserves attention;
- `info`: Wright-curated recommendation, usage pattern, caveat, or explanatory context.

These classifications are domain-intelligence presentation semantics, not lint/diagnostic severity. A `fact` is not an error, and `warning`/`info` guidance does not become a correctness diagnostic merely because it is returned by the query.

Facts remain machine-readable where the owner exposes structured data. Guidance has a stable guidance ID, applicability to semantic subjects, provenance, limitations when needed, and human-readable content. Guidance remains independently editable from canonical facts.

`source_context` contains source-owner information needed to explain how the selected source construct relates to the subject. Wright standardizes the envelope and provenance, not the source language's internal semantic model.

`unavailable` reports missing owner capability or unavailable source context explicitly. Wright does not guess a replacement fact or silently downgrade to text matching.

## Provenance

Every returned item identifies its responsible owner or guidance source and carries a traceable locator or evidence reference where one exists.

The public query contract does not require consumers to understand a detailed evidence taxonomy such as runtime evidence, heuristic, or community guidance, and it does not assign synthetic numeric confidence scores. Richer evidence metadata may exist in the owning fact source or guidance corpus for curation and review, but it does not replace the simple `fact` / `warning` / `info` contract.

Human-authored guidance cannot self-promote to a canonical fact. Provenance must remain sufficient to tell owner-backed facts from Wright-curated guidance.

## Public service boundary

The shared product operation is `domainIntelligence` on Wright's common query/tooling surface. CLI, embedding, stdio/JSON-RPC, and future agent adapters reuse that operation rather than creating agent-specific semantics.

The operation may be exposed through the existing ToolService facade, but canonical selection must not require an eagerly loaded project session. Source selection may use session/provider state because resolving source positions is inherently project- and owner-dependent.

The operation is additive within the existing `wright-embedding/v1` / `wright-result/v1` compatibility rules. Adding it does not require a new major contract version; removing or incompatibly changing its declared fields does.

Existing `targetMetadata` remains a catalog/discovery summary. It is not the domain-intelligence contract because it exposes broad target metadata rather than a selected, provenance-bearing composition of facts and guidance.

## Provider boundary

This contract does not require LPP to grow a source-selection method before evidence demands it. An in-process owner or provider may support source selection through an owner-backed semantic query. If a configured provider cannot resolve a source position to semantic subjects, Wright returns the capability gap explicitly.

A future LPP source-selection capability is a separate protocol decision and must preserve the same ownership rule: the provider resolves source-language meaning; Wright composes product context.

## Guidance boundary

The initial built-in guidance corpus is curated by Wright. Guidance is human-authored content plus machine-readable identity, semantic applicability, provenance, and limitations where needed. Its storage syntax and distribution mechanism are not semantic contracts and may evolve without moving canonical Workshop facts into Wright.

This initial ownership choice does not require all future guidance to be authored by Wright. Additional community or project guidance sources may be added later if a concrete source/distribution contract defines trust, provenance, conflict, and update behavior.

Wright does not require a package manager, executable plugin runtime, remote knowledge database, project-policy overlay, or prompt template to serve the initial contract. Those mechanisms require separate evidence and decisions.
