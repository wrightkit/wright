# M11 final-gate inventory refresh

related_issue: "#82"
freshness: snapshot
as_of_commit: 6d1417b
status: verification
owner: QA

**This snapshot supersedes [`m11-inventory-post86.md`](m11-inventory-post86.md)
where the two differ.** It records the state at `6d1417b` (CI run
31767535944, all six jobs success, headSha `6d1417b`; batch `b5b0578` +
lint-only `6d1417b` on top of the #87 fix `dbd342e`) and the pre-gate
verification results. The pixelart full-program class-3 family is resolved;
the pre-gate scan surfaced three further emission divergences (recorded
below) that the #82 final gate must account for. Implementation proposals
and prioritization are PM-owned and absent here.

## Resolved class-3 family (#87 + `b5b0578`, QA-verified at `6d1417b`)

| Divergence (class 3) | Native behavior | Oracle behavior | Status |
| --- | --- | --- | --- |
| `Custom String` wrapping in value positions (`Array` elements etc.) | bare strings in `Array(...)` | `Array(Custom String("a"), Custom String("b"))` | **fixed** (`dbd342e`); minimal repro byte-equal, byte-asserted driver test green |
| Long-string splitting (>128 decoded chars) | one unsplit `Custom String` | chained continuations: 125 decoded chars + `{0}` = 128 text per non-final segment, nested right, final = remainder | **fixed** (`b5b0578`); 300-char (3 segments) and 1000-char (8 segments: 7×[125+{0}] + 125) repros **byte-equal** to the pinned oracle; threshold verified: 128 decoded → 1 segment, 129 → 2 segments (128+4) |
| Value-string re-escaping | decoded newlines emitted as 0x0A | `\n` re-escaped to literal 0x5C 0x6E | **fixed** (`b5b0578`); `"a\nb\tc\rd\\e\"f"` repro **byte-equal**; tabs pass raw (0x09), `\n`/`\r`/`\\`/`"` re-escaped, per the oracle's byte spelling |
| Empty-rule emission (`pass`-only / condition-without-actions) | empty rule shells emitted | rules with no actions dropped | **fixed** (`b5b0578`); repro byte-equal (the only residual byte diff in the repro is the `debug()` HUD line — the documented M8 deferred item) |
| Trailing blank line | — | oracle artifact ends with one trailing newline | fixed; byte-equal |

**Pixelart full-program N-level row** (`target/m11-nlevel.json` at
`6d1417b`): `nativeExit: 0`, **`normalizedEqual: true`** — native 19,925
normalized chars == oracle 19,925; `byteEqual: false` (whitespace-only).
This is an **N-level row only**: no `PARITY_CASES` row was added (still 7
rows; a pixelart parity row would require a native==adapter HIR subtree
assertion, which the differential harness does not cover — QA did not
verify HIR subtree equality for pixelart; per AC-9 the row stays N-level).

## New findings — pre-gate supported-surface emission scan

The supported-surface sanity scan (string compare, format call, numeric
initializer, trailing-if) against the pinned oracle surfaced **three
previously unrecorded emission divergences**, all on matrix-listed
constructs, all with repros, none documented as intentional, none covered
by the gate fixtures:

1. **Trailing-if `End;` omission (class 3).** The oracle omits the closing
   `End;` of an `if`/`if-else` that is the final statement of a rule;
   the native emits it. Repro: rule ending with `if …:` (with or without
   `else`) → native `…End;`, oracle omits. Middle-of-rule ifs and
   trailing `while` blocks keep `End` in both (byte-equal). No gate
   fixture ends a rule with an if/else (control-flow's ifs are inside a
   for body), so the gates never exercise it. **Related roundtrip
   asymmetry**: the native ws parser rejects the oracle's own trailing-If
   spelling (`malformed: 'If' requires a matching 'End'`), i.e. oracle-
   valid Workshop text fails the `.ws` input path.
2. **`.format()` constant folding (class 3, low severity).**
   `"value: {0}".format(3)` with constant args is folded by the oracle
   into `Custom String("value: 3")`; the native emits the format node
   `Custom String("value: {0}", 3)`. Semantically equivalent (both render
   "value: 3"); N-level divergence on the supported format surface. With
   a variable argument both sides emit the format node (byte-equal).
3. **Non-default numeric globalvar initializers dropped (class 3, semantic
   impact).** `globalvar j = 5` (non-zero number): the native drops the
   initializer from HIR and emission (documented in the support-matrix as
   "literal-number `=` initializers are dropped from HIR, matching the
   reference adapter"); the pinned oracle emits
   `Set Global Variable(j, 5)` in the Initialize rule, and the **live
   adapter HIR carries the initializer** (`{"kind":"number","value":5}`).
   The adapter drops only integer-0 initializers (`globalvar h = 0` →
   no rule; `j = 5` → rule; `k = 0.0` → rule). The matrix's
   "matching the reference adapter" claim is refuted for non-zero values;
   the native artifact leaves such variables at the Workshop default (0)
   — a real in-game semantic difference. A PARITY fixture with a non-zero
   numeric initializer would fail the differential (HIR divergence).

These are recorded as open class-3 findings for the final gate; none
changes the per-program first-failure matrix (all are emission-level on
programs that already compile or fail earlier).

## Corrected per-program first-failure matrix (re-verified at `6d1417b`)

| Program | Native status | First failure / status |
| --- | --- | --- |
| overpy-pixelart | **exit 0** | full compile; settings section 256/256; full-program N-level (normalizedEqual true) |
| overpy-santa | exit 1 | `parse-error` 192:99 (named argument `rate=`) |
| overpy-broken-weapons | exit 1 | `parse-error` 53:55 (range literal `float[0.5:10]`) |
| overpy-client-to-server | exit 1 | `parse-error` 55:53 (inline conditional) |
| overpy-parabola | exit 1 | `parse-error` 35:37 (`Team.2` numeric member) |
| overpy-crosshair | exit 1 | `parse-error` 31:36 (bytes literal) |
| overpy-inputhud | exit 1 | `parse-error` 41:63 (inline conditional) |
| overpy-cronch | exit 1 | `parse-error` 32:21 (`++`) |
| overpy-meipocalypse | exit 1 | `lex-error` 223:37 (dict literal) |
| overpy-zencopter | exit 1 | `lex-error` 38:22 (`"""`) |
| ow1-emulator | exit 1 | `lex-error` `env.opy:7:66` (`\`) |
| 6v6-adjustments | exit 1 | `lex-error` `adj_constants.opy:8:85` (`\`) |
| skirmish_elim / lucioball_all_heroes | exit 1 | `#!obfuscate` 48:1 / multiline string 66:1 — documented S divergences (oracle rejects on `#!obfuscate` / OW2 lucioball gamemode validation) |
| zombies | — | class 6, metadata-only, unchanged |

## Settings surface (supported, corpus-evidenced)

- 7 oracle-success settings sections whitespace-collapsed-equal at
  `6d1417b`: pixelart 256, santa 350, broken-weapons 511, client-to-server
  297, parabola 136, crosshair 144, inputhud 476.
- Emission table: 34 exact-path entries (main/lobby/gamemodes per-mode
  subsets/gamemodes.general/heroes per-hero subsets) + name maps (7 mode,
  2 map, 10 hero, 1 team); enum domains `roleLimit`
  {2OfEachRolePerTeam}, `heroLimit` {off}; value-string re-escaping and
  long-string splitting apply to settings strings too (shared emission
  contract).
- Rejection surface: `settings-unknown-key` (keys, (mode,key) pairs,
  gamemodes), `settings-unknown-value` (enum/list members),
  `settings-placement` (includes, second blocks, placement),
  `settings-invalid` (`settings "file"` form, unterminated blocks) —
  identical codes and spans for check and compile.

## Parity baseline

- v1-gates: 6/6, `FIXTURES` unchanged.
- `PARITY_CASES`: 7 rows (5 synthetic + `synthetic/settings` +
  real-world/overpy-cake); no pixelart row, no other real-world settings
  row; HIR parity proven only on the bounded synthetic fixture.
- Pixelart: full-program **N-level row only** (normalizedEqual true),
  per AC-9; no parity row was forced.

## Deferred-syntax list with evidence (post-M11 candidates, unchanged)

Named arguments (santa 192:99), range literals (broken-weapons 53:55),
`Team.2` numeric members (parabola 35:37), inline conditionals
(client-to-server 55:53, inputhud 41:63), comprehensions (masked in
santa/inputhud), bytes literals (crosshair 31:36), multiline strings
(lucioball 66:1), expression-level `in`/`not in` (broken-weapons 107,
masked), `\` line continuation (ow1 7:66, 6v6 8:85), `++`/`--`
(cronch 32:21; `--` at meipocalypse 221/251 + barricades 196, masked),
dict literals (meipocalypse 223:37), `"""` docstrings (zencopter 38:22),
`#!obfuscate` (skirmish_elim 48:1), settings-in-includes and multiple
blocks (rejected `settings-placement`), `.ws` settings input (rejected),
OW2 gamemode validation (lucioball — oracle divergence, not replicated),
and the settings-unknown-key surface as corpus-boundary growth
(out-of-table keys grow the table only with fixture-evidenced snapshots).
The `for ... in` header remains the only supported `in` form.

## What the #82 final gate must confirm

1. **The three new class-3 emission findings** (trailing-if `End;`
   omission + ws-parser asymmetry, `.format()` constant folding,
   non-default numeric initializer drop) — remediation decision or
   documented intentional difference, then re-run of the affected
   supported-surface checks.
2. Re-confirmed pixelart full-program N-level row (normalizedEqual true,
   recorded in `target/m11-nlevel.json`) and the settings-section rows.
3. The support-matrix claim "literal-number initializers are dropped …
   matching the reference adapter" needs correction or a documented
   boundary (the live adapter carries non-zero numbers).
4. ADR-0007 filed (committed `9e1408b`) — re-checked.
5. All suites green at the gate commit (re-verified at `6d1417b`: v1-gates
   6/6, oracle 20/20, adapter 22/22, differential green with 7
   PARITY_CASES rows, all cargo suites green, clippy 0 warnings, fmt
   clean, CI 6/6).
6. No parity count was forced at any point (AC-8/AC-9 respected).
