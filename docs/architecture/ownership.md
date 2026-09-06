# Wright Ownership and Dependency Contract

Wright is WrightKit's user-facing tooling and integration product. It does not own the raw Workshop, OverPy, or DEL/OSTW language implementations.

## Repository ownership

- `workshop-rs` owns canonical raw Workshop semantics, parser, WIR, catalog, settings/localization, validation, emission, and Workshop-owned gameplay/query data.
- `opy-rs` owns OverPy syntax, preprocessing/macros, semantic resolution/HIR, diagnostics/provenance, OverPy-specific compiler/lowering behavior, standalone tooling, compatibility evidence, and Workshop→OPY reconstruction.
- `deltin-rs` owns DEL/OSTW project/source/type/runtime semantics, typed HIR, diagnostics/provenance, DEL-specific compiler/lowering behavior, standalone tooling, compatibility evidence, and Workshop→DEL/OSTW reconstruction.
- `language-provider-protocol` owns the process/data wire contract.
- Wright owns the unified tooling/product layer: orchestration, cross-language diagnostics, lint/static analysis, semantic query, source-edit transaction safety/refactoring, agent/embedding APIs, CI presentation, editor-neutral language services/LSP, and integration adapters.

## Dependency direction

```text
Wright ─────► opy-rs ─────► workshop-rs
   │
   ├────────► deltin-rs ───► workshop-rs
   │
   └───────────────────────► workshop-rs
```

LPP may provide a process boundary and does not require a cross-repository Rust dependency.

Language/core repositories must not depend back on Wright tooling internals. Wright adapters translate owner contracts; they do not become a second language implementation.

## Capability ceiling

A Wright command or adapter surface does not prove semantic support.

```text
Wright language-specific claim
    ≤ owning implementation's executable support
      + verified Wright integration
```

Legacy behavior from the historical Wright monolith is not authority after ownership moves to a dedicated implementation.

When an owning implementation lacks a source-language or Workshop semantic capability, fix the owner. Do not add compensating language semantics to Wright merely to make the integrated command appear to work.

## Product priority

Wright remains tooling-first:

1. check / diagnostics;
2. lint / static analysis;
3. inspect / semantic query;
4. validated source edits / refactoring;
5. agent / embedding;
6. CI / language services;
7. Workshop stability/cost analysis;
8. compilation/conversion needed by real workflows.

Architecture work is justified when it protects these workflows, ownership/contracts, provenance/edit safety, or an observed maintenance risk; internal symmetry and migration polish are not product goals by themselves.