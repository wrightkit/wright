# OSTW Compile Support Matrix

Status: accepted baseline — first declared OSTW forward-compilation surface (#119)
Scope: the OSTW source surface Wright compiles to Workshop through the shared
HIR → WIR → Workshop pipeline, with pinned-reference differential evidence,
the declared normalization contract, and the known limitations/divergences

This matrix records the **declared compile surface**: what `wright compile`
accepts for `.ostw`/`.del` inputs and what it rejects, and how the result is
validated against the pinned OSTW v3.4.0 reference. The forward-looking tiered
baseline lives in [`compatibility-baseline.md`](compatibility-baseline.md);
the corrected explicit-root oracle evidence model is documented there too
(#122). The pinned reference identity is recorded in
[`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md).

The owner-side pipeline is `del-rs` (project + syntax + semantic analysis) →
canonical WIR through the narrow `wright-ostw` adapter → canonical
`workshop-rs` emitter (en-US), identical
to the OPY/Workshop paths — no OSTW-specific backend exists.

## Accepted differential targets

`wright compile --root <probe> <probe>/main.ostw` compiles the #122
explicit-root accepted targets (Wright-authored pinned-reference probes under
`compatibility/ostw/probes/`):

| Target | What it exercises | Reference element count |
| --- | --- | --- |
| `p4-types-expressions` | User enums, null/union, `<Number>` cast, ternary, `&&`/`||` (incl. statement-bodied `ping()` operands), player receivers, format strings | 147 |
| `p5-functions-control` | Typed value/void functions, parameter defaults, C-style `for`, `foreach`, `switch` (fallthrough + `break` + `default`), `return` → `Abort` | 120 |
| `p6-catalog-signatures` | Named/default argument binding against canonical Workshop catalog signatures, user-function defaults, enums | 68 |

`crates/wright-ostw/tests/differential.rs` (CI job `ostw-compile-differential`)
compiles each target, asserts the declared round-trip fixed point
(Wright-emitted Workshop reparses and re-emits byte-identically), parses the
pinned reference evidence (`workshop.entry-only.txt`) through the same shared
parser, applies the declared normalization to **both** sides, and requires
structural equality of the rule bodies. A genuine lowering divergence (wrong
argument binding, dropped calls, wrong ternary order) fails the gate; the
machine-readable report lands at `target/wright-ostw-differential-report.json`.

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
  take the catalog's `paramDefaults` (P6) — `allPlayers` → `All Players(All
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
  reference too — #122), classes/generics/lambdas, multi-locale output,
  optimizer/output parity, and original-source recovery in reconstruction
  (#125 is semantic reconstruction, never comments/formatting/macros recovery).
- The `Event Player` restricted-value diagnostic for direct uses in global
  rules remains deferred; the accepted targets use `Event Player` only in
  Ongoing Player rules.

## Workshop → OSTW reconstruction surface (#125)

The reverse direction is owned by `crates/wright-ostw/src/reconstruct.rs`:
`wright_ostw::reconstruct::reconstruct` converts a validated `wir::Program`
whose constructs lie on the declared reconstruction surface into
deterministic canonical OSTW source, and **rejects** everything else with
structured, machine-readable diagnostics and no partial output. The declared
surface, the machine-readable boundary manifest, and the committed fixtures
match exactly (`compatibility/ostw/reconstruction/`):

- **Supported**: variables (`globalvar Any`/`playervar Any`, the permissive
  universal type — the WIR carries no type info and the pinned reference
  requires a type), rules with Global/Each Player events and comparison
  conditions, subroutines (`void name() "…" { … }`), set/modify assignments
  (`=`, `+=`, `-=`, `*=`, `/=`, `%=`, `.append(value)`), `if`/`else if`/
  `else`, `while`, `for (v = start; stop; step)`, `Call Subroutine`, `return`
  (rule-level `Abort`), scalar/array/vector/enum values, global/player
  variable access, `Event Player`, arithmetic (`+ - * /` infix, the real OSTW
  operator forms — the reference rejects callable `Add(...)`), comparison/
  logical/ternary/format-string values, and the catalog actions/values named
  in the manifest (source names reverse the `signature.rs` binding table; the
  catalog is the identity source).
- **Rejected** (never misleading output): `For Player Variable`, Wright's
  `debug`/`print` actions, custom-game `Program.settings`, calls/values/enums
  with no OSTW source binding, `Raise To Power`/`Remove From Array` modify
  operations, non-comparison rule conditions, partial-arity bound calls,
  name collisions, bodiless subroutines, and non-literal format strings
  (`support-boundary.json` lists every kind with its diagnostic code).
- **Not recovered** (non-goals): variable types/indexes, original
  formatting/comments, classes, macros, functions, and project structure.
  Variable-table identity is outside the declared #119 semantic comparison.
- **Reference divergence**: the reconstructed `.append(value)` form (from
  the Workshop Modify-Append-To-Array action) is accepted by Wright's native
  frontend, but the pinned OSTW v3.4.0 reference rejects member-call methods;
  the optional oracle cross-check records exactly this one remaining
  rejection (`target/ostw-reference`, reference-only by contract).

The full loop `Workshop → WIR → OSTW → owner source implementation → WIR →
Workshop` is proven per committed fixture with zero frontend diagnostics, the
declared #119 normalization applied to both sides, structural equality, and
the round-trip fixed point (reconstructed Workshop reparses and re-emits
byte-identically). The suite is `crates/wright-ostw/tests/reconstruct.rs`,
writing `target/wright-ostw-reconstruct-report.json`; the boundary conformance
test checks the manifest, the shipped classification API, and fixture
coverage against each other. A `ds.toml` project root (`entry_point`) is
generated in-test.

### Shared conversion path (#126)

The reconstructor is exposed end-to-end through one shared driver/session
conversion operation: `wright convert --target ostw <workshop-input>` (CLI)
and `CompilerSession::convert(ConvertTarget::Ostw)` (library) load validated
Workshop input through the driver's own `load()` path and call
`wright_ostw::reconstruct::reconstruct` unchanged. The reconstructed source is
the `result.text` of the `wright-result/v1` envelope; a construct outside the
declared surface fails with the reconstructor's stable diagnostics (stage
`reconstruction`, exit code 3) and no partial source. The operation is
Workshop → OSTW only: non-Workshop inputs are rejected explicitly, and there
is no direct OPY ↔ OSTW path. The cross-format suite
(`crates/wright-driver/tests/convert.rs`) proves the full loop
`Workshop → convert(ostw) → native frontend → HIR → WIR → Workshop` for the
`surface-*` fixtures (equivalence under the declared #119 normalization plus
the round-trip fixed point) and the deterministic `reject/` entries, and
writes `target/wright-convert-report.json`.

## Evidence

- `compatibility/ostw/probes/{p4-types-expressions,p5-functions-control,p6-catalog-signatures}/`
  — Wright-authored probe sources, pinned reference identity
  (`probe.json`), recorded reference diagnostics/emission
  (`result.entry-only.json`, `workshop.entry-only.txt`), and the #122
  `differential-target` designation.
- `crates/wright-ostw/tests/differential.rs` — the CI-protected
  forward-compilation differential + round-trip fixed-point gate.
- `compatibility/ostw/reconstruction/` — deterministic reconstruction
  fixtures (`surface-*` positive Workshop sources, `reject/` rejection
  sources) and the machine-readable `support-boundary.json` manifest; the
  #125 reverse-compilation evidence.
- `crates/wright-ostw/tests/reconstruct.rs` — the CI-protected
  reconstruction full-loop gate (`target/wright-ostw-reconstruct-report.json`)
  and the boundary-conformance test.
- `workshop-rs` catalog data (`crates/workshop-rs/src/catalog/data/catalog.json`)
  — canonical catalog with `paramDefaults` (probe-evidenced) and the `abort`
  action, consumed from `workshop-rs`.
- `docs/ostw/compatibility-baseline.md` — the explicit-root evidence model
  and #122 correction.
