# ADR-0008: Tooling-first semantic platform rebaseline

- Status: Accepted (amended by [ADR-0009](0009-language-ownership-licensing-boundaries.md))
- Date: 2026-08-14
- Supersedes: [ADR-0001: Project scope](0001-project-scope.md)
- Amends: [ADR-0002: Compatibility strategy](0002-compatibility-strategy.md)
- Related: [Issue #88](https://github.com/wrightkit/wright/issues/88),
  [docs/architecture.md](../architecture.md),
  [docs/compatibility.md](../compatibility.md),
  [ADR-0009: Language ownership and licensing boundaries](0009-language-ownership-licensing-boundaries.md)

## Context

ADR-0001 defined Wright at project start as "an OverPy-compatible Rust
compiler core" with OverPy as the external `.opy` frontend until a separately
approved native frontend decision. The v1 non-goals explicitly listed "a
native Rust `.opy` parser".

The implementation state that led to this rebaseline included a native `.opy`
semantic frontend (`wright-opy`), native Workshop parsing and emission, WIR/HIR,
semantic analysis, tool APIs, language services, and an LSP. ADR-0001's
non-goals no longer described that implementation state.

At the time of this decision, `ARCHITECTURE.md` carried the same contradiction:
its project-boundary section called OverPy "the `.opy` frontend/parser" until a
native frontend decision, while its Frontend section described `wright-opy` as
the v1 native frontend.

Compatibility work preceding this decision showed that treating every
reference-output difference as product-critical work can consume planning
capacity without a proportionate gain in real user value. That risk is a
product-priority signal, not a one-off exception.

This ADR corrects the record and establishes the product-priority boundary.

## Decision

### 1. Product priority: tooling first

Wright's primary product surface is semantic tooling:

- `check` and diagnostics;
- lint and static analysis;
- source inspection and query;
- safe source edits and refactoring;
- agent tooling and embedding APIs;
- Workshop cost and stability reasoning.

Compilation and conversion are required infrastructure and user capabilities,
but compiler parity work is secondary when it does not block real compilation,
analysis, source tooling, or a declared semantic contract.

### 2. Semantic frontend ownership

At the time of this decision, Wright's scope included independent semantic
frontends for standalone compilation, source-aware analysis, agent source
editing, CI, WASM/embedding, and long-term ecosystem independence.

The implementation areas considered by this decision were:

- **Vanilla Workshop**: Wright-owned canonical model, parser, emitter, and
  target semantics;
- **OPY**: an independently implemented compatible semantic frontend;
- **OSTW**: a first-class compatible semantic frontend only after a separate,
  evidence-backed decision.

Upstream compilers and language services (OverPy, OSTW) remain compatibility
oracles, behavior references, and test inputs. They are not production runtime
dependencies for supported standalone workflows.

### 3. Compatibility targets observable semantics

The compatibility contract is **semantic compatibility**, not compiler-output
identity.

Byte-identical output, identical temporary-variable allocation, identical
optimizer output, and identical formatting are not goals unless a difference
affects:

- observable Workshop or game behavior;
- valid Workshop syntax or native Workshop round-trip behavior;
- source or tooling contracts; or
- an explicitly documented compatibility surface.

N-level normalized-output evidence remains a useful regression-detection tool,
but it is supporting evidence rather than the ultimate product objective.
Reference-output differences that are presentation-only must not automatically
create implementation work.

### 4. Legacy and reference quirks

Default compatibility preserves corpus-evidenced observable upstream behavior
where real projects may depend on it. A future strict or fixed mode may
diagnose or correct known quirks, but this ADR does not require implementing
such a mode.

Language semantics, compatibility semantics, and legacy quirks are
conceptually distinguishable and must be kept separate in documentation.

### 5. Do not fork source languages

Wright may independently track new Workshop content (heroes, actions, values,
enums, settings, maps, localization and catalog data) and may expose
controlled or user-defined catalog or post-compile extension mechanisms where
justified.

Wright must not invent Wright-only OPY or OSTW syntax or language features as
a shortcut. Language-level evolution belongs upstream or behind an explicit
experimental proposal that does not silently redefine compatibility.

### 6. Conversion matrix

Workshop is the interoperability hub.

Required long-term capabilities:

- OPY → Workshop;
- OSTW → Workshop;
- Workshop → OPY;
- Workshop → OSTW;
- Workshop → Workshop.

Direct OPY ↔ OSTW source conversion is optional and must not drive the core
architecture prematurely.

### 7. Source-edit model

Agent and refactoring tooling should primarily use semantic, validated **source
edits against the original source**. Full AST or IR regeneration with
formatting and comment preservation is not the default mutation model.

### 8. Support claims are corpus-defined

"Supported" means the declared corpus or surface is parseable, semantically
understood, compilable where compilation is claimed, and analyzable through the
declared tooling contracts.

It does not guarantee successful execution in every live Overwatch runtime.
Runtime-sensitive claims require separate evidence.

## Consequences

### On ADR-0001

ADR-0001 is superseded. When this occurred, its "native Rust `.opy` parser"
non-goal was historical. The non-goals regarding a new language and OverPy-
internal parity are preserved in this ADR and in `ARCHITECTURE.md`.

### On ADR-0002 / COMPATIBILITY.md

ADR-0002's four-level S/D/N/E framework is preserved and remains normative.
This ADR adds a priority rule: **E-level observable semantics outrank N-level
output-text identity**. N-level differences that are purely presentational are
not automatically product bugs; they must be evaluated against the observable
and documented compatibility surface before creating implementation work.

### On ARCHITECTURE.md

The project-boundary section was rewritten to align with the tooling-first
product surface and the frontend scope described by this decision. The
contradiction between the "project boundary" text and the "Frontend" section
was resolved in favor of the implementation state at that time.

### On execution planning

This ADR does not set roadmap ordering, release sequencing, or Issue priority.
Execution planning belongs to Issues and release planning; a new semantic
frontend or tooling surface requires its own scope and evidence.

## Compatibility impact

No compatibility level is removed or weakened. The S/D/N/E levels from ADR-0002
remain normative. The priority clarification (semantic over text-identity)
affects how output differences are evaluated, not the measurement contracts
themselves.

## Scope boundaries

- This ADR does not choose Workshop output targets or runtime versions for E-level
  scenarios beyond the declared corpus.
- Corpus licensing and local-generation processes for OSTW fixtures are outside
  this ADR's scope.
- Third-party lint-rule extension mechanisms require a separate,
  evidence-backed decision.
