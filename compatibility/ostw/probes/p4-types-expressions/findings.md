# P4 findings — user enum, null/union, cast, ternary, short-circuit

Reference: OSTW v3.4.0 (identity in `probe.json`). Evidence:
`result.entry-only.json`, `workshop.entry-only.txt`.

## Observed facts

- `favorite = Fruit.Banana;` (enum Fruit { Apple, Banana, Cherry }) →
  `Set Global Variable(favorite, 1);` — the member is a raw integer.
- `combo = null;` (`Number | Team` union) → `Set Global Variable(combo, Null);`
  and `if (combo == null)` → `If(Compare(combo, ==, Null));`.
- `combo = Team.Team2;` → `Set Global Variable(combo, Team 2);` and
  `if (combo == Team.Team1)` → `If(Compare(combo, ==, Team 1));` — enum values
  stored raw in the Number cell.
- `selector = <Number>combo;` → `Set Global Variable(selector, Global Variable(combo));`
  — the cast disappears in emission.
- `selector = (selector > 0) ? 10 : 20;` →
  `Set Global Variable(selector, If-Then-Else(Compare(selector, >, 0), 10, 20));`
  (same shape for the typed declaration `Number pick: …`).
- Pure conditions: `&&` → `If(And(Compare…, Compare…))`, `||` → `If(Or(…))`.
- `if (ping() && ping())` where `Boolean ping() { counter += 1; return true; }` →
  `Modify Global Variable(counter, Add, 1); Modify Global Variable(counter, Add, 1); If(True);`
  — both side effects hoisted BEFORE the If, unconditionally; condition folded.
- `selector = ping() ? 10 : 20;` → `Modify(counter, Add, 1); Set(selector, If-Then-Else(True, 10, 20));`.
- `if (ping() && selector > 0)` →
  `Modify(counter, Add, 1); If(And(True, Compare(selector, >, 0)));`.
- `if (selector == 0 || ping())` →
  `Modify(counter, Add, 1); If(True);` — the Or's second operand side effect runs
  unconditionally BEFORE the If; no gating on `selector == 0`.
- accept, elementCount 147, 0 diagnostics, workshopCode SHA-256 `f6eb515e…f69f`.

## Decision support (interpretation)

- User enum members are implicit 0-based integers (Apple=0, Banana=1, Cherry=2),
  stored raw; they are not Workshop enum elements. #118 must not encode user
  enums as catalog enums (matches the inventory's explicit requirement).
- `null` is the Workshop `Null` value; a `Number | Team` union is a single Number
  cell that can hold a number, a Team enum value, or Null; comparisons emit
  `Compare(…, ==, Null)`.
- `<Number>` cast is a type-check-time construct; emission is a value
  pass-through for unions.
- Ternary lowers to the Workshop `If-Then-Else` VALUE — no short-circuit is
  possible inside it.
- `&&`/`||` lower to Workshop `And`/`Or` conditions. Side-effecting operands are
  hoisted to actions BEFORE the If/Set, UNCONDITIONALLY: the reference does not
  preserve short-circuit evaluation order for side effects (the `||` case is
  decisive: the second operand's action runs even when the first operand is
  true). #118 must not assume C#-like short-circuit or coercion semantics.
- Assignment is NOT an expression in the reference grammar (see p4b): the
  hoisting above is the only vehicle for operand side effects.

## Caveats

- The hoisted conditions folded to constants because `ping()` returns a literal
  `true`; the STRUCTURE (hoist-before-If, no nesting) is the evidence for
  non-short-circuit lowering, not a runtime measurement.
- `If-Then-Else`, `And`, `Or` are emitted as values/conditions; their runtime
  evaluation semantics are Workshop's, not the reference's.
