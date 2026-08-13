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
measures the heaviest corpus fixture: analyze ≈2.3 ms, diagnostics ≈1.3 ms,
hover ≈1.0 ms, peak RSS ≈6 MB (`target/language-service-perf.json`). Because
each workflow is bounded far below interactive latency, **true cancellation
is explicitly deferred**; the synchronous-recomputation path cannot be
interrupted mid-flight, but it also cannot surface obsolete results as
current: results are version-tagged, stale/out-of-order client versions are
rejected, and every query re-reads the current document state. This decision
is the measured option B of the M10 contract, not an unexamined shortcut.

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
