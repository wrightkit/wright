# ADR-0023: LSP scope for 1.0

- Status: Accepted (amended 2026-12 for OPY diagnostics and navigation, #738)
- Date: 2026-10-06
- Related: [Issue #421](https://github.com/wrightkit/wright/issues/421), [Issue #429](https://github.com/wrightkit/wright/issues/429), [Issue #738](https://github.com/wrightkit/wright/issues/738), [Product contract](../architecture/product-contract.md), [Language services](../language-services.md)

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

## Amendment — provider-backed OPY services (#738)

The 1.0 scope now includes OPY diagnostics and navigation, delivered through the negotiated OPY provider rather than an in-process implementation:

- **Diagnostics** are `lpp/check` over the complete open-buffer overlay — every open `.opy` document's current, unsaved text under the client's URI — never a disk read Wright performs itself.
- **Navigation** is `lpp/symbols` for `textDocument/documentSymbol`, `lpp/definition`, and `lpp/references`, including authored locations inside `#!include`d files.

Two constraints from the original decision still bind:

- The `initialize` result advertises only what the negotiated provider backs, so the provider is spawned and `lpp/initialize` is negotiated **inside the LSP `initialize` handshake** — `documentSymbolProvider` appears only when `lpp/symbols` negotiated. `hoverProvider`, `definitionProvider`, and `referencesProvider` keep their unconditional advertisement because raw Workshop backing is in-process; OPY requests against an unnegotiated provider capability answer `null`/`[]` rather than an error.
- A session without a resolvable provider still initializes; OPY surfaces degrade to the structured boundary instead of failing the handshake.

Non-goals are unchanged: OPY hover, completion, semantic tokens, and all DEL/OSTW services remain outside 1.0 until the owning implementations negotiate those capabilities.
