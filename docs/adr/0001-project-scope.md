# ADR-0001: Project scope

- Status: Superseded by [ADR-0008](0008-tooling-first-semantic-platform.md)
- Date: 2026-08-12
- Related: [Issue #1](https://github.com/wrightkit/wright/issues/1)

## Context

Wright is at the project-bootstrap stage. It needs a stable responsibility
boundary before compiler packages, compatibility tooling, or generated output
are implemented. Existing OverPy supplies the `.opy` frontend/parser and the
initial compatibility oracle.

## Decision

Wright v1 is an independently implemented Rust compiler core for an
OverPy-compatible workflow. The core owns its bridge, HIR, Workshop IR,
lowering contracts, diagnostics, and backends. Existing OverPy remains an
external frontend/parser and oracle until a separately approved native frontend
milestone.

The v1 non-goals are:

* a native Rust `.opy` parser;
* a full decompiler rewrite;
* a full LSP rewrite;
* a new language or intentionally incompatible `.opy` semantics; and
* reproducing OverPy internals merely for implementation parity.

## Consequences

Future components must cross an explicit Wright-owned boundary. This keeps
frontend-specific representation out of the core and allows compatibility to be
measured without treating an external implementation as Wright's architecture.
The first workspace may therefore contain only core contracts and libraries;
future adapters and backends are added when their contracts are needed.

## Compatibility impact

Compatibility claims use the levels in [`COMPATIBILITY.md`](../../COMPATIBILITY.md)
and identify the OverPy reference version and corpus. The scope decision alone
does not claim syntax, diagnostic, normalized-output, or semantic parity.

## Historical note

The v1 non-goal "a native Rust `.opy` parser" has been achieved: `wright-opy`
was shipped under the M7 milestone. The remaining non-goals (no new language,
no OverPy-internal parity) are carried forward in ADR-0008. The OverPy
openquestions (versions, Workshop target) were resolved in later ADRs.
