# Wright Language Services and LSP

Status: accepted baseline (living language services and LSP contract)
Scope: editor-neutral language services (`wright-language`) and the thin LSP
adapter (`wright-lsp`)

## Architecture

```text
document/workspace model (Document, DocumentStore)
   → LanguageService (editor-neutral, no LSP types)
       ├─ diagnostics (provider capability or explicit refusal)
       ├─ hover / definition / references (provider capability)
       ├─ completion (provider capability)
       ├─ rename (provider capability)
       └─ semantic tokens (provider capability)
            ↓
  wright-lsp (thin protocol adapter, Content-Length stdio framing)
```

All semantic logic lives in `wright-language`; `wright-lsp` only maps LSP
DTOs, so adding or removing an editor protocol never changes the compiler or
analyzer contracts.

## Editor-neutral contracts

* `Document`: URI identity, current text, monotonic internal `version`,
  project root (include base).
* `DocumentStore`: open/change/close lifecycle with version bumping.
* Positions and ranges are 0-based editor conventions; the service converts to
  the compiler's 1-based spans at the boundary. UTF-16 ↔ character conversion
  is centralized in `wright_language::document` (`utf16_offset_to_char`,
  `char_offset_to_utf16`, `span_to_range`, `full_document_range`) and is the
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

- the committed perf harness mean for `analyze` or `diagnostics` exceeds the
  harness regression bound (200 ms per workflow); or
- peak RSS on the perf workload exceeds 1 GB; or
- a representative project-scale measurement (a multi-file corpus of at least
  20 source files, or the declared representative project) shows any single
  interactive request exceeding 100 ms.

Until a trigger fires, bounded full recomputation with stale-result
suppression is the authoritative contract.

## Services

* **Diagnostics**: source-aware `SourceDiagnostic`s, including explicit
  `source-provider-unavailable` when no provider editor capability is
  negotiated.
* **Hover / Definition / References / Completion**: provider-owned; Wright
  does not duplicate OPY parsing, manifests, or semantic indexes.
* **Rename**: project-wide identifier-exact rename: resolves the symbol
  through the semantic index, unions its exact declaration/definition/reference
  identifier spans across every open root whose project includes the
  requesting document, and returns source-aware full-document edits for all
  affected sources (open overlays take precedence over filesystem content).
  Collisions, unresolvable identity, a missing exact identifier span, stale
  source identity, and failed validation refuse explicitly. Edits are
  Wright-owned (`RenameEdit`/`TargetSpan` in `wright-language`) and carry the
  SHA-256 source identity computed through `wright_driver::input_identity`.
  Rename delegates target resolution, edit generation, and validation to the
  shared driver refactoring contract
  (`wright_driver::edit::semantic_rename`, #129): every affected root
  resolves through its original owner-backed source implementation, the unioned exact-range
  transaction is validated through the shared #128 transaction boundary
  (`wright_driver::edit::validate_transaction`), and no duplicate
  edit-validation or span-collection semantics live here.
* **Semantic tokens**: provider-owned; no static lexer fallback is shipped.

## DEL/OSTW documents (#120)

`.ostw`/`.del` documents remain recognized entrypoints for a future source
provider, but Wright no longer carries a static DEL/OSTW implementation. The
service reports `source-provider-unavailable` as a structured source error and
does not produce semantic tokens, navigation, or rename edits for these
documents. It never invokes an upstream compiler or a removed Wright
implementation as a fallback.

Workshop → OPY reconstruction is available only when the provider negotiates
`reconstruct`; Workshop → OSTW is refused at the provider boundary until a
provider is configured.

## LSP adapter

`wright-lsp` (stdio, Content-Length framing) implements: initialize
(capability negotiation: hover/definition/references/completion/rename/full
semantic tokens), didOpen/didChange/didSave/didClose (didSave is an explicit
no-op for full-sync documents; didClose retires diagnostics), publishDiagnostics
(versioned and grouped by source identity, with didClose cleanup), per-root
publication ownership: a source that disappears from a root analysis is
retired with an empty publishDiagnostics unless another open root still owns
it, so no diagnostic stays stale solely because its source left the analysis,
dependency-refresh of affected documents on include/overlay changes, hover,
definition, references, completion, rename (multi-document workspace edit),
semanticTokens/full, shutdown/exit.

**Rename is a pure adapter (#131)**: the LSP layer maps the shared #129
transaction (exact-occurrence editor edits from `wright-language`) directly
to `WorkspaceEdit` `documentChanges`/`TextDocumentEdit`, creating one `TextEdit` per
semantic occurrence, grouped by document, with each open document identified
at its current version and filesystem-backed sources in the unversioned
`null` form. No symbol resolution, collision, or stale-state logic exists in
the protocol layer; unsupported rename targets surface the shared refusal as
an explicit LSP error, never a textual fallback. The end-to-end
  harness (`wright-lsp/tests/lsp.rs`) drives the real binary and verifies
  capability negotiation, lifecycle, refusal routing, and stale-version
  suppression.

## Out of scope (recorded)

VS Code/browser extensions, incremental diff-based reanalysis (full
recomputation is deterministic and fast enough), and client-side behavior
scenarios remain future work.
