# ADR-0008: Tooling-first semantic platform rebaseline

- Status: Accepted
- Date: 2026-08-14
- Supersedes: [ADR-0001: Project scope](0001-project-scope.md)
- Amends: [ADR-0002: Compatibility strategy](0002-compatibility-strategy.md)
- Related: [Issue #88](https://github.com/wrightkit/wright/issues/88),
  [ARCHITECTURE.md](../../ARCHITECTURE.md),
  [COMPATIBILITY.md](../../COMPATIBILITY.md)

## Context

ADR-0001 defined Wright at bootstrap as "an OverPy-compatible Rust compiler
core" with OverPy as the external `.opy` frontend until a later native frontend
milestone. The v1 non-goals explicitly listed "a native Rust `.opy` parser".

The implemented repository has since grown to include a native `.opy` semantic
frontend (`wright-opy`), native Workshop parsing and emission, WIR/HIR,
semantic analysis, tool APIs, language services, and an LSP. ADR-0001's
non-goals are therefore factually contradicted by the current codebase.

`ARCHITECTURE.md` carries the same contradiction: the project-boundary section
still calls OverPy "the `.opy` frontend/parser" until "a later native frontend
milestone", while the Frontend section describes `wright-opy` as the v1 native
frontend.

M11 also produced evidence that treating every reference-output difference as
product-critical compatibility work can consume roadmap capacity without a
proportionate gain in real user value. That risk is a product-priority signal,
not a one-off exception.

This ADR corrects the record and establishes the boundary for post-M11 roadmap
work.

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
but compiler parity work must not consume the roadmap by default when it does
not block real compilation, analysis, source tooling, or a declared semantic
contract.

### 2. Semantic frontend ownership

Wright owns independent semantic frontends where required for standalone
compilation, source-aware analysis, agent source editing, CI, WASM/embedding,
and long-term ecosystem independence.

Current and planned ownership:

- **Vanilla Workshop** — Wright-owned canonical model, parser, emitter, and
  target semantics;
- **OPY** — Wright-owned compatible semantic frontend (`wright-opy`, shipped);
- **OSTW** — future first-class compatible semantic frontend, introduced only
  through an evidence-backed milestone (see issue #90).

Upstream compilers and language services (OverPy, OSTW) remain compatibility
oracles, behavior references, and test inputs. They are not production runtime
dependencies for supported standalone workflows.

### 3. Compatibility targets observable semantics

The compatibility contract is **semantic compatibility**, not compiler-output
identity.

Byte-identical output, identical temporary-variable allocation, identical
optimizer output, or identical formatting are not goals unless a difference
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

ADR-0001 is superseded. Its "native Rust `.opy` parser" non-goal is now
historical; `wright-opy` was shipped under the M7 milestone. The remaining
non-goals from ADR-0001 (no new language, no OverPy-internal parity) are
preserved in this ADR and in `ARCHITECTURE.md`.

### On ADR-0002 / COMPATIBILITY.md

ADR-0002's four-level S/D/N/E framework is preserved and remains normative.
This ADR adds a priority rule: **E-level observable semantics outrank N-level
output-text identity**. N-level differences that are purely presentational are
not automatically product bugs; they must be evaluated against the observable
and documented compatibility surface before creating implementation work.

### On ARCHITECTURE.md

The project-boundary section is rewritten to reflect the current tooling-first
product surface and Wright's ownership of its semantic frontends. The
contradiction between the "project boundary" text and the "Frontend" section is
resolved in favor of the implemented state.

### On roadmap

Issues #89 (M12 lint platform) and #90 (M13 OSTW) are the next roadmap items.
Both are consistent with this rebaseline. #89 makes the tooling-first direction
concrete; #90 adds a second semantic frontend through an evidence-backed
milestone, consistent with decision 2 above.

## Compatibility impact

No compatibility level is removed or weakened. The S/D/N/E levels from ADR-0002
remain normative. The priority clarification (semantic over text-identity)
affects roadmap triage and issue prioritization, not the measurement contracts
themselves.

## Open questions

- Which Workshop output targets and runtime versions will be covered by E-level
  scenarios beyond the current corpus (tracked in COMPATIBILITY.md open
  questions)?
- What corpus-licensing and local-generation process applies to future OSTW
  fixtures?
- What extension mechanism for third-party lint rules is justified by evidence
  (tracked in issue #89)?
