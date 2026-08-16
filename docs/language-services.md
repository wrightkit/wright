# Wright Language Services and LSP

Status: accepted baseline — living language services and LSP contract
Scope: editor-neutral language services (`wright-language`) and the thin LSP
adapter (`wright-lsp`)

## Architecture

```text
document/workspace model (Document, DocumentStore)
   → LanguageService (editor-neutral, no LSP types)
       ├─ diagnostics (parse errors + semantic findings)
       ├─ hover / definition / references
       ├─ completion (symbols + builtins + keywords)
       ├─ rename (identifier-exact, pipeline-validated)
       └─ semantic tokens (native lexer classification)
            ↓
  wright-lsp (thin protocol adapter, Content-Length stdio framing)
```

All semantic logic lives in `wright-language`; `wright-lsp` only maps LSP
DTOs, so adding or removing an editor protocol never changes the compiler or
analyzer contracts.

## Editor-neutral contracts

* `Document` — URI identity, current text, monotonic internal `version`,
  project root (include base).
* `DocumentStore` — open/change/close lifecycle with version bumping.
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
The committed language-service perf harness (`wright-language/tests/perf.rs`)
measures the heaviest corpus fixture: analyze ≈1.0 ms, diagnostics ≈0.9 ms,
hover ≈0.9 ms, peak RSS ≈7 MB (`target/language-service-perf.json`). Each
workflow is bounded far below interactive latency.

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

* **Diagnostics** — source-aware `SourceDiagnostic`s for parse errors and
  analyzer findings, carrying source identity, source-local range, severity,
  code, message, and source/requesting-document versions; included-file spans
  are resolved against their own source text, not the requesting document.
* **Hover** — symbol name/kind and usage summary (reads/writes/calls/rules).
* **Definition / References** — via the semantic index over source spans.
* **Completion** — declared symbols, manifest-declared builtins and receiver
  members, keywords (the OPY semantic manifest is the authoritative builtin
  surface, #109).
* **Rename** — project-wide identifier-exact rename: resolves the symbol
  through the semantic index, unions its exact declaration/definition/reference
  identifier spans across every open root whose project includes the
  requesting document, and returns source-aware full-document edits for all
  affected sources (open overlays take precedence over filesystem content).
  Collisions, unresolvable identity, a missing exact identifier span, stale
  source identity, and failed validation refuse explicitly. Edits are
  Wright-owned (`RenameEdit`/`TargetSpan` in `wright-language`) and carry the
  SHA-256 source identity computed through `wright_driver::input_identity`.
  Rename delegates target resolution, edit generation, and validation to the
  shared M14 driver refactoring contract
  (`wright_driver::edit::semantic_rename`, #129): every affected root
  resolves through its original native frontend, the unioned exact-range
  transaction is validated through the shared #128 transaction boundary
  (`wright_driver::edit::validate_transaction`), and no duplicate
  edit-validation or span-collection semantics live here.
* **Semantic tokens** — classified by the native lexer/parser identity
  (keywords, variables, identifiers, strings, numbers, operators, macros,
  attributes), not textual heuristics.

## OSTW documents (#120)

`.ostw`/`.del` documents route through the same editor-neutral services with
no OSTW-specific analysis stack: the native OSTW frontend loads the `ds.toml`
project closure and resolves the #118 semantic HIR, which is lowered through
the shared HIR→WIR path; diagnostics, findings, hover, definition,
references, completion, and semantic tokens then come from the shared
analyzer/semantic index exactly as for OPY/Workshop inputs. Project-level and
#118 semantic boundary diagnostics (missing imports, Math/Cursor/class
surfaces) surface as source-aware errors with project-relative paths.

Operations that stay unsupported for OSTW are **explicitly refused or
documented, never emulated through upstream calls**:

- **Semantic rename** — offered for OSTW symbols on the declared semantic
  surface (globals, player variables, subroutines/functions) through the
  shared M14 refactoring contract (#129): resolution runs over the shared
  semantic index of the `ds.toml` project graph, edited transactions validate
  through the native OSTW frontend, and targets without an exact identifier
  span (e.g. typed constants and other provenance-limited forms) refuse
  explicitly instead of broadening to a statement span. Whole-source OSTW
  regeneration/emitters remain a declared non-goal (#120) — rename edits
  original OSTW source with exact identifier ranges, never reconstructed
  text.
- **Whole-source pretty-printing / comment-preserving regeneration** — not
  implemented; diagnostics and navigation are source-preserving.
- Upstream OSTW LSP parity, classes/generics/lambdas/pattern matching beyond
  the accepted corpus boundary, and whole-source regeneration remain out of
  scope for the language service (the corpus boundary defines what resolves).
  Workshop → OPY/OSTW semantic reconstruction is available through the CLI
  and driver conversion command (`wright convert --target opy|ostw`, #126),
  not through the language-service surface.

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
to `WorkspaceEdit` `documentChanges`/`TextDocumentEdit` — one `TextEdit` per
semantic occurrence, grouped by document, with each open document identified
at its current version and filesystem-backed sources in the unversioned
`null` form. No symbol resolution, collision, or stale-state logic exists in
the protocol layer; unsupported rename targets surface the shared refusal as
an explicit LSP error, never a textual fallback. The end-to-end
harness (`wright-lsp/tests/lsp.rs`) drives the real binary and verifies
capability negotiation, lifecycle, navigation, completion, rename (OPY
multi-document and supported OSTW rename, UTF-16/non-BMP ranges, version
preconditions), semantic tokens, and stale-version suppression.

## Out of scope (recorded)

VS Code/browser extensions, incremental diff-based reanalysis (full
recomputation is deterministic and fast enough), and client-side behavior
scenarios remain future work.
