# Wright Architecture

Status: accepted living architecture.

Wright is WrightKit's unified tooling and integration product. It does not own
the complete OverPy, DEL/OSTW, or raw Workshop implementations. Those durable
responsibilities belong to `opy-rs`, `del-rs`, and `workshop-rs` respectively.

This document describes Wright's product boundary and how it consumes those
implementations. Historical migration decisions remain in the ADRs; current
capability claims must follow executable evidence in the owning repositories.

## Product surface

Wright is tooling-first. Its primary user-facing responsibilities are:

- `check` and deterministic diagnostics;
- lint and static analysis;
- semantic inspection and query;
- validated source edits and refactoring;
- agent tooling and embedding APIs;
- CI and machine-readable presentation;
- editor-neutral language services and LSP;
- Workshop stability/cost analysis;
- unified compilation/conversion UX where the owning implementation supports
  the requested direction.

Compilation and conversion are important capabilities, but they do not justify
recreating missing language semantics inside Wright.

## Ecosystem dependency model

```text
                    Wright
       unified tooling / orchestration
 check · lint · analyze · inspect · edit
       agent · CI · LSP · embedding
                      │
        ┌─────────────┼─────────────┐
        ▼             ▼             ▼
     opy-rs         del-rs      workshop-rs
     OverPy         DEL/OSTW      Workshop
 implementation  implementation  implementation
        │             │             ▲
        └─────────────┴─────────────┘
              canonical Workshop
```

Durable ownership:

- `workshop-rs`: standalone raw Workshop implementation and canonical Workshop
  parser/WIR/catalog/settings/localization/validation/emission owner;
- `opy-rs`: standalone OverPy implementation, including source semantics,
  compiler/lowering behavior, diagnostics/provenance, standalone tooling, and
  Workshop→OPY reconstruction;
- `del-rs`: standalone DEL/OSTW implementation, including project/semantic and
  runtime/compiler behavior, diagnostics/provenance, standalone tooling, and
  Workshop→DEL reconstruction;
- `language-provider-protocol`: neutral LPP process/data contract;
- Wright: integration and cross-language tooling/product behavior.

The intended Rust dependency direction is toward the owning implementation.
LPP may also be used as a process boundary where isolation/versioning makes it
the better integration choice.

No language/core implementation should depend back on Wright tooling internals.

## Terminology

### Frontend

A **frontend** is an internal stage inside a language implementation that turns
authored source into parsed/project/semantic representations and HIR where
applicable.

It is useful to distinguish a Workshop-independent semantic frontend from the
compiler backend because `check`, inspect/query, and source tooling need not
wait for complete target emission.

`frontend` is not the product identity of `opy-rs` or `del-rs`.

### Provider

A **provider** is an integration role through which a language implementation
exposes capabilities to a tooling client. LPP is the stable protocol for this
process boundary.

Provider conformance proves protocol behavior, not semantic completeness. A
standalone implementation can expose a provider and still remain independently
usable through its own library and CLI.

See ADR-0010 for the durable terminology decision.

## Source-form integrations

### Raw Workshop

Wright consumes canonical raw Workshop semantics from `workshop-rs`:

```text
Workshop text
   ↓
workshop-rs parser / canonical WIR / catalog validation
   ↓
Wright tooling
   ├─ diagnostics / lint / analysis / inspect
   ├─ source-edit orchestration where supported
   ├─ agent / CI / embedding
   └─ emission / locale conversion through workshop-rs
```

Wright must not maintain a second authoritative Workshop catalog, parser, WIR,
or emitter.

### OverPy

```text
OPY source
   ↓
opy-rs semantic implementation
   ├─ standalone check / inspect / queries
   ├─ OPY-specific compiler behavior
   └─ canonical Workshop integration through workshop-rs
           ↓
        Wright tooling
```

Wright may consume native APIs and/or an LPP provider depending on the product
boundary. It must not implement missing OPY syntax/semantic/compiler behavior in
the integration layer merely to keep a Wright command working.

### DEL / OSTW

```text
DEL / OSTW source
   ↓
del-rs project / semantic / runtime implementation
   ├─ standalone check / inspect / queries
   ├─ DEL-specific compiler/runtime lowering
   └─ canonical Workshop integration through workshop-rs
           ↓
        Wright tooling
```

The same ownership rule applies: DEL/OSTW semantics remain in `del-rs`.

## Tooling data flow

Wright consumes structured semantic information from the owning implementation
and exposes product-oriented services:

```text
source implementation
   ↓
structured diagnostics + semantic identities + queries + provenance
   ↓
Wright session/tool services
   ├─ check
   ├─ lint/static analysis
   ├─ inspect/query
   ├─ validated edits/refactoring
   ├─ language services/LSP
   ├─ agent/embedding APIs
   └─ CI presentation
```

Where a task requires target Workshop semantics, Wright composes source-language
information with canonical `workshop-rs` contracts instead of introducing a
Wright-owned shadow semantic model.

## Lint and analysis

Wright owns cross-language lint, stability, cost, and analysis capabilities.
Rules should operate on stable semantic information rather than parser-specific
or provider-specific implementation details whenever practical.

A rule must preserve its evidence classification and known limitations. Source
implementations provide language meaning; Wright determines cross-language
product analysis and presentation.

## Source edits and refactoring

The default mutation model is:

```text
semantic understanding
   ↓
validated edit intent
   ↓
source-owned ranges/provenance
   ↓
transaction-safe edits to original source
   ↓
re-check / re-analyze
```

Whole-file regeneration is not required for normal agent/refactoring workflows.
Unsafe or unsupported edits fail explicitly; Wright must not silently degrade to
textual search/replace when semantic correctness is required.

## Agent, embedding, and CI

Wright owns reusable task-oriented services for programmatic consumers. CLI,
LSP, agent/MCP-style adapters, CI, and embedding should reuse the same semantic
and edit services rather than each building language-specific parsers or query
models.

Machine-readable contracts are versioned and deterministic where declared.
Presentation layers must not change semantic results.

## Compilation and conversion

Workshop is the interoperability hub:

```text
OPY ─────► Workshop ◄───── DEL/OSTW
                │
                ▼
             Workshop
```

Supported conversion directions include source→Workshop, Workshop→supported source,
and Workshop→Workshop. Direct OPY↔DEL translation is optional and should not
drive architecture for symmetry.

Source→Workshop compilation is owned jointly by the source-language
implementation (source semantics/lowering) and `workshop-rs` (canonical
Workshop semantics/validation/emission). Wright owns the integrated UX.

Workshop→OPY and Workshop→DEL reconstruction belong to the respective language
implementations and consume canonical Workshop semantics from `workshop-rs`.
Wright does not claim those directions as supported until the owning repositories
have executable evidence.

## Capability claims

The existence of a Wright command is not evidence that every source-language
construct is supported.

Public claims must satisfy:

```text
Wright claim ≤ owning implementation's executable evidence
```

Legacy behavior from the old Wright monolith is not authoritative after
ownership moves to a dedicated repository. Support matrices, corpus evidence,
real-project gates, and current integration tests determine actual coverage.

## Failure routing

When a real workflow fails, identify the owner before changing code:

- OPY syntax/preprocess/semantic/compiler gap → `opy-rs`;
- DEL/OSTW project/semantic/runtime/compiler gap → `del-rs`;
- canonical Workshop parser/WIR/catalog/settings/localization/emission gap →
  `workshop-rs`;
- LPP wire/conformance gap → `language-provider-protocol`;
- lint/analysis/edit/agent/CI/LSP/integration UX gap → Wright.

Do not work around an owner gap in Wright unless the behavior is genuinely
Wright-owned.

## Architecture priority

Architecture work deserves product priority when it protects:

- a public/versioned contract;
- repository ownership or dependency direction;
- source provenance or edit correctness;
- canonical identities/schema;
- licensing/provenance boundaries;
- an observed high-cost reliability/maintenance problem.

Internal module layout, helper traits, temporary adapters, and organization are
revisable implementation details. They should not displace real user workflows
without evidence.

## Compatibility

Compatibility targets observable semantics, valid syntax, diagnostics,
provenance, analysis correctness, and declared source-edit/round-trip contracts.
It does not require compiler-output identity, matching temporary variables,
formatting, optimizer internals, or identical IR.

Upstream OverPy/OSTW remain compatibility oracles/research references where
permitted, not runtime dependencies for declared first-party workflows.

## Validation strategy

Wright product validation uses multiple layers:

- focused unit/integration tests;
- provider/LPP conformance where applicable;
- source implementation and Workshop canonical validation;
- minimized provenance-linked regressions;
- full real-project workflows;
- machine-readable/CI contract tests.

A large passing unit-test count does not establish product usability if the real
project that motivated the work still fails.

## Related decisions

- [ADR-0008: Tooling-first semantic platform](adr/0008-tooling-first-semantic-platform.md)
- [ADR-0009: Language ownership and licensing boundaries](adr/0009-language-ownership-licensing-boundaries.md)
- [ADR-0010: Independent language implementations and Wright integration](adr/0010-independent-implementations-and-wright-integration.md)
- [Compatibility](compatibility.md)
- [Embedding](embedding.md)
