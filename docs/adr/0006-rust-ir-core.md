# ADR-0006: Rust IR core — typed IDs, arenas, and two-layer models

- Status: Accepted
- Date: 2026-08-12
- Related: [Issue #4](https://github.com/wrightkit/wright/issues/4),
  [Issue #20](https://github.com/wrightkit/wright/issues/20),
  [Issue #21](https://github.com/wrightkit/wright/issues/21),
  [Issue #22](https://github.com/wrightkit/wright/issues/22),
  [ADR-0003](0003-ir-boundary.md),
  [ADR-0005](0005-opy-hir-v1.md)

## Context

M3 must establish Wright's durable Rust IR so later analysis, lowering,
optimization, emission, and tooling can share one data model. The protocol
types from ADR-0005 (`wright/opy-hir` v1, in `wright-core::hir`) are the
serialized bridge contract between the frontend adapter and the core; they
use raw strings for identity (symbol names, function names, operator
spellings) and own no storage strategy. M3 requires a compiler-side model with
strongly typed identity, arena storage, source provenance, and an explicit
HIR-to-Workshop-IR boundary — without reimplementing OverPy internals and
without speculative nodes beyond the v0.1 corpus.

## Decision

A new `wright-ir` crate owns the compiler IR. It is protocol-agnostic and has
no dependencies.

1. **Typed IDs and arenas.** Every stable identity is a `wright_ir::ids::Id<T>`
   newtype index into an `wright_ir::arena::Arena<T>`: files, global/player
   variables, subroutines, constants, macros, rules, statements, expressions,
   workshop values, and workshop actions. IDs are opaque, comparable, and
   type-safe: an `ExprId` cannot be passed where a `RuleId` is expected.
   `Arena::get` is bounds-checked and returns `Option`, so a dangling or
   out-of-range ID is a recoverable invariant error, not a panic. Arenas are
   append-only for a program lifetime, which keeps IDs stable.
2. **Source model.** `wright_ir::source` defines `SourceFile`, `Position`,
   and `Span` with a typed `FileId`. Spans are optional on nodes; the
   protocol's half-open, 1-based span convention carries over unchanged.
3. **Two-layer models.** `wright_ir::hir` is the internal Opy HIR: the same
   semantics as the bridge protocol, but with typed symbol references
   (`GlobalVarId`, `PlayerVarId`, `SubroutineId`, `ConstantId`, `MacroId`)
   instead of name strings, a typed `BinaryOp`/`UnaryOp` instead of operator
   strings, and arena storage for statements and expressions.
   `wright_ir::wir` is the Workshop IR: workshop program structure (variables
   with indexes, subroutines with indexes, rules with events, conditions,
   actions, and values) with Wright-owned action/value nodes and a documented
   name policy (§Names below).
4. **Boundaries.** The bridge protocol types stay in `wright-core::hir`;
   `wright-core` converts a validated protocol `Program` into
   `wright_ir::hir::Program` and lowers that into `wright_ir::wir`. The
   lowering lives in `wright_ir::lower`. Text emission and optimization are
   explicitly out of scope for the v0.1 lowering.

## Names

Workshop IR keeps Wright's source-level function names (`len`, `debug`,
`wait`, `createBeam`, `range`) as the `name` on call/value nodes. Mapping
those to Workshop presentation names (`Count Of`, `Create HUD Text`, …) is an
emission concern for a later milestone, not IR content. Exceptions: `debug`
and `print` lower to first-class `Action::Debug`/`Action::Print` nodes, and
`.append` lowers to `ModifyGlobalVariable`/`ModifyPlayerVariable` with an
`AppendToArray` op, because those express distinct source intents that the
workshop expresses as structured ops.

## Consequences

The IR is inspectable, type-safe, and free of OverPy internals. Conversion and
lowering are the two named boundaries where unsupported constructs are
reported structurally (stable code + span). The cost is intentional
duplication between bridge protocol types and internal model types, per
ADR-0005.

## Compatibility impact

IR equality is not semantic evidence (ADR-0003). This ADR defines the model
behind S/D-level evidence only; no N/E-level claim is made. The v0.1 corpus
must convert and lower without lossy catch-all nodes.

## Open questions

* Whether `debug`/`print` should become HUD actions in the Workshop IR once an
  emitter exists, and where the function-name mapping table should live.
* Whether call names need interning once analysis (M4) needs identity
  comparisons at scale.
* How user-defined enums (folded by the frontend today) should be represented
  once a native frontend milestone exists.
