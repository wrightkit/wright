# ADR-0013: Entry-based source-provider integration seam

- Status: Accepted (backfilled)
- Date: 2026-09-12 (backfill date)
- Clarifies: [ADR-0010: Independent language implementations and Wright integration](0010-independent-implementations-and-wright-integration.md)
- Related: [Issue #243](https://github.com/wrightkit/wright/issues/243),
  [PR #247](https://github.com/wrightkit/wright/pull/247),
  [Issue #246](https://github.com/wrightkit/wright/issues/246),
  [PR #304](https://github.com/wrightkit/wright/pull/304),
  [language-provider-protocol #16](https://github.com/wrightkit/language-provider-protocol/issues/16)

## Historical note

This record was backfilled on 2026-09-12. The decision emerged through Issue
#243, the boundary implementation and review in PR #247, and the canonical
lint/analyze handoff in Issue #246 and PR #304; this ADR did not exist when
those changes landed.

## Context

Source languages have different project models. Raw Workshop is a single
source, OPY owns `#!mainFile`, includes, preprocessing, and source closure,
and DEL/OSTW owns its project discovery and imports. A Wright-side generic
workspace scanner or client-supplied full document set would make Wright a
second source-language owner.

Wright also needs to keep owner diagnostics, provider failures, canonical
Workshop output, source identity, and truthful provenance distinct. The LPP
process contract is a transport boundary, not the Wright product model.

## Decision

The Wright product seam carries only:

- the selected source language;
- the user-selected entry path or source target; and
- invocation context needed to resolve that target, with the CLI current
  working directory as the default base for relative paths.

Wright selects the owner path and delegates project/source discovery to that
owner. Raw Workshop stays in-process through `workshop-rs`; provider-backed
source languages receive the entry through the owner-published provider
contract. The seam does not expose provider lifecycle, `DocumentSet`, provider
AST/HIR/WIR, or a Wright-owned project/module graph.

Results preserve three classes:

1. source-language diagnostics from the owner;
2. structured provider/process failures; and
3. Wright-owned lint/analyze results built from canonical Workshop evidence.

Provider-returned canonical Workshop is parsed and validated through the
canonical Workshop owner. Canonical evidence is mapped back to authored source
only when the contract carries a real mapping. Otherwise it is explicitly
unmapped/generated/provider-artifact evidence; Wright never fabricates source
locations. Provider-backed stdin is refused when the entry-based contract
cannot represent it rather than being assigned a synthetic path.

## Alternatives considered

- **A generic Wright workspace scanner:** rejected because it would duplicate
  OPY/DEL project discovery and source-closure semantics.
- **Passing a full `DocumentSet` from Wright to every provider:** rejected
  because it makes transport/session mechanics part of the product boundary and
  is not required for the entry-based CLI workflow.
- **Expose provider AST/HIR/WIR to Wright tooling:** rejected because it leaks
  owner internals and encourages duplicated source semantics.
- **Attribute generated canonical output to the selected source path:**
  rejected because it creates false provenance and misleading diagnostics.

## Consequences

- `check`, `compile`, `lint`, and `analyze` share a narrow target-resolution
  boundary while each owner retains its source/project semantics.
- Provider transport and document-coordinate details remain adapter concerns.
- Wright can reuse canonical tooling for raw Workshop and provider-backed
  source workflows without claiming language-owner completeness.
- An owner or protocol gap is routed to that owner; Wright does not add a
  semantic fallback to keep a command apparently working.

## Compatibility impact

The seam preserves structured diagnostics, source identity, canonical semantic
evidence, and explicit failure classes. It intentionally does not promise
source-span mapping when the owner/provider cannot supply one. LPP wire
versions and coordinate encodings remain owned by the protocol and adapter,
not by this product-level ADR.

## Scope boundaries

- This ADR does not define a source-map contract for authored-source
  attribution; adding one requires owner/protocol evidence.
- This ADR does not decide DEL/OSTW provider delivery; any such integration
  requires owner-backed evidence and a separate decision when material.
