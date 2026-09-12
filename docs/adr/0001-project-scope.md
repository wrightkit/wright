# ADR-0001: Project scope

- Status: Superseded by [ADR-0008](0008-tooling-first-semantic-platform.md)
- Date: 2026-08-12
- Related: [Issue #1](https://github.com/wrightkit/wright/issues/1)

## Context

Wright began with a need for a stable responsibility boundary before compiler
packages, compatibility tooling, or generated output were implemented. Existing
OverPy supplied the `.opy` frontend/parser and the initial compatibility oracle.

## Decision

Wright v1 was defined as an independently implemented Rust compiler core for an
OverPy-compatible workflow. The core owns its bridge, HIR, Workshop IR,
lowering contracts, diagnostics, and backends. Existing OverPy remains an
external frontend/parser and oracle until a separately approved native frontend
decision changes that boundary.

The v1 non-goals are:

* a native Rust `.opy` parser;
* a full decompiler rewrite;
* a full LSP rewrite;
* a new language or intentionally incompatible `.opy` semantics; and
* reproducing OverPy internals merely for implementation parity.

## Consequences

Components under this decision cross an explicit Wright-owned boundary. This keeps
frontend-specific representations out of the core and allows compatibility to be
measured without treating an external implementation as Wright's architecture.
The initial workspace scope was therefore limited to core contracts and
libraries; adapters and backends cross the same boundary when their contracts
are defined.

## Compatibility impact

Compatibility claims use the levels in [`docs/compatibility.md`](../compatibility.md)
and identify the OverPy reference version and corpus. The scope decision alone
does not claim syntax, diagnostic, normalized-output, or semantic parity.

## Historical note

When ADR-0008 superseded this record, the v1 non-goal "a native Rust `.opy`
parser" was treated as historical. ADR-0008 retained the non-goals regarding
a new language and OverPy-internal parity. The questions about versions and
Workshop targets were addressed by later decision records.
