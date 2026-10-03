# Wright Language Services and LSP

Status: current scope — document synchronization, diagnostics, and
provider-backed rename
Scope: editor-neutral language services (`wright-language`) and the thin LSP
adapter (`wright-lsp`)

## Architecture

```text
document/workspace model (Document, DocumentStore)
   → LanguageService (editor-neutral, no LSP types)
       └─ diagnostics (provider refusal for source documents)
            ↓
  wright-lsp (thin protocol adapter, Content-Length stdio framing)
```

All semantic logic lives in `wright-language`; `wright-lsp` only maps LSP
DTOs, so adding or removing an editor protocol never changes the compiler or
analyzer contracts.

## Current LSP scope

`wright-lsp` backs and advertises only what is implemented:

- lifecycle: `initialize`, `initialized`, `shutdown`, `exit`;
- full-document synchronization: `textDocument/didOpen`,
  `textDocument/didChange`, `textDocument/didClose` (`didSave` is an explicit
  no-op under full sync);
- `textDocument/publishDiagnostics`: versioned, grouped by source identity,
  with didClose cleanup.

The `initialize` result advertises `textDocumentSync`, the UTF-16 position
encoding, and `renameProvider`. It contains no provider entry for hover,
definition, references, completion, or semantic tokens: those capabilities
have no backing implementation and are never advertised. A request for an
unadvertised or unknown method receives a `result: null` response and the
server keeps running.

Diagnostics: opening a source-language document (`.opy`, `.del`, `.ostw`)
publishes an explicit `source-provider-unavailable` error while no provider
language-service capability is negotiated. A raw Workshop document (or any
document without a source language) publishes no diagnostics.

`textDocument/rename` on a source-language document routes through the same
provider-owned mutation path as the CLI and agent surfaces (#156): the
provider computes edits over the open document set, Wright verifies document
versions and source preconditions, asks the provider to validate the
transaction, and rechecks the edited project before returning a
`WorkspaceEdit`. Unsupported documents (`rename-unsupported-document`),
unopen documents (`rename-unknown-document`), unconfigured providers, and
provider or validation refusals answer with a `RequestFailed` error whose
`error.data.code` carries the structured refusal code — there is no textual
search/replace fallback. `--opy-provider <PATH>` points the session at an
explicit OPY provider executable; by default the resolver locates or
downloads the released provider like the CLI does.

Provider-backed editor capabilities beyond rename — hover, definition,
references, completion, and semantic tokens — are future work tracked under
#156 and the owning implementations (for example `opy-rs` language-service
capabilities). They arrive through provider capability negotiation, not
through Wright-side reimplementation or static fallbacks.

## Editor-neutral contracts

* `Document`: URI identity, current text, monotonic internal `version`,
  project root (include base).
* `DocumentStore`: open/change/close lifecycle with version bumping.
* Positions and ranges are 0-based editor conventions; the service converts to
  the compiler's 1-based spans at the boundary. UTF-16 ↔ character conversion
  is centralized in `wright_language::document` (`utf16_offset_to_char`,
  `char_offset_to_utf16`, `span_to_range`) and is the
  only path that consumes or emits editor positions; no UTF-16 offset is ever
  used as a byte index.
* File URI ↔ filesystem path conversion is centralized in
  `wright_language::document` (`uri_to_path`, `path_to_uri`) using the
  standard URL parser, covering percent-encoding, spaces, Unicode filenames,
  and platform drive paths.
* Every result carries `document_version`; stale results are detectable and
  replaceable.

## Incremental behavior

Reanalysis is a deterministic full recomputation over the changed document.
Provider capabilities are not reimplemented locally for latency or feature-
matrix symmetry. The document/version and transport contracts remain local to
Wright and continue to work when a provider returns an explicit refusal.

## Responsiveness contract

The responsiveness requirement is **stale/current-state correctness, not
in-flight request cancellation**. Synchronous deterministic full recomputation
is the implemented contract: results are version-tagged, stale/out-of-order
client versions are rejected and cannot overwrite newer state, obsolete
results are never surfaced as current, and every query re-reads the current
document state. True in-flight request cancellation is explicitly deferred.

Re-evaluation trigger (measured, testable): re-open the true-cancellation or
incremental-analysis decision when any of the following holds on the current
machine baseline:

- a measured perf workload mean for `diagnostics` exceeds the recorded
  regression bound (200 ms per workflow); or
- peak RSS on the perf workload exceeds 1 GB; or
- a representative project-scale measurement (a multi-file corpus of at least
  20 source files, or the declared representative project) shows any single
  interactive request exceeding 100 ms.

Until a trigger fires, bounded full recomputation with stale-result
suppression is the authoritative contract.

## Diagnostics

The service produces source-aware `SourceDiagnostic`s. Today the only
diagnostic is the explicit `source-provider-unavailable` refusal for
source-language documents, which marks where a negotiated provider
capability would attach; it is never a static fallback result.

## DEL/OSTW documents (#120)

`.ostw`/`.del` documents remain recognized entrypoints for a future source
provider, but Wright no longer carries a static DEL/OSTW implementation. The
service reports `source-provider-unavailable` as a structured source error
for these documents. It never invokes an upstream compiler or a removed
Wright implementation as a fallback.

Workshop → OPY reconstruction is available only when the provider negotiates
`reconstruct`; Workshop → OSTW is refused at the provider boundary until a
provider is configured.

## LSP adapter

`wright-lsp` (stdio, Content-Length framing) implements the scope listed
under *Current LSP scope* above and nothing else. The end-to-end harness
(`wright-lsp/tests/lsp.rs`) drives the real binary and verifies capability
negotiation, lifecycle, the diagnostics boundary, and null responses to
unadvertised requests.

## Out of scope (recorded)

VS Code/browser extensions, incremental diff-based reanalysis (full
recomputation is deterministic and fast enough), client-side behavior
scenarios, and provider-backed editor features until a provider capability
exists remain future work.
