# ADR-0023: LSP scope for 1.0

- Status: Proposed
- Date: 2026-10-06
- Related: [Issue #421](https://github.com/wrightkit/wright/issues/421), [Issue #429](https://github.com/wrightkit/wright/issues/429), [Product contract](../architecture/product-contract.md), [Language services](../language-services.md)

The decision was made on 2026-09-29 and recorded on `#421` and `#134`; it is moved here because a decision belongs in an ADR and not in an issue thread.

## Context

`wright-lsp` ships in every release archive, while its language-service handlers for hover, definition, references, completion, rename, and semantic tokens returned empty results and the server advertised some of them anyway. `#421` makes the `initialize` result advertise only capabilities with a real backing implementation, which at that time was document sync, and leaves the 1.0 scope as a separate decision. This ADR records that decision.

Two scopes were considered:

- **A.** Diagnostics and lifecycle only, with provider-backed features added as owner capabilities appear.
- **B.** Also ship raw Workshop language services from the canonical `workshop-rs` program and the existing `wright-analyzer` `SemanticIndex`, which already holds symbols, references, and spans. Diagnostics match `wright check`.

## Decision

The scope for 1.0 is **B**: diagnostics and document lifecycle, plus raw Workshop language services from the canonical `workshop-rs` program and the `wright-analyzer` `SemanticIndex`. Provider-backed OverPy and DEL/OSTW services stay future work, gated on the owning implementations' capabilities.

Rationale: `SemanticIndex` already holds symbols, references, and spans, so hover, definition, and references for raw Workshop are a real capability and not new analysis. Declaring only diagnostics (A) would understate what the implementation backs, which is the same kind of untruthful declaration as advertising what is not backed, in the opposite direction.

## Constraints

- B is what Wright commits to declare by 1.0, not what is implemented when this is written. The `initialize` result advertises only what is backed at the time of each change; the implementation work for B is tracked in separate issues.
- `wright-lsp` advertises only capabilities backed by the owning implementation, and agent-native tools do not substitute for editor document synchronization.
- The editor surface and the CLI/agent query surface (`#429`) share one resolution layer in `wright-driver` and `wright-analyzer`; neither grows its own.

## Consequences

Editor-facing services are a consumer surface over the same Wright and owner capabilities as the CLI. Readers must not take B as implemented: current LSP capability claims are stated in [language services](../language-services.md) and established by the code and its tests.
