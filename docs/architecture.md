# Wright Architecture

Status: accepted baseline (tooling-first semantic platform, ADR-0008)
Scope: the Wright tooling and compiler core, its semantic frontends, and
compatibility boundaries

This document defines the responsibilities and observable contracts that later
development may rely on. It does not promise that every component named here is
implemented at any given moment.

## Product surface

Wright is a **tooling-first semantic platform** for the Overwatch Workshop and
OverPy ecosystem. Its primary product surface is:

- `check` and deterministic diagnostics;
- lint and static analysis;
- source inspection and query;
- safe source edits and refactoring;
- agent tooling and embedding APIs;
- Workshop cost and stability reasoning.

Compilation and language-surface conversion are required infrastructure and
first-class user capabilities. Compiler parity work must not consume the
roadmap by default when it does not block real compilation, analysis, source
tooling, or a declared semantic contract.

## Project boundary

Wright owns independent semantic frontends where required for standalone
compilation, source-aware analysis, agent source editing, CI, WASM/embedding,
and long-term ecosystem independence.

Current ownership:

- **Vanilla Workshop**: Wright-owned canonical model, parser, emitter, and
  target semantics (`wright-workshop`);
- **OPY**: Wright-owned compatible semantic frontend (`wright-opy`);
- **OSTW**: Wright-owned compatible semantic frontend (`wright-ostw`),
  introduced under M13 per ADR-0008; native syntax/project frontend and HIR
  lowering for the declared reachable corpus slice.

Upstream compilers and language services (OverPy, OSTW) remain compatibility
oracles, behavior references, and test inputs. They are **not** production
runtime dependencies for supported standalone workflows.

Workshop is the canonical interoperability and target boundary. The required
long-term conversion directions are:

```text
OPY      → Workshop   (implemented: wright-opy + shared compile pipeline)
OSTW     → Workshop   (implemented: wright-ostw + shared compile pipeline, #119)
Workshop → OPY        (implemented: wright_opy::reconstruct via the shared
                        driver/session conversion operation, #124/#126)
Workshop → OSTW       (implemented: wright_ostw::reconstruct via the shared
                        driver/session conversion operation, #125/#126)
Workshop → Workshop
```

The reverse directions (Workshop → OPY, Workshop → OSTW) are **semantic
reconstruction**: the language-owned reconstructors convert validated
Wright-owned WIR back into canonical source for their language, driven through
one shared `CompilerSession::convert` operation with explicit `opy`/`ostw`
target selection (M13 phase D). Reconstruction does not recover
comments/formatting/macros/functions/source abstractions and is never
original-source recovery. A generic transpiler matrix is an explicit
non-goal: each reverse direction is owned by its language frontend crate.

Direct OPY ↔ OSTW source conversion is optional and must not drive the core
architecture prematurely; it remains explicitly deferred (M13 records the
deferral pending PM reassessment, see
[`docs/ostw/compatibility-baseline.md`](ostw/compatibility-baseline.md)).

The intended primary flow is:

```text
`.opy` or Workshop source
    → wright-opy / wright-workshop (owned semantic frontend)
    → Wright HIR
    → Wright Workshop IR (WIR)
    → Wright backend
    → Workshop-oriented output
```

Compatibility evaluation is a side path, not a core dependency:

```text
the same input and produced artifact
    → compatibility harness
    → documented oracle and/or behavior runner
```

The harness may invoke an installed OverPy tool or consume its documented
outputs. It must not make the Rust core depend on OverPy or OSTW
implementation types.

## Component responsibilities

### Frontends

Wright owns its semantic frontends. A frontend is responsible for
source-language parsing, preprocessing, syntax rules, and producing typed
Wright data at the HIR boundary. Frontends never expose external AST types
through Wright APIs.

**`wright-opy`**: the native Rust OPY frontend (lexer → preprocessing
includes/defines → CST/parser → semantic resolution → Opy HIR). The supported
surface is declared in [`docs/opy/support-matrix.md`](opy/support-matrix.md)
and verified at the HIR boundary by the differential suite. The pinned OverPy
adapter remains the compatibility oracle.

**`wright-ostw`**: the native Rust OSTW frontend (lexer → CST/parser → project
settings `ds.toml` → reachable import-closure resolution → semantic lowering
to Wright HIR). The supported baseline is documented in
[`docs/ostw/compatibility-baseline.md`](ostw/compatibility-baseline.md). Pinned
OSTW (`v3.4.0`) serves as the compatibility oracle.

**`wright-workshop`**: the native Workshop frontend and emitter (localized
catalog, lexer, parser, validation, and emitter). Workshop is the canonical
target for all frontends.

### Adapter and bridge

The adapter/bridge is the narrow translation boundary between a frontend and
Wright's owned representations. It is responsible for:

* accepting a documented frontend result or an explicitly supported interchange
  format;
* validating identifiers, references, source spans, and unsupported constructs;
* preserving source provenance where it is available; and
* producing typed Wright data or structured diagnostics.

The bridge must not expose frontend-internal types through a Wright API. A
frontend-specific change should be contained at this boundary unless it changes
the Wright contract itself.

### HIR

HIR is Wright's frontend-independent, semantic representation. It describes
the meaning needed by later compiler stages: declarations, references,
expressions, control flow, source provenance, and explicit unsupported cases as
the supported subset grows.

HIR must not encode a particular Workshop text layout or depend on an OverPy
AST type. Its contract is semantic identity and validated relationships, not
source-text preservation.

### Workshop IR

Workshop IR (WIR) is Wright's target-oriented representation. It contains the
validated operations, values, control-flow constructs, and metadata required by
Workshop backends. Lowering from HIR to WIR is explicit so that target
constraints and unsupported semantics are observable at a named boundary.

WIR must be deterministic for the same validated HIR and compiler
configuration. It may carry source provenance for diagnostics, but it must not
become a catch-all container for frontend implementation details.

### Backend

A backend consumes WIR and produces a documented Workshop-oriented artifact or
a structured diagnostic. It owns target serialization, formatting, and
target-specific validation. It does not reparse source or consult upstream
compiler internals as part of normal compilation.

### Compiler driver and CLI

The compiler/session driver (`wright-driver`) is the single orchestration path
shared by the CLI, library consumers, and tool/LSP adapters: input discovery →
frontend selection → validation → lowering → analysis → emission. It also owns
the shared conversion operation ([`CompilerSession::convert`], target `opy` |
`ostw`), which reuses the load path and delegates per target to the
language-owned reconstructors (`wright_opy::reconstruct`,
`wright_ostw::reconstruct`) — the driver maps their structured rejections
into the shared `Diagnostic` contract (stable codes preserved, stage
`reconstruction`) and never carries reconstruction logic itself (M13 phase D,
#126). The `wright` CLI (`wright-cli`) is a thin argv/presentation layer;
human text and machine-readable JSON (`wright-result/v1`) are two renderings
of the same typed result envelope. The normative contract is
[`docs/cli.md`](cli.md).

### Semantic analysis and tooling

`wright-analyzer` provides symbols, references, control-flow graphs, and
semantic findings. The tooling-first product surface builds on this: lint rules,
stability findings, inspection queries, and refactoring source edits are all
consumers of the semantic layer. Tool and agent APIs are exposed through
`wright-driver`/`wright-serve` rather than through separate services.

Source-oriented **semantic edits against the original source** are the default
mutation model for agent and refactoring tooling. Full AST/IR regeneration with
comment and formatting preservation is not the default.

### Language services and LSP

`wright-language` provides editor-neutral language services; `wright-lsp` maps
them to the LSP transport. Language services are consumers of the semantic
layer and do not depend on backend or compatibility tooling.

### Compatibility harness and oracle

Compatibility tooling is an evaluation boundary around the core. It records
the reference version, corpus identity, normalization rules, and comparison
result for each claim. It may call OverPy, compare normalized artifacts, or run
behavioral scenarios according to [`docs/compatibility.md`](compatibility.md).

The harness is not evidence that the core may link to or copy OverPy code. The
engineering and licensing boundary is defined in
[`docs/licensing.md`](licensing.md).

## Dependency direction

Dependencies point toward owned, stable contracts:

```text
frontend-specific adapter → HIR → Workshop IR → backend
                                      ^
                          compatibility harness (evaluation only)
```

More precisely:

* frontend adapters may depend on the HIR and bridge contracts;
* HIR must not depend on a frontend or backend;
* WIR may depend on HIR concepts during lowering, but HIR must not depend on
  WIR;
* backends may depend on WIR, not on parser internals;
* compatibility tooling may depend on public artifacts and documented test
  interfaces, but the core must not require the oracle to compile or run.

The workspace layout is allowed to grow around these boundaries. A package is
not justified merely because a future component is named here; its public
contract should be introduced with the milestone that needs it.

## Cross-cutting contracts

The following are normative:

* **Tooling-first priority:** compiler parity work must not consume the roadmap
  when it does not block real compilation, analysis, source tooling, or a
  declared semantic contract.
* **Explicit unsupported behavior:** unsupported syntax or semantics produces a
  structured diagnostic or a documented rejection. Silent fallback is not a
  compatibility strategy.
* **Provenance:** source spans and relevant origin information are preserved
  across transformations where the input provides them.
* **Determinism:** equal validated inputs, configuration, and toolchain produce
  stable observable IR and output, subject to explicitly documented volatile
  fields.
* **Semantic compatibility over output identity:** observable Workshop/game
  behavior, valid syntax, source/tooling contracts, and documented compatibility
  surfaces outrank byte-identical text output. Presentation-only output
  differences are not automatically product bugs.
* **Corpus-defined support:** "supported" means the declared corpus/surface is
  parseable, semantically understood, compilable where claimed, and analyzable
  through the declared tooling contracts. It does not guarantee successful
  execution in every live Overwatch runtime.
* **Owned public interfaces:** Wright APIs expose Wright-owned types and
  contracts, not convenient aliases for external implementation details.
* **No source-language forking:** Wright must not invent Wright-only OPY or
  OSTW syntax or language features. Language-level evolution belongs upstream
  or behind an explicit experimental proposal.

## Non-goals

The following remain outside the current contract:

* inventing Wright-only OPY or OSTW language features;
* reproducing OverPy internals merely for implementation parity;
* a full decompiler rewrite without an accepted contract;
* direct OPY ↔ OSTW source conversion without both frontends and evidence;
* full OSTW ecosystem parity before evidence-backed milestone completion; and
* guaranteeing successful execution in every live Overwatch runtime without
  separate runtime evidence.

A native Rust `.opy` parser (formerly listed as an initial non-goal in ADR-0001)
is implemented as `wright-opy`. That non-goal entry is historical.

## Open questions

These questions are intentionally left for milestones that have enough
implementation evidence to answer them:

1. Which Workshop output targets and runtime versions are covered by E-level
   semantic scenarios beyond the current corpus?
2. Which diagnostic codes and machine-readable fields are stable enough for
   external clients?
3. Which compatibility corpus entries can be redistributed, and which must be
   generated locally?
4. What extension mechanism for third-party lint rules is justified by evidence?
5. Which licensing questions require advice from qualified counsel?

Decisions that answer or materially revise these questions belong in an ADR.

## Related decisions

* [ADR-0001: Project scope](adr/0001-project-scope.md) _(superseded by ADR-0008)_
* [ADR-0002: Compatibility strategy](adr/0002-compatibility-strategy.md)
* [ADR-0003: IR boundary](adr/0003-ir-boundary.md)
* [ADR-0004: OverPy licensing and clean-room boundary](adr/0004-overpy-licensing-boundary.md)
* [ADR-0005: Opy HIR v1 frontend protocol](adr/0005-opy-hir-v1.md)
* [ADR-0006: Rust IR core: typed IDs, arenas, and two-layer models](adr/0006-rust-ir-core.md)
* [ADR-0007: OverPy reference pinning policy](adr/0007-reference-pinning-policy.md)
* [ADR-0008: Tooling-first semantic platform rebaseline](adr/0008-tooling-first-semantic-platform.md)
