# ADR-0003: IR boundary

- Status: Accepted
- Date: 2026-08-12
- Related: [`ARCHITECTURE.md`](../../ARCHITECTURE.md)

## Context

The frontend, semantic compiler stages, and Workshop serialization have
different responsibilities. Passing a third-party AST through the whole system
would make an external implementation's representation an accidental public
contract and would make target-specific concerns difficult to test.

## Decision

Wright uses two owned intermediate boundaries:

1. **HIR** is frontend-independent and semantic. The bridge validates frontend
   data, preserves source provenance where available, and produces HIR-owned
   types.
2. **Workshop IR** is target-oriented and deterministic. An explicit lowering
   step maps validated HIR into the operations and values required by a
   Workshop backend.

Backends consume Workshop IR and do not reparse source or depend on frontend
   internals. Neither IR exposes an external AST type. Unsupported constructs
   remain explicit diagnostics or documented rejection at the earliest boundary
   that can identify them.

## Consequences

Frontend and backend changes are isolated behind named contracts, and the
semantic meaning of a program can be tested independently of output formatting.
The exact fields, versioning policy, and first supported construct set must be
defined when the first implementation path requires them.

## Compatibility impact

HIR and Workshop IR comparisons may support normalized-output evidence, but IR
equality alone does not establish semantic compatibility. Provenance and
determinism are part of the transformation contract and should be covered by
tests as the representations become executable.

## Open questions

The initial HIR/Workshop IR schema, identity rules, diagnostic code set, and
versioning policy are intentionally deferred until a concrete compiler slice is
implemented.
