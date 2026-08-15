# P3a findings — automatic globals around an explicit ID

Reference: OSTW v3.4.0 (identity in `probe.json`). Evidence:
`result.entry-only.json`, `workshop.entry-only.txt`.

## Observed facts

- Declarations (in source order): `a` auto, `b` auto, `i 127` explicit,
  `c` auto, `d 128` explicit, `e` auto.
- Emitted variable table (exact):
  `global: 0: a, 1: b, 127: i, 2: c, 128: d, 3: e`.
- 0 diagnostics; accept, elementCount 30, workshopCode SHA-256 `dd2e3542…4c1`.
- `Initial Global` sets ONLY the initializer-bearing variables (a=1, b=2, c=3,
  e=4); `i` (127) and `d` (128) have no initializer and are not set.
- `i += a + b + c + d + e` emits
  `Modify Global Variable(i, Add, Add(Add(Add(Add(a, b), c), d), e))`.

## Decision support (interpretation)

- Automatic allocation fills the LOWEST FREE global index, skipping every
  explicit ID: after a=0, b=1, the explicit 127 is reserved, so the next auto is
  2, then 128 is explicit, and the last auto is 3. No collision diagnostics occur.
- Explicit IDs are reserved for both auto allocation and duplicate checks
  (see p3b), and the table lists variables in declaration order with their real
  indices. `i 127` lands exactly on 127.
- #118 allocation policy: lowest-free auto allocation with explicit-ID
  reservation, validated before WIR emission.
- Initialization: only variables with an initializer appear in the synthesized
  `Initial Global` rule.

## Caveats

- Variable indices and table order are structural (N-level) evidence; the
  allocation POLICY is inferred from the exact indices chosen.
