# ADR-0010: Independent language implementations and Wright integration

- Status: Accepted
- Date: 2026-08-19
- Clarifies: ADR-0008 and ADR-0009 terminology and product boundaries
- Related: `workshop-rs`, `opy-rs`, `del-rs`, `language-provider-protocol`

## Context

WrightKit's repository split established separate owners for raw Workshop,
OverPy, and DEL/OSTW semantics. Existing Wright documentation often calls
`opy-rs` and `del-rs` **frontends** or **providers** because Wright consumes
language intelligence through frontend-like semantic models and LPP provider
processes.

Those terms are valid at specific technical boundaries but are misleading when
used as repository identities. They imply that the language repositories exist
primarily as internal components of Wright.

The intended ecosystem is instead composed of independently usable language /
Workshop implementations plus a deeply integrated tooling product:

- `workshop-rs` is the standalone raw Workshop implementation and canonical
  Workshop semantic core;
- `opy-rs` is the standalone OverPy implementation;
- `del-rs` is the standalone DEL/OSTW implementation;
- Wright consumes all three and adds unified tooling, orchestration, analysis,
  source editing, agent/CI/embedding, and language-service capabilities.

This distinction matters because the completeness of Wright's language-specific
features is bounded by the owning implementation, while the implementations
must remain useful without Wright.

## Decision

### 1. Repository identity

`opy-rs`, `del-rs`, and `workshop-rs` are independent implementations, not
Wright-owned frontend/provider repositories.

Each implementation owns its standalone library/CLI surface and may be consumed
by Wright or other tools.

### 2. Meaning of frontend

**Frontend** names an internal implementation stage that converts authored
source into syntax/project/semantic representations and HIR where applicable.

A frontend may intentionally remain independent from Workshop emission so
`check`, diagnostics, inspect/query, and source tooling can operate without a
complete compiler backend.

Documentation may use *frontend* for that stage, but must not use it as the
primary identity of `opy-rs` or `del-rs`.

### 3. Meaning of provider

**Provider** names an integration role. An implementation can expose a long-lived
process or other interface conforming to LPP so tooling clients such as Wright
can consume diagnostics, semantic queries, and source edits without sharing
compiler internals.

Provider support does not make the implementation subordinate to Wright and does
not require standalone users to route through Wright.

### 4. Workshop dependency

OverPy and DEL/OSTW compile to Workshop, so their complete implementations may
depend on `workshop-rs` for canonical Workshop semantics, identities, WIR,
validation, settings/localization, parsing, and emission.

This is the durable dependency direction:

```text
opy-rs ─────► workshop-rs ◄───── del-rs
                  ▲
                  │
                Wright
```

Source-language-specific compiler behavior and Workshop-to-source
reconstruction remain owned by the source-language implementation.

A dependency on `workshop-rs` therefore does not make `opy-rs` or `del-rs`
incomplete by design; it prevents duplicate raw Workshop implementations.

### 5. Wright product boundary

Wright owns the unified product experience across source forms:

- check/diagnostics orchestration;
- lint/static analysis;
- semantic inspection/query;
- validated source edits and refactoring;
- agent and embedding APIs;
- CI and machine-readable presentation;
- language services/LSP;
- cross-source workflow orchestration;
- compilation/conversion UX where the owning implementations support it.

Wright must not compensate for missing language semantics by recreating an
independent authoritative OPY, DEL, or Workshop implementation inside the
integration layer.

### 6. Capability claims

Wright's support claims cannot exceed the current executable support of the
owning implementations merely because a command or legacy in-repo path exists.

Current implementation reality and project intent must remain distinct. Public
documentation should describe partial compiler or reconstruction paths as
partial until the owning repository has end-to-end evidence.

### 7. Development ordering

Real user workflows are the default prioritization signal.

When Wright fails on a real project, first identify the owning layer:

- OPY syntax/semantic/compiler gap → `opy-rs`;
- DEL/OSTW syntax/semantic/runtime/compiler gap → `del-rs`;
- canonical Workshop semantic/WIR/catalog gap → `workshop-rs`;
- integration/lint/analysis/source-edit/agent/CI UX gap → Wright.

Architecture cleanup should not displace user-visible functionality unless it
protects a public/versioned contract, repository ownership, dependency
direction, provenance/source-edit correctness, licensing boundary, or an
observed high-cost maintenance risk.

## Consequences

- Repository READMEs and AGENTS files should describe the three implementation
  repositories as independently usable products/components.
- `frontend` remains valid in internal pipeline documentation.
- `provider` remains valid in LPP/process documentation.
- LPP stays a neutral integration protocol rather than the definition of a
  language repository.
- Wright's architecture docs that use older frontend/provider repository
  shorthand are interpreted through this ADR and should be updated when touched.
- Compatibility/support matrices continue to define current executable support;
  this ADR does not promote incomplete features.

## Non-goals

- Requiring every repository to ship identical CLI commands.
- Forcing a single Rust API boundary when LPP/process isolation is preferable.
- Making `workshop-rs` depend on OPY/DEL implementation details.
- Expanding the direct OPY↔DEL conversion matrix for symmetry.
- Claiming compiler or reconstruction completeness that current evidence does
  not support.
