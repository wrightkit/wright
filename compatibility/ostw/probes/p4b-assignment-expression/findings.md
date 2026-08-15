# P4b findings — assignment in expression position

Reference: OSTW v3.4.0 (identity in `probe.json`). Evidence:
`result.entry-only.json`, `workshop.entry-only.txt`.

## Observed facts

- `if ((selector += 1) > 0 && (selector += 1) > 0)` → reject; parse diagnostics
  on `main.ostw` line 5 starting with `) expected` at 18–20 (the `+=`), then
  `; expected`, `Unexpected token '>'`… cascades; final `Expected a variable.`.
- `if ((selector = 2) == 2)` → also rejected (cascade `Expected a hook variable.`
  at line 8–9, `Unexpected token 'if'`, …).
- All 40 diagnostics severity 1; result reject, elementCount -1.

## Decision support (interpretation)

- Compound AND plain assignment are STATEMENTS, not expressions, in the pinned
  reference grammar: `(selector += 1)` and `(selector = 2)` in condition
  position are parse errors. #118's expression surface must not include
  assignment expressions (the inventory's "assignment and compound assignment"
  are statement-level only).

## Caveats

- The parser recovers after the first failure and reports a cascade; the first
  diagnostic on each construct is the reliable evidence.
