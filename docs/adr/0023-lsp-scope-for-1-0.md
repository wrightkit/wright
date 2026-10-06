# ADR-0023: LSP scope for 1.0

- Status: Proposed
- Date: 2026-10-06
- Related: [Issue #421](https://github.com/wrightkit/wright/issues/421), [Issue #134](https://github.com/wrightkit/wright/issues/134), [Product contract](../architecture/product-contract.md), [Language services](../language-services.md)

Recorded from the decision comment of 2026-09-29 on `#134` and on `#421`, moved here because a decision belongs in an ADR and not in an issue thread.

## Decision

The language-service scope for 1.0 is **B**: diagnostics and document lifecycle, plus raw Workshop language services from the canonical `workshop-rs` program and the existing `wright-analyzer` `SemanticIndex`. Provider-backed OverPy and DEL/OSTW services stay gated on the owning implementations' capabilities, and `wright-lsp` advertises only capabilities that are backed.

## Consequences

Editor-facing services are a consumer surface over the same Wright and owner capabilities as the CLI; agent-native tools do not substitute for editor document synchronization.
