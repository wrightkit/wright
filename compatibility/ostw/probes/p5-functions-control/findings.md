# P5 findings — typed functions, parameter defaults, for/foreach/switch/return

Reference: OSTW v3.4.0 (identity in `probe.json`). Evidence:
`result.entry-only.json`, `workshop.entry-only.txt`.

## Observed facts

- Variable table: `global: 0: counter`; `player: 0: by, 1: by_0, 2: by_1, 3: by_2,
  4: k, 5: by_3, 6: by_4, 7: by_5, 8: by_6` — one player variable per void-function
  CALL (8 calls: bump×4, bumpDefault×2, loop bump, plus 4 switch bumps →
  by..by_6), plus the foreach local `k`.
- `counter = twice(3);` (expression-bodied `Number twice(Number v): v * 2;`) →
  `Set Global Variable(counter, 6);` — constant-folded, NO parameter storage.
  `counter = twice(counter);` → `Set Global Variable(counter, Multiply(counter, 2));`
  — inlined as a value expression.
- Void calls: `bump(4)` → `Set Player Variable(Event Player, by, 4);
  Modify Global Variable(counter, Add, Player Variable(Event Player, by));`
  `bumpDefault()` → `by_0 = 5` then Add (default 5 materialized at the call site);
  `bumpDefault(1)` → `by_1 = 1` then Add.
- `if (counter == 0) { return; }` → `If(Compare(counter, ==, 0)); Abort; End;`.
- C-style `for (counter = 0; counter < 5; 1)` →
  `For Global Variable(counter, 0, Compare(counter, <, 5), 1);` with body
  `by_2 = 1; Modify…; Wait(0.1, Ignore Condition); End;`.
- `foreach (Number k in [1, 2, 3])` →
  `For Player Variable(Event Player, k, 0, Count Of(Array(1, 2, 3)), 1);`
  body `Modify(counter, Add, Value In Array(Array(1, 2, 3), k)); End;`.
- `switch (counter) { case 1: bump(10); case 2: bump(20); break;
  case 3: bump(30); default: bump(40); }` →
  `Skip(Value In Array(Array(7, 0, 2, 5), Add(Index Of Array Value(Array(1, 2, 3), counter), 1)));`
  then the case bodies in source order with `Skip(4)` after the case-2 body
  (break). Entry skips: case1=0, case2=2, case3=5, not-matched=7.
- accept, elementCount 120, 0 diagnostics, workshopCode SHA-256 `b687e939…30f`.

## Decision support (interpretation)

- Expression-bodied value functions inline as pure VALUE expressions (parameters
  substituted; constant-foldable). Void (action) functions inline as ACTION
  sequences at each call site, with every parameter materialized as a per-call
  PLAYER variable named after the parameter and suffixed `_0`, `_1`, … on reuse.
  Function evaluation order is call-site order; there are no runtime call
  elements (no subroutines here).
- Default parameter values are resolved at the CALL SITE (a constant `Set`), so
  the binding of defaults is a compile-time, per-call decision.
- `return` inside a rule body = `Abort` of the rule (early exit).
- C-style `for` over a GLOBAL loop variable lowers to Workshop
  `For Global Variable`; `foreach` lowers to an index `For Player Variable` over
  `Count Of(array)` with `Value In Array` element access — the local binding is a
  player variable.
- `switch` lowers to a Skip-array dispatch: sequential case bodies in source
  order (fallthrough = no skip between consecutive bodies), `break` = `Skip`
  over the remaining bodies, `default` = the last body; a non-matching value
  skips all case bodies. Element indexing encodes entry points.

## Caveats

- Structural/output evidence; runtime behavior (e.g. `For Global Variable`
  semantics) belongs to Workshop, not the reference.
- `twice(3)` folding is an optimizer artifact; the inlining shape is the
  evidence, not the folded constant.
