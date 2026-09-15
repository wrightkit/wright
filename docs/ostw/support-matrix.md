# OSTW Compile Support Matrix (historical/provider boundary)

Status: historical record. Wright no longer ships the static OSTW adapter;
`.ostw`/`.del` requests return `source-provider-unavailable`. Authoritative
compatibility evidence is maintained by `deltin-rs` after #49/#95.
Scope: the OSTW source surface Wright compiles to Workshop through the shared
HIR → WIR → Workshop pipeline, with owner-maintained pinned-reference
differential evidence,
the declared normalization contract, and the known limitations/divergences

This matrix records the former **declared compile surface**. Current Wright
does not compile `.ostw`/`.del` inputs because no DEL/OSTW provider is shipped; the pinned OSTW
reference and authoritative evidence are maintained by
[`deltin-rs`](https://github.com/wrightkit/deltin-rs); the forward-looking
tiered baseline lives in [`compatibility-baseline.md`](compatibility-baseline.md).
The corrected explicit-root evidence model is documented in the owner
repository. The pinned reference identity is recorded in
[`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md).

The historical owner-side pipeline was `deltin-rs` (project + syntax + semantic
analysis) → canonical WIR through a narrow adapter → canonical `workshop-rs`
emitter. That static path is removed from Wright; a future provider must own
the integration boundary.

## Accepted differential targets

The owner-side #122 explicit-root accepted targets are maintained in
`deltin-rs` as pinned-reference probes:

| Target | What it exercises | Reference element count |
| --- | --- | --- |
| `p4-types-expressions` | User enums, null/union, `<Number>` cast, ternary, `&&`/`||` (incl. statement-bodied `ping()` operands), player receivers, format strings | 147 |
| `p5-functions-control` | Typed value/void functions, parameter defaults, C-style `for`, `foreach`, `switch` (fallthrough + `break` + `default`), `return` → `Abort` | 120 |
| `p6-catalog-signatures` | Named/default argument binding against canonical Workshop catalog signatures, user-function defaults, enums | 68 |

The `deltin-rs` owner-side compatibility workflow compiles each target and
compares the native result with the pinned reference evidence under the
declared normalization. Wright's integration tests exercise the adapter and
shared driver contract separately; they do not replay or duplicate the owner
oracle.

## Declared semantic normalization (#119)

Applied identically to both sides before comparison (never at emission):

- **Constant folding** (`wright-transform` FoldConstants), including the
  reference's `x || true` → `True` / `x && false` → `False` domination folds
  and the `Vector(0,1,0)` → `Up` unit-vector form.
- **Write-once per-call player variables**: the reference materializes every
  void-function argument into a fresh player variable (`Set Player Variable(…,
  by, value)` then reads); the declared contract inlines those single-writer
  variables on both sides.
- **Foreach counters**: Wright lowers `foreach` to `For Global Variable` (the
  #118 HIR models the counter as a global); the reference uses a per-player
  counter. Workshop rule execution is atomic, so the loop semantics coincide;
  the reference's `For Player Variable(Event Player, v, …)` form and its
  loop-body reads normalize to the global form.
- **Null/unit vector idioms**: `Vector(0,0,0)` ≡ `Subtract(Left, Left)` and
  `Vector(1,0,0)` ≡ `Left` (reference output idioms, P6 evidence).
- **`Custom String` placeholder syntax**: `<0>` ≡ `{0}`.
- **Team colors ≡ Teams**: `Color.TEAM_1/2` and `Team.TEAM_1/2` are the same
  Workshop value (the ambiguous `Team 1`/`Team 2` spelling can resolve either
  way); Wright's emitter qualifies the unpinned Team/Color collision as
  `Team(Team 2)` so emitted text reparses deterministically.
- **Initialize-rule names**: the synthetic `Initial Global`/`Initial Player`
  rule names are presentation (the game keys rules by structure); Wright's
  shared lowering carries the OPY-surface names.

The following are **outside** the declared semantic comparison and excluded
from it (non-goals: optimizer parity, identical variable allocation names,
formatting parity, byte-identical output): variable-table identity (names,
slots, player-vs-global placement of foreach counters), rule element-count
comments, formatting whitespace, and the reference's constant-folded
arithmetic (the fold pass restores it on the Wright side).

## Declared lowerings

- Variables: explicit IDs honored, automatic slots lowest-free (pinned P3a);
  duplicate explicit IDs rejected with source-located diagnostics (P3b);
  player-variable receiver reads/writes/modifies (P3c).
- User enums: members lower to 0-based integers (P4).
- Value functions inline as expressions (parameters substituted); void
  functions inline as action sequences with per-call player-variable argument
  materialization (`by`, `by_0`, …), defaults resolved at the call site (P5).
- Statement-bodied value functions hoist their side-effect statements before
  the enclosing action and inline the terminal return value (P4).
- C-style `for` → `For Global Variable(variable, start, condition, step)`;
  `foreach` → `For Global Variable(counter, 0, Count Of(arr), 1)` with
  `Value In Array` element access (P5).
- `switch` → the reference's Skip-array dispatch: sequential case bodies
  (fallthrough = no skip), `break` = `Skip` over the remaining bodies,
  `default` = the last body, non-matching values skip to the default (P5).
  The jump table and break skips are computed over the emitted action counts.
- `return;` in a rule → `Abort` (P5).
- Ternary → `If-Then-Else`; `<T>expr` casts are emission pass-throughs (P4).
- Workshop calls resolve through the canonical Wright-owned catalog: named
  arguments bind by name in canonical signature order and omitted parameters
  take the catalog's `paramDefaults` (P6): `allPlayers` → `All Players(All
  Teams)`, `wait` → `Wait(duration, Ignore Condition)`, `isButtonHeld` →
  `Is Button Held(Event Player, Button(Ability 2))`, `startCamera` Facing
  defaults to `0`, HUD-text colors default to `Color(White)`, etc.
- Rule priority orders the emitted rules (stable sort by priority, lower
  first; synthetic initialize rules at priority 0) (P1).

## Boundaries and known limitations

- **Rejected deterministically** (structured, source-located diagnostics,
  never deferred to emission): missing imports (the `../OSTWUtils/…` edges),
  classes/`new`, `define` function macros, generics/lambdas/pattern matching,
  structs/`in`/`ref` semantics, extended collections, the missing
  `Cursor`/`Math`/`Diagnostics` surfaces, `continue`, loop-level `break`, and
  `return` inside function bodies (only a rule-level `return;` and the
  terminal value-return of a statement-bodied value function lower).
- **Declared divergences from the reference output** (semantically
  equivalent, documented above): foreach counters are globals (not per-player
  variables), void-function arguments materialize under the same names but
  table placement/identity differs, and the OPY-evidenced `Visible To and
  String` casing differs from the reference's `Visible To And String`
  (normalized in the differential).
- **Not claimed**: `protect-ban` entry-project compilation (its entry graph
  rejects at the three missing `../OSTWUtils/…` imports under the pinned
  reference too, per #122), classes/generics/lambdas, multi-locale output,
  optimizer/output parity, and original-source recovery in reconstruction
  (#125 is semantic reconstruction, never comments/formatting/macros recovery).
- The `Event Player` restricted-value diagnostic for direct uses in global
  rules remains deferred; the accepted targets use `Event Player` only in
  Ongoing Player rules.

## Current Workshop → OSTW boundary

Wright does not ship a static OSTW reconstructor or a DEL/OSTW provider.
`wright convert --target ostw` remains a recognized product boundary and
returns the structured `source-provider-unavailable` result without partial
source. OSTW reconstruction semantics and compatibility evidence belong to
`deltin-rs`; the former Wright-side conversion fixtures and suite were removed
with the static adapter so they cannot become a second owner contract.

## Evidence

- [`deltin-rs` compatibility evidence](https://github.com/wrightkit/deltin-rs/tree/main/compatibility/ostw):
  pinned reference identity, probes, recorded observations, and the
  owner-side differential workflow.
- `workshop-rs` catalog data (`crates/workshop-rs/src/catalog/data/catalog.json`):
  canonical catalog with `paramDefaults` (probe-evidenced) and the `abort`
  action, consumed from `workshop-rs`.
- `docs/ostw/compatibility-baseline.md`: the explicit-root evidence model
  and #122 correction.
