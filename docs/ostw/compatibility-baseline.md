# OSTW Compatibility Baseline and OSTW Reference Investigation

Status: accepted baseline (OSTW reference/corpus/support investigation #113:
baseline pinned #115, native syntax/project frontend foundation #117,
protect-ban HIR lowering #118, explicit compile-root oracle rebaseline #122,
first declared OSTW → Workshop compile surface #119, tooling/language-service
integration #120, Workshop → OSTW reconstruction #125, and the shared
driver/session conversion integration #126)
Status note: Native AST/parser, project settings (`ds.toml`), import-closure
resolution, and protect-ban HIR lowering are implemented in `crates/wright-ostw`.
The first declared OSTW → Workshop compile surface is implemented (#119) and
validated against the pinned explicit-root evidence by the CI-protected
differential suite; the declared surface, normalization contract, and known
divergences live in [`support-matrix.md`](support-matrix.md). Since #122,
every recorded oracle observation names an explicit compile/document root;
the historical 79-element protect-ban observation is reclassified as the
`utils/ServerLoad.del` document-root compile, not entry-project acceptance.
The reverse direction — Workshop → OSTW reconstruction — is implemented by
#125 (`wright_ostw::reconstruct`) and integrated end-to-end by #126 behind
the same shared driver/session conversion contract as the #124 Workshop → OPY
reconstruction (see [`../cli.md`](../cli.md)): the Workshop → OSTW
reconstruction phase is complete, and the tooling/language-service phase was
completed by #120. Direct OPY ↔ OSTW conversion remains
explicitly deferred pending the PM's roadmap reassessment (recorded in the
"OSTW reverse-interoperability completion" section below).
Scope: forward-looking, tiered inventory of the OSTW language surface against
the pinned reference, the corpus/acquisition plan, the oracle feasibility
report, and the reuse/boundary findings that the OSTW work respects.

This document is the OSTW counterpart of
[`docs/opy/compatibility-baseline.md`](../opy/compatibility-baseline.md): the
future `support-matrix.md` records the corpus-evidenced surface Wright supports,
while this baseline records how the surface is tiered and sequenced. Facts
marked "verified" were checked against the upstream repository, wiki, and
release artifacts on 2026-08-15; see
[`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md)
for the durable reference identity.

## Upstream architecture (verified against source)

The upstream compiler (`Deltinteger`, C# / .NET 8.0) is a single executable
containing the compiler, a decompiler, an emulator, and a full stdio language
server:

| Component | Location in `Deltinteger/` | Relevance to Wright |
| --- | --- | --- |
| Frontend: hand-written recursive-descent parser, custom lexer with incremental lex/parse, operator-stack expression parsing (C-style + vanilla operators), CST | `Compiler/Parse/` (`Parser.cs`, `Lexer/`), `Compiler/Syntax Tree/` | Defines the grammar surface Wright's OSTW frontend must match for S-level claims |
| Semantic/type layer: scopes (global/player/rule), `ScriptTypes` type provider, classes (heap, ≤999 instances, inheritance, virtual/override, constructors), structs (value types, `in`/`ref`), enums + pattern matching, generics, lambdas/first-class functions, variables (chasing, extended collection), subroutines, macros, `import`/JSON import, Vanilla Workshop superset with variable/subroutine linking, `ds.toml` project settings | `Parse/` (`Translate.cs` driver, `Types/`, `Variables/`, `Functions/`, `Loops.cs`, `Switch.cs`, `Lambda/`, `PatternMatching/`, `Import/`, `Vanilla/`, `Settings/`) | The semantic surface: categories 2–10 below |
| Rule/event model: 11 events (table in the wiki "Rules" page), rule priority ordering, synthesized `Initial Global`/`Initial Player` rules | `Parse/RuleAction.cs`, `Parse/TranslateRule.cs` | Shared with Wright's HIR rule model |
| Emission: Workshop elements (`IWorkshopTree`), rule/variable/subroutine serialization, 13 locales, old vs `c_style_workshop_output` syntax, optimizer (per-function constant folding, default on), element counts, optional comments | `Elements/` (`WorkshopConverter/`, `Optimize.cs`, `OutputLanguage.cs`, `Variables.cs`) | N-level comparisons; optimizer parity is explicitly not a goal |
| Decompiler: Workshop text → element tree (`TextToElement`) → OSTW code (`ElementToCode`, function mapping table) | `Decompiler/` | Workshop → OSTW reconstruction reference |
| Emulator: ticks Workshop rules with players/variables/arrays; upstream's own behavioral oracle in `Deltinteger.Tests` (`EmulateTick`, `AtomizeAndReconstruct`) | `Emulator/` | Candidate E-level reference within its documented subset |
| Language server: full LSP stdio server (completion, signature, hover, definition, references, rename, code lens, color, document symbols, semantic tokens, configuration; custom `workshopCode`/`elementCount`/`serverError` notifications; custom `decompile.insert`/`decompile.file` requests) | `Language Server/` | The headless oracle execution path (see oracle report) |
| WASM interop: `OstwJavascript` exports (`AddModelAsync`, `UpdateModelAsync`, `SetCompiledWorkshopCode`, …) | `Web/Javascript.cs` | Secondary oracle path; built only via `dotnet publish -r browser-wasm` (not shipped in release assets) |
| CLI: `--ping`, `--langserver`, `--schema`, `--editor`, `--decompile-clipboard <file>`, default compile (interactive, clipboard-bound) | `Program.cs` | Default compile is not headless-friendly; `--langserver` is |

## Tier taxonomy

Identical to the OPY baseline:

| Tier | Meaning |
| --- | --- |
| `baseline-supported` | Implemented and corpus/reference-evidenced; part of the declared supported surface |
| `baseline-planned` | Stable, high-fan-out, systematically implementable; contract is discoverable and reference-testable; not yet implemented |
| `evidence-prioritized` | Complex or broad feature with clear tooling value; corpus/consumer evidence determines ordering |
| `legacy-quirk/demand-driven` | Rare historical quirks, upstream bugs, obsolete aliases; implemented only when the declared compatibility target requires them |
| `reference-limited/inconclusive` | Cannot be resolved from the proposed reference; needs a demonstrated need, a pin change, or further investigation |

## Support dimensions

For each category: **Parse** (accepted by the future native grammar),
**Semantic resolution** (resolved to meaningful HIR), **Compilation**
(standalone compile/emission through the Workshop backend with
reference-equivalent semantics), **Tooling/analysis** (`check`/`analyze`/
`lint`/`inspect` and language services), **Reference coverage** (oracle
probes/fixtures validate the behavior). The frontend (categories 1–7, #118)
and the tooling dimension (category 13, #120) are implemented for the
protect-ban entry-point reachable graph; the remaining dimensions stay
planned or evidence-prioritized as marked per row.

## Category inventory

| # | Category | Tier | Parse | Sem | Comp | Tooling | Ref |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | **Lexing/grammar core**: C#-style tokens, `//`/`/* */` comments, braces, literals (Number/String/Boolean/Vector/Any), operators incl. `??`/`?.`/`++`/`--`/compound assignment, `.del`/`.ostw`/`.workshop` inputs | `baseline-planned` (core subset) | ✅ planned | ✅ | ✅ | ✅ | ✅ probes |
| 1a | Vanilla Workshop superset inside OSTW files (`variables { … }`, `rule("…") { event/actions }`, variable/subroutine linking) | `evidence-prioritized` (needed for Workshop → OSTW round-trip and mixed files) | - | - | - | - | ✅ upstream parses it |
| 2 | **Rules**: `rule: "name"` (+ optional sort order), 11 events, rule-level `if` conditions, rule/event context (global vs player), synthesized initial rules | `baseline-planned` (core) | ✅ | ✅ | ✅ | ✅ | ✅ |
| 3 | **Variables & scopes**: `globalvar`/`playervar`/`define`/`static`, typed, rule-level and block scope, chasing/modifying, player-variable receiver semantics, explicit IDs, extended collection | `baseline-planned` (core subset) | ✅ | ✅ | ✅ | ✅ | ✅ |
| 4 | **Control flow**: if/else if/else, `for`/`foreach`/`while`, `continue`/`break`, `switch` (fallthrough), ternary, `Root` keyword | `baseline-planned` (core subset) | ✅ | ✅ | ✅ | ✅ | ✅ |
| 5 | **Values & workshop-function calls**: `SmallMessage`, `Kill`, `Wait`, `HostPlayer`, … resolved through Wright's Workshop catalog (not OSTW's game-derived data); receiver/member access (`EventPlayer().x`, `AllPlayers().isBoss = …`), string concat/format | `baseline-planned` (generic surface; catalog-bound like OPY category 6/7) | ✅ | ✅ | ✅ | ✅ | ✅ |
| 6 | **Types**: Number/String/Boolean/Player/Vector/Any, arrays (literal/index/append/remove/Length), structs (value semantics, `in`/`ref`), enums (basic members), casts (`<T>expr`) | `baseline-planned` (core); class/generic/lambda surface below | ✅ | ✅ | ✅ | ✅ | ✅ |
| 7 | **Functions/macros/subroutines**: user functions with params/return, `in`/`ref` params, macros (compile-time, no actions), subroutines (incl. linking), bounded recursion, `recursive` keyword | `baseline-planned` (core) | ✅ | ✅ | ✅ | ✅ | ✅ |
| 8 | **Classes**: heap allocation (≤999 instances), inheritance, virtual/override, constructors, `delete`, pointer/reference semantics, reference validation (inline/subroutine), class generations | `evidence-prioritized` | - | - | - | - | ✅ upstream tests |
| 9 | **Generics / lambdas / pattern matching**: generic classes/functions, `Func`/function types, expanded enum pattern matching | `evidence-prioritized` | - | - | - | - | ✅ upstream tests |
| 10 | **Project settings (`ds.toml`)**: `entry_point`, `out_file`, `optimize_output`, `c_style_workshop_output`, variable/subroutine prefixes, `reset_nonpersistent`/`__loadPersist`, validation toggles | `baseline-planned` (the settings subset the corpus uses) | ✅ | ✅ | - | ✅ | ✅ probes |
| 11 | **OSTW → Workshop emission**: en-US, both output syntaxes, default-argument filling, element counts | `baseline-planned` | - | - | ✅ | - | ✅ normalized |
| 12 | **Workshop → OSTW reconstruction**: rules/actions/conditions/values via Wright's Workshop parser → WIR → a Wright-owned OSTW emitter; `import "settings.json"` handling | `baseline-supported` (declared surface #125, shared driver/session conversion path #126) | - | - | ✅ | - | ✅ reference decompiler |
| 13 | **Tooling/language services**: OSTW documents in `check`/`lint`/`analyze`/`inspect`, editor-neutral language services, LSP mapping, source provenance and structured diagnostics through `wright-result/v1` | `baseline-planned` | - | - | - | ✅ | - |
| 14 | **Emission extras**: multi-locale output (12 non-en-US), optimizer-equivalent transforms, `use_tabs_in_workshop_output`, `compile_miscellaneous_comments` | `legacy-quirk/demand-driven` (en-US is the corpus default) | - | - | - | - | partial |
| 15 | **Specialized subsystems**: pathfinding (`.pathmap`/`.csv`), asset/model import, JSON import, debugger protocol, lobby-settings authoring schema, save/load (`reset_nonpersistent`) | `evidence-prioritized` (lobby settings if corpus needs it) / `reference-limited` (debugger requires a live game) | - | - | - | - | partial |
| 16 | **Reference quirks**: interactive/clipboard-bound default CLI, `xclip` dependency headless, `gitHead`-style tag drift (`Program.VERSION` lags master), rolling `latest` tag, emulator `Wait` unimplemented, optimizer output identity | `reference-limited/inconclusive` | - | - | - | - | documented |

## Corpus inventory and acquisition plan

Policy: the corpus follows `compatibility/README.md`: fixtures from a project
without an explicit redistribution license stay out until resolved. Acquisition
reuses `scripts/acquire-corpus.py` and `scripts/corpus-manifest.json` (immutable
commits, per-file SHA-256, license fields, full include closures).

| Candidate | Provenance | Verdict |
| --- | --- | --- |
| `ItsDeltin/Lava`: official OSTW example collection (minigames, maze, dodgeball, …), ≈25 `.del` files | No license file (GitHub API `license: null`), last pushed 2021 | **Excluded until licensing resolves**; can be a local reference read for behavior |
| `pharingWell/MOBAwatch`: "A MOBA made in Overwatch", 69 `.ostw`/`.del` files (35 `.ostw` + 34 `.del`) across `Header Files/`/`Source Files/`, `c_style_workshop_output = true`, `ds.toml` present | BSD-2-Clause, last pushed 2025-02-21, `release` default branch | **Primary corpus candidate** (large, active-format, BSD-redistributable) |
| `GrandeurHammers/protect-ban`: `main.ostw` + `interface/`/`utils/` `.del` modules, `ds.toml` present | MIT, last pushed 2025-07-19 | **Secondary corpus candidate** (small, clean project layout); entry closure reaches three missing `../OSTWUtils/…` imports |
| `GrandeurHammers/OSTWUtils`: the missing protect-ban dependency family (`OnScreenText`, `Cursor`, `StringSorting`) | No license file, public repository | **Excluded until licensing resolves**; source must not be copied into Wright |
| Upstream `Deltinteger.Tests` sources | Part of the unlicensed compiler repo | Not redistributable; behavior may inform the baseline as observed evidence only |
| OSTW wiki examples | Wiki repo, no license | Not redistributable wholesale; short snippets under fair-use review before any commit |

Initial acquisition plan (PM approval pending): pin `MOBAwatch` and
`protect-ban` at their current default-branch heads, record full file manifests
with per-file SHA-256, and mark every fixture with the exact reference identity
(`v3.4.0` proposed). A fixture's `expectedStatus`/`features` will name the
categories in the inventory above that its sources exercise, so the first
support boundary is corpus-defined rather than speculative.

## Measured OSTW reference baseline

Evidence was generated by `compatibility/ostw/run_oracle.py` against the
immutable `v3.4.0` tag commit `769ce7aab097178cfe905bf21f0326d8e0d12e6b`
and the pinned `v3.4.0-linux-x64.zip` SHA-256
`1ae882898961eac8ac25234a18fa3b130a02836651f7f936b9ece85f181e3a88`.
The reference is invoked only through `Deltinteger --langserver`; `--ping`
returned `Hello!`. The binary remains an external `target/`/CI artifact, not a
Rust dependency or committed fixture.

| Project | Immutable revision | Licensed files | Entry-root oracle result | Document-root observations |
| --- | --- | ---: | --- | --- |
| MOBAwatch | `b9b1ac3b77a484256e89aca6be8c27470803f665` | 70 source/project/license files | **rejected** (`Source Files/main.ostw`): generic `==` on type `T` plus missing include-closure assets | per-document table in `results.json`; the historical 79-element-era observation was the `tests.ostw` root: reject, error log SHA-256 `b0a8d959a1280c4513a15d0ed0ea64e62d192b0dff628f4924514d3490389bc9` |
| protect-ban | `f8c2353ed8447f13038fbf6b9938031cced5796f` | 19 source/project/license files | **rejected** (`main.ostw`): three hard missing `../OSTWUtils/…` imports; no Workshop output | per-document table in `results.json`; the historical 79-element observation was the `utils/ServerLoad.del` root: accept, `elementCount` 79, SHA-256 `75722f0aa7ed0484bf8ca5503bd93d2798c7bbb2fd59b5729b122ee1d8a03912` |

`compatibility/ostw/corpus.json` is the machine-readable provenance and
integrity inventory: repository, license, revision, `ds.toml` entry point,
source kind, semantic categories, SHA-256 for every committed source, and the
explicit reviewable `roots` each project is observed under. `results.json`
(schema v2) records one observation per root — accept/reject, `elementCount`,
the full diagnostics with document URIs, the import-closure identity, and the
missing-import boundaries — as generated by the explicit-root corpus runner.

The corpus exercises project settings, imports, rules, macros, arrays,
Workshop calls and classes. These categories define the first Wright-owned
support-boundary evidence set; classes remain evidence-prioritized rather than
an automatic first implementation requirement. Measured observations:

* **MOBAwatch entry root rejects under the pinned Linux reference.** The errors
  are the generic `==` on type `T`, Windows-authored backslash include paths
  (`Header Files/projectiles/..\entity.del`, `..\structures.del`) that Linux
  OSTW cannot resolve, and missing closure assets (`customGameSettings.json`).
  The committed closure is therefore not a clean Linux compile unit; the
  categories it exercises still define the support-boundary evidence, but no
  N-level claim can be made from it as acquired.
* **protect-ban per-document evidence is mixed; the project entry root
  rejects.** `main.ostw` (the `ds.toml` entry point) reaches three hard
  missing `../OSTWUtils/…` imports and produces no Workshop output. Individual
  documents that do not touch the missing imports compile (for example
  `utils/ServerLoad.del`, 79 elements; `Credits.ostw`, 49 elements), while
  every document that reaches the missing imports rejects. These are
  per-document observations, not project acceptance: the reference LSP compiles
  the last-opened document plus its import closure, and `ds.toml.entry_point`
  is not the LSP compile selector (pinned P1 evidence).
* The earlier "empty `workshopCode`" blocker was a harness artifact (it
  captured the first transient compile notification, `"\n"`, before the real
  compile) and is resolved by the deterministic last-compile capture.
* Neither result supports an E-level claim, optimizer parity, multi-locale
  behavior, or a frontend scope.

## Corrected evidence model: explicit compile roots (#122)

Pinned P1 evidence established that the upstream LSP compiles the **last-opened
document plus its transitive import closure**, not the project's
`ds.toml.entry_point`. The corpus runner therefore changed to schema v2
(`results.json`): every observation is produced by a session that opens exactly
one document — the observation's explicit `root` — so the recorded compile can
only be that root's compile and can never acquire meaning from `didOpen`
ordering. Each observation also records its computed import-closure identity
and missing-import boundaries (relative-path resolution per pinned P2
evidence), so project-level support, per-document evidence, and
reference-rejected boundaries are distinguishable.

* **Reclassification.** The historical `accept, elementCount 79,
  SHA-256 75722f…` protect-ban observation was not entry-project acceptance: it
  is the `utils/ServerLoad.del` document-root compile and is recorded as
  `historical-document-root` in `results.json` (re-run under the pinned v3.4.0
  reference reproduces the same 79 elements and hash). The historical MOBAwatch
  observation is likewise attributed to its `tests.ostw` root (same error log
  hash). No historical hash or pinned identity changed; only the attribution
  and the evidence model did.
* **Entry-root boundary.** `main.ostw` (entry root) rejects at exactly three
  missing `../OSTWUtils/{OnScreenText,Cursor,StringSorting}.del` imports with
  an import-closure of exactly the seven reachable files. OSTWUtils remains
  uncompiled in Wright's corpus because the upstream repository has no
  license; its source is not copied.
* **Per-document table.** Every committed protect-ban source is recorded with
  its own root; documents whose closure avoids the missing imports compile,
  others reject — per-document evidence never stands in for project support.
* **#119 differential target.** The first forward-compilation differential
  targets are the pinned-reference-accepted, Wright-authored #118 semantic
  probes `p4-types-expressions` (147 elements),
  `p5-functions-control` (120 elements), and `p6-catalog-signatures`
  (68 elements), designated `differential-target` in their `probe.json`
  manifests and aggregated under `differentialTargets` in
  `probes/results.json`. Their sources are committed (immutable), the pinned
  reference identity is recorded per probe, and the oracle runner refuses to
  list a target that the reference rejects. A complete licensed real-world
  OSTW corpus remains unavailable (OSTWUtils is unlicensed; MOBAwatch is
  reference-rejected), so the probes are the initial target.
* **Determinism.** The corpus runner was re-run twice under the pinned
  reference with byte-identical `results.json`; the drift check
  (`run_oracle.py` without `--update`) fails on divergence. It is a manual
  maintainer command, not a Wright CI gate: the upstream-reference replay was
  removed from CI in #177, and future oracle reproducibility workflows belong
  to `del-rs` (del-rs#49, tracked in Wright by #182). Wright CI consumes the
  recorded evidence through the #119 compile differential; it does not
  re-derive it.

## Reference/oracle feasibility report

Verified on 2026-08-15 (linux-x64 self-contained build under linux/amd64
container emulation on an arm64 macOS host):

* **`--ping` works**: prints `Hello!` and exits 0. Cheapest CI smoke check.
* **Default CLI compile is not headless.** It prints "Press enter to copy code
  to clipboard", blocks on stdin, and fails when clipboard tooling (`xclip`)
  is absent. `out_file` in `ds.toml` is honored only on the workspace/LSP path
  (the in-memory compile path has no project URI), so the default runner is
  unusable as a deterministic oracle without a clipboard/X server.
* **Viable oracle path: the stdio language server (`--langserver`).** Standard
  LSP `initialize` + `textDocument/didOpen` triggers a workspace-aware compile
  that honors `ds.toml`; results arrive as `textDocument/publishDiagnostics`
  plus the custom `workshopCode` (string) and `elementCount` notifications
  (the same protocol the VS Code extension consumes). Output language is
  configurable. This is a deterministic, clipboard-free, non-interactive
  oracle.
* **Secondary path: browser-WASM interop (`OstwJavascript`).** Exposes
  `AddModelAsync`/`UpdateModelAsync` and `SetCompiledWorkshopCode`, but the
  WASM AppBundle is not shipped in release assets; it requires building with
  `dotnet publish -r browser-wasm` (needs a .NET 8 SDK), which is heavier and deferred.
* **Platform constraint.** Release assets are x64-only (win-x64/win-x86/
  linux-x64 self-contained; framework-dependent zips need the .NET 8 runtime).
  No macOS/arm64 builds. On arm64 hosts, a linux/amd64 container (verified via
  Docker) or a .NET 8 runtime is required.
* **E-level reference.** The upstream emulator (`Emulator/`, used by
  `Deltinteger.Tests` `EmulateTick` and `AtomizeAndReconstruct` round-trip)
  can serve as an E-level reference for the subset it implements, but `Wait`
  raises `NotImplementedException`, so timing-sensitive scenarios need the
  game runtime and are out of scope for the first milestone.
* **Recommendation.** Build the oracle runner as a small LSP-stdio client in
  the `compatibility/` harness (pinned to the proposed `v3.4.0` identity),
  invoke it only for S/D/N evidence, and keep it out of the Rust core. The
  `--ping` probe guards acquisition.

## Architecture reuse and boundary findings

Reusable Wright-owned contracts (issue #90's "converge on canonical Workshop
semantics/WIR" requirement):

* **Session/driver** (`wright-driver` `CompilerSession`, `SourceKind`): add
  `SourceKind::Ostw`; input resolution, file registry, provenance, diagnostics
  envelope, profile application, and result rendering are frontend-neutral.
* **HIR/WIR/lowering** (`wright-core`, `wright-ir`): the OSTW frontend lowers
  to Wright HIR; workshop-function calls resolve through the existing
  catalog/WIR path exactly like the OPY receiver-call surface (the canonical
  `workshop-rs` catalog, consumed via `wright-workshop`).
* **Workshop parser/emitter/round-trip** (`workshop-rs` via the
  `wright-workshop` adapter): Workshop stays the interoperability hub;
  OSTW → Workshop emission reuses the canonical emitter, and
  Workshop → OSTW reconstruction reuses the canonical Workshop parser and
  WIR, adding only a Wright-owned OSTW emitter.
* **Analyzer/language services/LSP** (`wright-analyzer`, `wright-language`,
  `wright-lsp`): symbols, references, CFG, hover/definition/references/rename/
  semantic tokens are language-neutral once the frontend produces HIR with
  provenance.
* **Compatibility harness and corpus tooling**: fixture schema, oracle
  identity blocks, S/D/N gates, `diff.py` producer contract, and
  `acquire-corpus.py`/`corpus-manifest.json` extend unchanged.

OSTW-frontend-specific (must remain frontend-specific):

* **Lexer/parser/CST** for the C#-style syntax plus the Vanilla superset:
  a new `wright-ostw` crate (or frontend module), mirroring `wright-opy`.
* **Type/name-resolution semantics** unique to OSTW: classes (heap, ≤999,
  inheritance), structs with `in`/`ref`, enums/pattern matching, generics,
  lambdas, macros (compile-time), `ds.toml` project model. Some values (arrays,
  strings, variables, enums) reuse HIR/WIR shapes; class/generic/lambda
  semantics are OSTW-specific.
* **OSTW emission naming for reconstruction**: the decompiler's function
  mapping is OSTW-idiomatic; Wright must own its mapping rather than import
  the upstream table (unlicensed source).
* **Oracle driver**: the LSP-stdio client is reference infrastructure in the
  harness, never a core dependency.

Directional evidence requirement (issue #90): OSTW → Workshop and Workshop →
OSTW are evaluated as separate surfaces with their own corpora and quality
criteria. No direct OPY ↔ OSTW conversion work is implied by this baseline.

## Proposed bounded OSTW decomposition (pending PM review)

Grouped by semantic responsibility, in dependency order. **These are proposal
categories, not created implementation issues.** The first milestone boundary
is A+B+C scoped to the MOBAwatch/protect-ban corpus.

| Phase | Semantic responsibility | Contents | Evidence gate |
| --- | --- | --- | --- |
| A | Reference & corpus baseline | Pin OSTW `v3.4.0` (content-verified); `compatibility/` oracle runner (LSP stdio client + `--ping` smoke); acquire MOBAwatch + protect-ban with manifests; S-level accept/reject record for corpus files | Corpus + oracle runner exist; every fixture records reference identity |
| B | OSTW frontend core | Lexer/parser/CST → HIR for categories 1–7 (rules, variables/scopes, control flow, values/workshop calls via catalog, core types/structs/enums, functions/macros/subroutines); structured source-located diagnostics | D-level diagnostics + S-level parity on corpus; HIR boundary tests |
| C | OSTW → Workshop emission | Lower HIR → WIR → Workshop (en-US); round-trip validation; normalized differential comparison against pinned explicit-root reference evidence | **Implemented by #119** for the declared accepted surface (the p4/p5/p6 differential targets): shared `wright-ir` lowering + shared Workshop emitter, `wright compile` enabled, round-trip fixed point and normalized differential in `crates/wright-ostw/tests/differential.rs`, CI-protected; see `docs/ostw/support-matrix.md`. Protect-ban entry-project compilation stays unclaimed (missing `../OSTWUtils/…` imports). |
| D | Workshop → OSTW reconstruction | Wright-owned OSTW emitter for the declared Workshop surface; reconstruction quality criteria (semantics + useful structure, no formatting/comments/macro recovery); round-trip tests | **Implemented by #125** (`wright_ostw::reconstruct`) and #126 (shared `CompilerSession::convert` + `wright convert --target ostw`, cross-format acceptance in `crates/wright-driver/tests/convert.rs`); reconstruction recompiles to equivalent WIR under the declared boundary and the #119 normalization |
| E | Tooling & language services | OSTW in `check`/`lint`/`analyze`/`inspect`; editor-neutral language services; LSP mapping; session/CI integration | Cross-input workflows without language-specific semantic forks. **Implemented by #120**: the shared session lowers the #118 semantic HIR through the shared validate→lower→validate path, the four CLI workflows and the tool service run the shared analyzer over it with multi-file provenance, language-service diagnostics/symbol classification work for OSTW documents, and cross-language regressions cover OPY/Workshop/OSTW. |
| Later | Evidence-prioritized | Classes/generics/lambdas/pattern matching (category 8–9) if the corpus demands them; lobby-settings authoring; multi-locale | Corpus or PM evidence |
| Explicitly deferred | - | Direct OPY ↔ OSTW (deferral recorded below, pending PM reassessment); debugger protocol; optimizer parity; perfect reconstruction; E-level timing scenarios | - |

## OSTW reverse-interoperability completion and the direct-OPY↔OSTW deferral (#126)

The reverse-interoperability integration of the OSTW work is complete: the #124
Workshop → OPY and #125 Workshop → OSTW reconstructors are both exposed
through **one shared driver/session conversion contract**
(`CompilerSession::convert` with explicit `opy`/`ostw` target selection; the
CLI is `wright convert --target opy|ostw`). The shared operation reuses the
driver's own `load()` path for validated Workshop input, calls the
language-owned reconstructors directly (no generic transpiler matrix, no
duplicated reconstruction logic), preserves the reconstructors' structured
diagnostics (stable codes, stage `reconstruction`, unsupported exit code 3)
with no partial output, and requires no upstream runtime (no OverPy/Node, no
OSTW .NET). The cross-format acceptance suite
(`crates/wright-driver/tests/convert.rs`) proves both reverse loops through
the real native frontends and writes `target/wright-convert-report.json`;
the declared conversion directions and limits are documented in
[`../cli.md`](../cli.md), [`support-matrix.md`](support-matrix.md), and
[`../opy/support-matrix.md`](../opy/support-matrix.md). The four supported
directions are OPY → Workshop, OSTW → Workshop, Workshop → OPY, and
Workshop → OSTW; reconstruction is semantic, never original-source recovery.

**Direct OPY ↔ OSTW source conversion remains explicitly deferred.** The
roadmap reassessment that would admit it is the PM's reassessment decision; this
baseline records the deferral and the readiness state (both directions
already meet at Wright-owned Workshop/WIR semantics, so a future decision can
build a direct path on the same owned contracts) but the reassessment itself
is an external human action outside this milestone's gates.

## Open questions

* ~~Whether the first milestone should include Workshop → OSTW reconstruction
  (D) or only the OSTW → Workshop pipeline (A–C)~~ — resolved: phase D was
  included and is complete (#125/#126).
* Whether `MOBAwatch`'s full include closure (asset/lobby files beyond `.ostw`)
  is needed for compile parity or only for emission parity.
* Whether any corpus construct demonstrates divergence between tag `v3.4.0`
  and master, forcing a pin change before the first milestone.
* Whether `Lava` licensing can be resolved to admit the official examples.
* Whether the PM's roadmap reassessment admits direct OPY ↔ OSTW conversion
  (deferred above).

## Related documents

* [`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md): pinned OSTW reference identity and provenance
* [`docs/compatibility.md`](../compatibility.md): S/D/N/E framework
* [`docs/opy/compatibility-baseline.md`](../opy/compatibility-baseline.md): the OPY counterpart this baseline mirrors
* [`docs/licensing.md`](../licensing.md), [ADR-0004](../adr/0004-overpy-licensing-boundary.md), [ADR-0007](../adr/0007-reference-pinning-policy.md)
* [Issue #90](https://github.com/wrightkit/wright/issues/90), [Issue #113](https://github.com/wrightkit/wright/issues/113)
