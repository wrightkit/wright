# Issue #87 independent QA verification report

related_issue: "#87"
freshness: snapshot
as_of_commit: dbd342e
status: verification
owner: QA

Independent verification of issue #87 (emit `Custom String` form for array
string elements) at commit `dbd342e4…` (unpushed; CI not yet run at write
time). All evidence re-derived from the working tree and the pinned oracle
9.7.10; nothing taken from the Engineer report on trust. Report only — no
code changes.

## AC-1 — minimal repro emits the oracle spelling: **PASS (with a caveat)**

- `globalvar x = ["a", "b"]` + one rule → `wright compile --profile compat`
  emits `Set Global Variable(x, Array(Custom String("a"), Custom
  String("b")));` — byte-identical to the pinned oracle's line.
- The byte-asserted driver test `opy_string_array_initializer_emits_custom_string_elements`
  exists and passes (18/18 driver integration tests green).
- **Caveat — full-artifact normalized identity does not hold for the test's
  own source**: the byte-asserted test uses a `pass`-body rule; the native
  emits an empty rule shell (`rule ("r") { event { Ongoing - Global; } }`)
  while the oracle drops rules with no actions. The array line is identical;
  the artifacts differ by exactly this empty-rule emission (see Divergence C
  below). AC-1's "pinned-oracle artifact identical under the v1 normalizer"
  is satisfied only for the construct under fix, not for the whole artifact
  with a `pass` rule.

## AC-2 — pixelart full-program normalized equality: **NOT MET — residual divergence (reclassified below)**

- Full native compile: exit 0. Settings section: 256/256 equal.
- Full-program comparison (`wright compile -f json` output vs
  `oracle.json compile.workshop`, v1 normalizer = Create-HUD-Text collapse +
  whitespace collapse): **`normalizedEqual: False`** — native 14,365
  normalized chars vs oracle 19,925.
- **Divergence count**: 10,481 differing normalized positions in **279
  distinct regions**; every region lies inside the artwork string-array
  initializers (9 `Set Global Variable` sites: owo, uwu, iwi, ywy, awa,
  ewe, _w_, TwT, lwl). No other construct diverges.
- **First region**: normalized char 460 (the second element of the `owo`
  array, pixelart.opy:36, the `'…▒▒▒…\n[0]:  …'` artwork string). All 279
  regions are the same two-mechanism family.

### Residual divergence A — long-string splitting (strings > 128 chars)

- Native spelling (byte-level, first `[0]:` element):
  `Custom String("…▒▒▒…\n[0]:  …▒▒▒▒…")` — one unsplit Custom String; the
  element content (~150 chars) exceeds the Workshop string limit.
- Oracle spelling (byte-level, same element):
  `Custom String("…▒▒▒…\n[0]:  {0}", Custom String("…"))` — split into
  chained/nested `Custom String` continuations.
- **The oracle's split rule (measured)**: content chunks of exactly 125
  chars per non-final segment, `{0}` appended (128 chars total per Custom
  String text argument), the next segment passed as a nested Custom String
  argument; the final segment holds the remainder (1000-char string → 8
  segments of 128+128+128+128+128+128+128+125; 300-char → 128+128+50).
- **Rationale (pinned OverPy 9.7.10 README, installed package, line 461)**:
  "The Workshop applies a limit of 128 characters and 3 placeholders per
  string. OverPy automatically splits a string if it goes beyond that
  limit." The splitting exists to stay within Workshop's 128-char string
  constraint — the native's unsplit emission would exceed it.
- **Scope**: not array-specific — `globalvar x = "B"×1000` (plain string
  initializer) splits identically in the oracle (8 segments) while the
  native emits one 1017-char Custom String.
- **Classification: class 3** (Wright correctness bug): supported-surface
  construct (string literals with `\n`/`\t` escapes are matrix-listed;
  globalvar initializers preserved), oracle success, real artifact
  divergence with a minimal repro, no documented intent (no Wright doc,
  test, or ADR records this rendering as intentional), and no gate
  coverage (no gate fixture contains a >128-char string). **Severity note**:
  the native output would exceed Workshop's documented 128-char string
  limit for such strings — functionally broken in-game, not merely a
  spelling difference.

### Residual divergence B — `\n` re-escaping in value strings

- Native spelling: the decoded newline is emitted as a **real 0x0A byte**
  inside the Custom String text (`Custom String("a` + newline + `b")`).
- Oracle spelling: the decoded newline is re-escaped to the **literal
  two-character `\n`** (0x5C 0x6E): `Custom String("a\nb")`.
- **Scope**: not array-specific — `globalvar x = "a\nb"` diverges the same
  way. Settings strings are NOT affected (they use the separate
  `escape_settings_string` path fixed in #86; the 7 settings sections all
  still match — see AC-3).
- **Classification: class 3** — same family as the #86 settings-string
  fix, applied only to settings emission; value-position strings remain
  unfixed. Supported surface (escaped strings are matrix-listed), repro
  exists, no documented intent.

### Divergence C — empty (pass-only) rule emission

- `rule "r": @Event global pass` → native emits an empty rule shell; the
  oracle drops rules with no actions entirely.
- Repro-level only: **no corpus fixture uses `pass`** (grep across all
  fixtures: zero matches), so no gate or per-program record is affected.
- **Classification: class 3** per the strict definition (supported surface
  — `pass` is matrix-listed — with a repro), low severity, no corpus
  impact; recorded for completeness of the residual set.

### Scope summary

The residual divergence is **value-string handling generally** (long-string
splitting + newline re-escaping), not array-specific: any >128-char string
and any `\n`-containing string in a value position (globalvar
initializers, array elements, assignments) diverges. The `Custom String`
wrapping itself (the #87 fix) is correct and byte-equal to the oracle
(verified on the minimal repro, the first array element, and the
`[0]:`-element prefix up to the split point).

## AC-3 — previously-passing emission contexts unchanged: **PASS**

- v1-gates: 6/6 (`FIXTURES` untouched; summary `{"passed": 6, "total": 6}`).
- Settings sections (whitespace-collapsed, re-verified at `dbd342e`):
  pixelart 256/256, santa 350/350, broken-weapons 511/511,
  client-to-server 297/297, parabola 136/136, crosshair 144/144,
  inputhud 476/476 — all EQUAL.
- Emitter suite: 17/17; driver integration: 18/18; ws-parser roundtrip and
  the fixed-point test green; wright-workshop full suite green.

## AC-4 — no new class-3 from the matrix / everything else green: **PASS**

- Per-program first-failure matrix **unchanged** (12 programs re-run):
  pixelart exit 0; santa 192:99; broken-weapons 53:55; client-to-server
  55:53; parabola 35:37; crosshair 31:36; inputhud 41:63; cronch 32:21;
  meipocalypse 223:37; zencopter 38:22; ow1 `env.opy:7:66`; 6v6
  `adj_constants.opy:8:85`. (The new class-3 findings of this report are
  emission-level, not first-failure-level; they do not alter the matrix.)
- Oracle 20/20; adapter 22/22; differential green; **PARITY_CASES still 7
  rows, no pixelart row**; all cargo suites green (wright-opy 38+1,
  wright-core, wright-workshop, wright-driver 18, wright-language,
  wright-lsp 22/22); `cargo clippy --all-targets` 0 warnings; `cargo fmt
  --check` clean.
- `git diff 1cd07ab..HEAD`: only `emitter.rs` + driver tests + QA docs
  (nothing else changed).

## AC-5 — inventory: **this document records the residual divergence and
reclassification; a refreshed inventory snapshot is the follow-up**

## Reclassification per AC-2

| Construct | Class | Reasoning |
| --- | --- | --- |
| Long-string splitting (>128 chars in value positions) | **3** | supported surface (strings/arrays/globalvar initializers), oracle success, real artifact divergence + minimal repro, no documented intent, no gate coverage; exceeds the documented Workshop 128-char limit in-game |
| `\n` re-escaping in value strings | **3** | supported surface (escaped strings), oracle re-escapes to literal `\n`, native emits 0x0A; repro exists; same family as the fixed #86 settings-string path |
| Empty (pass-only) rule emission | **3** | supported surface (`pass`), oracle drops empty rules, native emits an empty shell; repro exists; no corpus impact (no fixture uses `pass`) |

None is class 5 (no documented intent anywhere in Wright docs/ADRs/tests)
and none is class 6 (complete evidence with repros).

## Pixelart full-program row verdict

**Not justified at `dbd342e`.** Full compile and settings-section equality
hold, but full-program normalized equality fails (279 regions across the
artwork string arrays). The `Custom String` wrapping portion of the
divergence (the #87 fix) is resolved and verified; the long-string
splitting and `\n` re-escaping remain as class-3 findings. A pixelart
full-program N-level/parity row can only be justified after those are
resolved (or documented as intentional differences).

## Gate-readiness summary

- #87 AC-1, AC-3, AC-4: **pass**; AC-2: **not met** (residual divergence
  recorded and reclassified above).
- Between the current state and the #82 final gate:
  1. Residual divergence A (long-string splitting) and B (`\n` re-escaping
     in value strings) — remediation or documented intentional-difference
     decisions; then the pixelart full-program equality re-run.
  2. Divergence C (empty-rule emission) — low-impact class-3 record; corpus
     impact none.
  3. Refreshed inventory snapshot at the remediation commit (AC-5) with the
     reclassification.
  4. CI run for `dbd342e` (not yet executed at write time) plus the
     re-confirmed suites from this report.

---

## #87 closure record (`b5b0578` + `6d1417b`, verified at `6d1417b`)

Re-verification after the value-string emission fix commit `b5b0578`
(lint-only `6d1417b` on top; CI run 31767535944, all six jobs green, no
skips). All evidence independently re-derived; the residual divergence
record from the previous section is superseded where this section differs.

### AC verdicts at `6d1417b`

- **AC-1 (minimal repro parity): PASS.** `globalvar x = ["a","b"]` +
  one rule → `Array(Custom String("a"), Custom String("b"))`, byte-equal
  to the pinned oracle; byte-asserted driver test green.
- **AC-2 (pixelart full-program equality): PASS.** Normalized equality
  (v1 normalizer) **holds: `normalizedEqual: True`**, 19,925/19,925
  chars; `byteEqual: false` (whitespace-only); native exit 0.
- **AC-3 (contexts unchanged): PASS.** v1-gates 6/6 (`FIXTURES`
  unchanged); all 7 settings sections whitespace-collapsed-equal
  (256/350/511/297/136/144/476); emitter 17/17; driver integration 18/18;
  split and re-escaped spellings round-trip through the ws parser
  (`longstr1000` and `esc` oracle artifacts parse OK).
- **AC-4 (no new class-3 / everything green): PASS with findings.** The
  12-program first-failure matrix is unchanged (all positions
  re-verified). All suites green (oracle 20/20, adapter 22/22,
  differential green, PARITY_CASES 7 with no pixelart row, all cargo
  suites, clippy 0 warnings, fmt clean). **However** the supported-
  surface emission scan surfaced three previously unrecorded class-3
  emission divergences (trailing-if `End;` omission incl. a ws-parser
  asymmetry on the oracle's own spelling; `.format()` constant folding;
  non-default numeric globalvar initializer drop, which also refutes the
  support-matrix's "matching the reference adapter" claim). Recorded in
  `m11-inventory-final.md`; they do not alter the first-failure matrix.
- **AC-5 (inventory): PASS** — `m11-inventory-final.md` records the
  resolved class-3 family, the pixelart N-level row, and the three new
  findings.
- **AC-6..AC-10 (emission contract detail): PASS.** Split basis verified
  (decoded >128 triggers; non-final segments exactly 125 decoded + `{0}`
  = 128 text; final = remainder; 128→1 segment, 129→2 segments (128+4),
  300→3 (128+128+50), 1000→8 (7×[125+{0}]+125)); re-escape matrix
  byte-equal (`\n`/`\r`/`\\`/`"` re-escaped, tab raw 0x09); empty-rule
  drop byte-equal (pass-only and condition-without-actions); artifacts
  end with the oracle's trailing newline; ws lexer decode round-trips the
  emitted spellings.

### Pixelart full-program row at `6d1417b`

Recorded as an **N-level row only** (`target/m11-nlevel.json`:
`nativeExit: 0`, `normalizedEqual: true`); no `PARITY_CASES` row (the
native==adapter HIR subtree assertion for pixelart was not verified and is
not covered by the differential harness — per AC-9 the row stays N-level,
and no parity count is forced).

---

## #87 final-batch closure record (AC-1..AC-14) at `8182959` — dated 2026-08-14

Final batch `c8e3430` (numeric source-spelling preservation) + `f71bc4a`
(non-zero numeric initializers) + `1841452` (trailing-if End omission,
constant-format folding, ws parser acceptance) + `8182959` (corpus count);
CI run 31770753398 — all six jobs success, no skips, headSha `8182959`.
All evidence re-derived; the earlier closure section is superseded where
this section differs.

| AC | Verdict | Evidence (re-derived at 8182959) |
| --- | --- | --- |
| AC-1 (string-array Custom String wrapping) | PASS | repro byte-equal; byte-asserted driver test green |
| AC-2 (pixelart full-program) | PASS | `normalizedEqual: True`, 19,925/19,925; native exit 0 |
| AC-3 (contexts unchanged) | PASS | v1-gates 6/6; 7 settings sections equal (256/350/511/297/136/144/476); emitter/roundtrip suites green |
| AC-4 (matrix unchanged) | PASS | 12-program first-failure matrix unchanged; oracle 21/21; adapter 23/23; differential green (PARITY_CASES 8, no pixelart/real-world settings rows); all cargo suites green; clippy 0; fmt clean |
| AC-5 (inventory) | PASS | this document + `m11-inventory-final.md` |
| AC-6..AC-10 (split/re-escape/empty-rule/ws roundtrip) | PASS | split basis (125+`{0}`, >128 trigger), re-escape matrix (tab raw), empty-rule drop — all byte-equal, roundtripping |
| AC-11 (numeric initializers) | PASS | `j = 5` → `Set Global Variable(j, 5)`; `h = 0` dropped; `k = 0.0` → `Set Global Variable(k, 0.0)` (source spelling); `playervar p = 7` → separate player-initialize rule; artifact byte-equal; bare-index form still works; support-matrix claim corrected |
| AC-12 (trailing-if) | PASS | repros a–d byte-equal; oracle's trailing-If spelling accepted by the native ws parser; roundtrip fixed-point green |
| AC-13 (format folding) | PASS with residual | constant folding (single/multi-arg, `0.50`/`0.13` toFixed(2) spelling) byte-equal; variable-arg residual below |
| AC-14 (no new class-3 / final scan) | FAIL | the final scan surfaced the playervar-read divergence (below) |

### Residuals and findings at `8182959`

1. **Variable-arg format placeholder `{}` vs `{0}` (class 3, emission-only,
   corpus-unexercised)** — native `Custom String("v: {}", Global.x)` vs
   oracle `Custom String("v: {0}", Global.x)`; HIR carries `{}` in both
   producers; every corpus `.format` site is HUD-collapsed or in a
   non-compiling program. Low-to-moderate severity (client behavior on
   bare `{}` unverifiable).
2. **Playervar member reads in value positions (class 3, new from the
   final scan)** — `g = eventPlayer.p` → native `Event Player.p` vs oracle
   `(Event Player).p`; **both spellings fail the native ws parser**, so the
   native's own emission does not round-trip for this construct. Zero
   corpus coverage (no fixture reads a playervar). Pre-existing emission
   behavior, surfaced by the final scan.

### Pixelart row

N-level row only (`target/m11-nlevel.json`: `nativeExit: 0`,
`normalizedEqual: true`); no `PARITY_CASES` row; the native==adapter HIR
subtree assertion for pixelart remains unverified (per AC-9 the row stays
N-level; no parity count forced).

---

## #87 AC-15..AC-17 closure record at `4c6a490` — dated 2026-08-14

Final batch `65b7fb6` (placeholder numbering, playervar-read spelling) +
`4c6a490` (matrix-surface closure scan); CI run 31772287928, all six jobs
success, no skips, headSha `4c6a490`. Supersedes the residual items in the
previous closure section (both closed).

| AC | Verdict | Evidence |
| --- | --- | --- |
| AC-15 (placeholder numbering) | PASS | all pinned shapes byte-equal (implicit renumbering, multi-arg folding, fold-interplay `3 {0}`, explicit-only verbatim, explicit constant folding); mixed implicit+explicit → oracle error, native keeps text unchanged and round-trips; HIR untouched (expressions-values fixture `points: {}`, differential green) |
| AC-16 (playervar reads) | PASS | `(Event Player).p` byte-equal in assignment/condition/binary shapes; ws parser accepts the oracle spelling; fixed-point roundtrip byte-identical; SET form and method-call receivers unchanged |
| AC-17 (closure scan + probes) | PASS with one new class-3 | closure test green (17 families); enum/define/indexed-read probes byte-equal; indexed-write explicitly rejected (lower-error); `eventPlayer.getCurrentHero()` — outside-surface (catalog-data gap, frontend `unknown-value`, ws parser rejects the text spelling; NOT class 3 — the emitter cannot produce it); **new class-3: augmented playervar assignment** (`eventPlayer.p += 1` → native `Set Player Variable(…, Add((Event Player).p, 1))` vs oracle `Modify Player Variable(…, Add, 1)`), supported surface, semantically equivalent, both spellings roundtrip, zero corpus coverage |

Final gate standing: pixelart N-level row (`normalizedEqual: true`,
19,925/19,925), v1-gates 6/6, PARITY_CASES 8, oracle 21/21, adapter 23/23,
12-program matrix unchanged, all suites green. The only open class-3 item
is the augmented-playervar spelling (emission-only, low severity,
corpus-unexercised).
