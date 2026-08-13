# Wright Language Services and LSP (M10)

Status: contract baseline for milestone M10 (issue #27)
Scope: editor-neutral language services (`wright-language`) and the thin LSP
adapter (`wright-lsp`)

## Architecture

```text
document/workspace model (Document, DocumentStore)
   → LanguageService (editor-neutral, no LSP types)
       ├─ diagnostics (parse errors + semantic findings)
       ├─ hover / definition / references
       ├─ completion (symbols + builtins + keywords)
       ├─ rename (M9 safe-edit contract, pipeline-validated)
       └─ semantic tokens (native lexer classification)
            ↓
  wright-lsp (thin protocol adapter, Content-Length stdio framing)
```

All semantic logic lives in `wright-language`; `wright-lsp` only maps LSP
DTOs, so adding/removing an editor protocol never changes the compiler or
analyzer contracts.

## Editor-neutral contracts (#63)

* `Document` — URI identity, current text, monotonic internal `version`,
  project root (include base).
* `DocumentStore` — open/change/close lifecycle with version bumping.
* Positions/ranges are 0-based editor conventions; the service converts to
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
  replaceable (#64).

## Incremental behavior (#64)

Reanalysis is a deterministic full recomputation over the changed document.
The committed language-service perf harness (`wright-language/tests/perf.rs`)
measures the heaviest corpus fixture: analyze ≈1.0 ms, diagnostics ≈0.9 ms,
hover ≈0.9 ms, peak RSS ≈7 MB (`target/language-service-perf.json`,
re-measured after #72–#74). Each workflow is bounded far below interactive
latency.

## Responsiveness contract (#75)

The M10 responsiveness requirement is **stale/current-state correctness, not
in-flight request cancellation**. Synchronous deterministic full recomputation
is the implemented contract: results are version-tagged, stale/out-of-order
client versions are rejected and cannot overwrite newer state, obsolete
results are never surfaced as current, and every query re-reads the current
document state. True in-flight request cancellation is **explicitly deferred**
and is not an M10 requirement; it must not be re-introduced speculatively.

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
suppression is the authoritative M10 contract.

## Services (#65/#66)

* **Diagnostics** — source-aware `SourceDiagnostic`s for parse errors and
  analyzer findings, carrying source identity, source-local range, severity,
  code, message, and source/requesting-document versions; included-file spans
  are resolved against their own source text, not the requesting document.
* **Hover** — symbol name/kind and usage summary (reads/writes/calls/rules).
* **Definition / References** — via the M4 semantic index over source spans.
* **Completion** — declared symbols, corpus-evidenced builtins, keywords.
* **Rename** — project-wide semantic rename: resolves the symbol through the
  semantic index, unions declaration/definition/reference targets across every
  open root whose project includes the requesting document, and returns
  source-aware edits for all affected sources (open overlays take precedence
  over filesystem content). Collisions, unresolvable identity, and failed
  validation refuse explicitly. Reuses `wright_driver::edit`
  (`rename_occurrences`/`SourceEdit`) as the shared edit contract.
* **Semantic tokens** — classified by the native lexer/parser identity
  (keywords, variables, identifiers, strings, numbers, operators, macros,
  attributes), not textual heuristics.

## LSP adapter (#67/#68)

`wright-lsp` (stdio, Content-Length framing) implements: initialize
(capability negotiation: hover/definition/references/completion/rename/full
semantic tokens), didOpen/didChange/didSave/didClose (didSave is an explicit
no-op for full-sync documents; didClose retires diagnostics), publishDiagnostics
(versioned and grouped by source identity, with didClose cleanup),
dependency-refresh of affected documents on include/overlay changes, hover,
definition, references, completion, rename (multi-document workspace edit),
semanticTokens/full, shutdown/exit. The end-to-end
harness (`wright-lsp/tests/lsp.rs`) drives the real binary and verifies
capability negotiation, lifecycle, navigation, completion, rename, semantic
tokens, and stale-version suppression.

## Out of scope (recorded)

VS Code/browser extensions, incremental diff-based reanalysis (full
recomputation is deterministic and fast enough), and client-side behavior
scenarios remain future work.
