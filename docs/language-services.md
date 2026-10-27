# Wright language services and LSP

Status: current scope, covering document synchronization, diagnostics, raw
Workshop hover/definition/references, provider-backed OPY
diagnostics/symbols/definition/references, and provider-backed rename
Scope: editor-neutral language services (`wright-language`) and the thin LSP
adapter (`wright-lsp`)

## Architecture

```text
document/workspace model (Document, DocumentStore)
   → LanguageService (editor-neutral, no LSP types)
       └─ diagnostics + semantic queries (canonical check/index for raw
          Workshop, the negotiated OPY provider for `.opy`, a provider
          boundary for the rest)
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
  with didClose cleanup;
- raw Workshop semantic queries (#555): `textDocument/hover`,
  `textDocument/definition`, `textDocument/references`;
- provider-backed OPY services (#738): `textDocument/documentSymbol`,
  `textDocument/definition`, `textDocument/references`, and diagnostics.

The `initialize` result advertises `textDocumentSync`, the UTF-16 position
encoding, and `hoverProvider`/`definitionProvider`/`referencesProvider`
unconditionally, since they are backed for raw Workshop documents, plus
`renameProvider` when the client negotiates
`workspace.workspaceEdit.documentChanges` (applied renames are returned as
versioned `TextDocumentEdit`s, and a client that cannot receive the
validated document version cannot be handed an edit safely).
`documentSymbolProvider` is advertised only when the OPY provider negotiated
`lpp/symbols`: the provider is spawned and `lpp/initialize` negotiated
inside the LSP `initialize` handshake so the answer reflects the negotiated
surface exactly. The result contains no provider entry for completion or
semantic tokens: those capabilities have no backing implementation and are
never advertised. A request for an unadvertised or unknown method receives a
`result: null` response and the server keeps running, as does a request the
negotiated provider cannot serve.

Diagnostics: opening or changing a raw Workshop document publishes the same
diagnostics `wright check` reports for the buffer text, with code, severity,
and span carried through and an empty list for a clean buffer (#555). Opening
or changing an `.opy` document publishes the provider's `lpp/check` answer
over the complete open-buffer overlay — the unsaved text under the client's
own URI — with the provider's UTF-16 ranges carried through (#738). DEL/OSTW
documents and OPY documents without a working provider publish an explicit
`source-provider-unavailable` or provider-resolution error code, and a
failed provider request publishes its structured error code on the
document — a provider refusal carries the same `provider-refusal-<code>`
classification the driver surfaces report (#1004). A document that is
neither publishes no diagnostics.

`textDocument/rename` on a source-language document routes through the
provider-owned mutation path (#156): the
provider computes edits over the open document set (only the provider can
compute project membership, so the set stays language-wide), Wright verifies
document versions and source preconditions, asks the provider to validate
the transaction, and rechecks the edited project, with the check verdict
scoped to the documents the provider actually edited plus the position
document so an unrelated open document cannot block a valid rename, before
returning anything. A successful rename answers with versioned
`WorkspaceEdit.documentChanges`: each `TextDocumentEdit` carries the
document version the provider computed and Wright re-validated, so the
client can reject the edit when its buffer has moved. Clients that did not
negotiate `workspace.workspaceEdit.documentChanges` get no `renameProvider`
advertisement, and their rename requests are refused
(`rename-unversioned-workspace-edit`) rather than served an unversioned
`changes` map a stale buffer could silently absorb. Unsupported documents
(`rename-unsupported-document`), unopen documents
(`rename-unknown-document`), unconfigured providers, and provider or
validation refusals answer with a `RequestFailed` error whose
`error.data.code` carries the structured refusal code. There is no textual
search/replace fallback. `--opy-provider <PATH>` points the session at an
explicit OPY provider executable; by default the resolver locates or
downloads the released provider like the CLI does.

For source-language documents, remaining editor capabilities — hover,
completion, and semantic tokens — are future work tracked under the owning
implementations. They arrive through provider capability negotiation, not
through Wright-side reimplementation or static fallbacks; a source-language
document answers `null` to hover, and `.del`/`.ostw` documents answer
`null` to every navigation request.

## OPY provider-backed services (#738)

`.opy` documents are served by the negotiated OPY provider, never by an
in-process OPY implementation. The provider session resolves at
`initialize` through the same `resolve()` path the driver uses — an
explicit `--opy-provider <PATH>` executable, the installed provider
store, or a bootstrap download of the released provider — and
`lpp/initialize` runs inside the LSP handshake so the advertised surface
is truthful. The session is retained and shared by every later request; an
unresolvable or failed provider latches as unavailable and the LSP session
continues without OPY backing.

Every provider request sends the open-document overlay: the text the editor
holds, at the buffer's version, under the URI the client opened — including
`.opy` files that exist only as unsaved buffers. `#!include` membership and
any disk reads behind it are the provider's own project semantics, not
Wright's.

- diagnostics are `lpp/check` over the overlay; the provider's UTF-16
  ranges refer to the exact text sent, so they publish unchanged and
  authored positions survive;
- `textDocument/documentSymbol` is `lpp/symbols`, advertised only when
  negotiated;
- `textDocument/definition` on `.opy` is `lpp/definition`, and
  `textDocument/references` is `lpp/references`; both return provider
  locations verbatim, including locations inside `#!include`d files;
- a capability the provider did not negotiate — or a provider refusal
  such as `definition.noSymbolAtPosition` — answers `null` (or an empty
  list), never an LSP error.

## Raw Workshop language services (#555)

ADR-0023 scoped raw Workshop to diagnostics, hover, definition, and
references for Wright 1.0. The service recognizes a raw Workshop document
by the editor's `workshop` language id, or, when untagged, by a Workshop
extension `wright check` accepts (`.txt`, `.ow`, `.ws`, `.workshop`); a
source-language extension or an explicit non-Workshop language id stays
out of this path.

Each query parses the current buffer text through the same driver session
pipeline the CLI runs (`InputSpec::Text` carries the buffer under the
document's file identity, so an unsaved file still resolves), and answers
from the canonical `SemanticIndex` `wright inspect` serves:

- diagnostics are `check`'s diagnostics verbatim: code, severity, and
  span, at the buffer's document version;
- hover names the resolved symbol as `` `kind name` `` over the identifier
  occurrence that matched;
- definition targets the symbol's declared span;
- references return every occurrence span `inspect` reports for the
  symbol, and `includeDeclaration: false` drops the declaration;
- a position outside any identifier occurrence, a position covered by
  more than one symbol, a buffer that fails to load, and any non-Workshop
  document answer `null`.

No LSP-specific resolution layer exists: spans, symbols, and reference
kinds are the analyzer's canonical data, converted to UTF-16 ranges at the
protocol boundary.

## Editor-neutral contracts

* `Document`: URI identity, current text, monotonic internal `version`,
  project root (include base), and the editor-declared `language_id` when
  the host reports one.
* `DocumentStore`: open/change/close lifecycle with version bumping.
* Positions and ranges are 0-based editor conventions; the service converts to
  the compiler's 1-based spans at the boundary. UTF-16/character conversion
  is centralized in `wright_language::document` (`utf16_offset_to_char`,
  `char_offset_to_utf16`, `span_to_range`) and is the
  only path that consumes or emits editor positions; no UTF-16 offset is ever
  used as a byte index.
* File URI/filesystem path conversion is centralized in
  `wright_language::document` (`uri_to_path`, `path_to_uri`) using the
  standard URL parser, covering percent-encoding, spaces, Unicode filenames,
  and platform drive paths.
* Every result carries the document version it was validated against
  (`document_version`, `SourceTextEdit.version`); stale results are
  detectable and replaceable.

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

The service produces source-aware `SourceDiagnostic`s. Raw Workshop
documents publish the session `check` diagnostics for the buffer text
(#555); `.opy` documents publish the provider's `lpp/check` answer over the
open-buffer overlay (#738). `.del`/`.ostw` documents — and `.opy` documents
when no provider can be negotiated — publish the explicit
`source-provider-unavailable` refusal. It is never a static fallback
result.

## DEL/OSTW documents (#120)

`.ostw`/`.del` documents remain recognized entrypoints for a future source
provider, but Wright no longer carries a static DEL/OSTW implementation. The
service reports `source-provider-unavailable` as a structured source error
for these documents. It never invokes an upstream compiler or a removed
Wright implementation as a fallback.

Workshop-to-OPY reconstruction is available only when the provider negotiates
`reconstruct`; Workshop-to-OSTW is refused at the provider boundary until a
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
scenarios, OPY hover/completion/semantic tokens, and DEL/OSTW editor
features until a provider capability exists remain future work.
