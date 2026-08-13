# M11 inventory snapshot after the settings batch (#86)

related_issue: "#82"
freshness: snapshot
as_of_commit: 1cd07ab
status: verification
owner: QA

**This snapshot supersedes [`m11-inventory-rebaseline.md`](m11-inventory-rebaseline.md)
where the two differ** (next-blocker records, the settings surface, the
parity baseline, and the class-3 finding below). It records the post-#86
corpus state: settings are now supported native surface, three candidates
were acquired, one bounded parity row was added, and the first class-3
finding of M11 was identified. Evidence: independently re-derived at
`1cd07ab` (CI run 31747128723, six jobs green); see
[`m11-issue86-verification.md`](m11-issue86-verification.md) for the
per-AC verification record. Implementation proposals and prioritization are
PM-owned and absent here.

## Per-program first-failure records (QA-verified positions at `1cd07ab`)

All nine settings-bearing programs now parse their settings block; settings
is never the first failure. Positions column-verified against source.

| Program | Native status | First failure after settings | Class |
| --- | --- | --- | --- |
| overpy-pixelart | **exit 0, full compile** | none | settings-section N-equal (256/256); full-program row **not** added — class-3 emission divergence (see below) |
| overpy-santa | exit 1 | `parse-error` 192:99 — named argument `rate=` (`chase(..., rate=SPEED, ...)`) | 2 (unsupported surface) |
| overpy-broken-weapons | exit 1 | `parse-error` 53:55 — range literal `float[0.5:10]` in `createWorkshopSetting` | 2 (unsupported surface) |
| overpy-client-to-server | exit 1 | `parse-error` 55:53 — chained inline conditional (ternary) | 2 (unsupported surface) |
| overpy-parabola | exit 1 | `parse-error` 35:37 — numeric member `Team.2` in `createDummy(Hero.TORBJORN, Team.2, ...)` | 2 (unsupported surface); settings section 136/136 |
| overpy-crosshair | exit 1 | `parse-error` 31:36 — bytes literal `b" \n"` | 2 (unsupported surface); settings section 144/144 |
| overpy-inputhud | exit 1 | `parse-error` 41:63 — inline conditional `"..." if any([...` | 2 (unsupported surface); settings section 476/476 |
| skirmish_elim | exit 1 | `unsupported-directive` 48:1 — `#!obfuscate` | 4 (reference-oracle limitation), S divergence documented (native accepts settings; oracle rejects the program) |
| lucioball_all_heroes | exit 1 | `lex-error` 66:1 — unterminated (multiline) string literal | 4 (reference-oracle limitation), S divergence documented (oracle rejects at the lucioball gamemode validation 39:34) |

Corrected vs the rebaseline (`8b782ad`) predictions: broken-weapons
(`not in` :107 → range literal **53:55**), santa (comprehension macro :213 →
named arg **192:99**), parabola (conditional :46 → `Team.2` **35:37**),
inputhud (comprehension :83 → conditional **41:63**), lucioball (no native
record → multiline string **66:1**). The `not in` at broken-weapons:107 and
the comprehension at inputhud:83 still exist and fail independently, but
are masked by earlier failures. The non-settings fixtures are unchanged:
cronch `++` 32:21, meipocalypse dict 223:37, zencopter `"""` 38:22, ow1
`\` 7:66 (env.opy), 6v6 `\` 8:85 (adj_constants.opy); zombies remains
metadata-only (class 6).

## New fixtures and parity

- **Acquisitions**: overpy-parabola, overpy-crosshair, overpy-inputhud
  committed via `scripts/acquire-corpus.py` (pinned `eea67ad`, GPL-3.0-only;
  sha256 `2dce50c3…` / `d2c22b59…` / `ae7a0e00…` match the candidates
  record; oracle.json snapshots added). Settings class: supported-parity at
  the settings boundary (section equality 136/136, 144/144, 476/476); the
  program bodies are partially classified with their next blockers above.
- **Bounded parity**: `synthetic/settings` added to `PARITY_CASES`
  (native-vs-adapter HIR parity on settings + supported syntax only) —
  `PARITY_CASES` now **7 rows** (5 synthetic + synthetic/settings +
  real-world/overpy-cake). No real-world settings fixture is in
  `PARITY_CASES`.
- skirmish_elim / lucioball: **not acquired** as parity candidates;
  recorded S divergences.

## Settings surface — now SUPPORTED (corpus-evidenced)

- The 7 oracle-success settings programs (pixelart, santa, broken-weapons,
  client-to-server, parabola, crosshair, inputhud) emit settings sections
  equal to their pinned-oracle snapshots (whitespace-collapsed: 256/350/511/
  297/136/144/476 chars).
- Emission table (`crates/wright-ir/src/settings/table.rs`): **34 exact-path
  entries** — main{description, modeName}, lobby{ffaSlots},
  gamemodes.<mode>{enabled, enabledMaps, roleLimit,
  enableCompetitiveRules} on the evidenced per-mode subsets, gamemodes.
  general{heroLimit, respawnTime%, enableHeroSwitching,
  enableRandomHeroes}, heroes.<team>{enabledHeroes, disabledHeroes},
  heroes.<team>.mei{enablePrimaryFire, enableSecondaryFire, enableAbility1,
  enableAbility2, health%, passiveUltGen%, combatUltGen%}; name maps (7
  mode, 2 map, 10 hero, 1 team); enum domains `roleLimit`
  {2OfEachRolePerTeam} and `heroLimit` {off} (the un-evidenced
  skirmish_elim `roleLimit: "off"` member is rejected, provenance corrected).
- Rejection surface (`settings-*` codes, source-located, identical for
  check and compile): `settings-unknown-key` (keys and (mode, key) pairs
  outside the exact-path table, unknown gamemodes), `settings-unknown-value`
  (enum/list members outside domains), `settings-placement` (settings in
  included files, second blocks, non-first construct), `settings-invalid`
  (the `settings "file"` form, unterminated blocks).
- Settings string values round-trip the oracle spelling: decoded `\n`/`\t`/
  `\r`/`\\`/`"` re-escape at emission (inputhud description verified
  end-to-end, 476/476).
- Reference policy: ADR-0007 (`9e1408b`) — pinned oracle 9.7.10 sole
  primary reference, content-pinned (`889d974`), changed only on
  demonstrated behavioral need.

## `in`-operator record (corrected)

`for ... in range(...)` headers are supported; expression-level `in`/`not
in` membership operators are **not** supported (native `parse-error`, first
corpus evidence broken-weapons `not in` at :107, masked by the range-literal
failure at 53:55) and remain deferred. The support-matrix operator list no
longer claims `in` as an expression operator.

## Class-3 finding — pixelart full-program emission divergence (the first of M11)

**Classification: class 3 (Wright correctness bug)** — oracle success +
native divergence on a supported-surface construct, with a minimal repro.

- Construct: a string-literal array as a `globalvar` initializer
  (`globalvar owo = [' ', '▒▒▒…', ...]`, pixelart.opy:35), emitted as a
  `Set Global Variable(owo, Array(...))` action. Inside the declared
  supported surface: arrays `[...]`, double-quoted strings, and
  `globalvar name = expr` with array initializers are all matrix-listed;
  the native frontend compiles it with exit 0.
- Divergence: the oracle renders string elements wrapped
  `Array(Custom String("."), Custom String("▒▒…"))`; the native emitter
  renders bare `Array(".", "▒▒…")`. The v1 normalizer does not collapse
  this, so full-program normalized equality fails at char 411
  (`scripts/v1-gates.py` normalizer; machine report `target/m11-nlevel.json`
  records `normalizedEqual: False` for pixelart).
- Repro: `globalvar x = ["a", "b"]` + one rule — native emits
  `Array("a", "b")`; oracle emits `Array(Custom String("a"), Custom
  String("b"))`. Both are accepted by wright's own ws parser (the native
  spelling is self-consistent), but the pinned reference's artifact is the
  N-level contract, and the divergence is real at the artifact level.
- Why the 6 v1-gates fixtures pass: no gate fixture contains a string-array
  initializer (`expressions-values` has `[1, 2, 3]` only; cake has none) —
  the construct is a coverage gap, not a normalizer artifact.
- Why not class 5: no document records this rendering as an intentional
  difference (emitter/support-matrix/ADRs are silent on it).
- Gate impact: this is the first class-3 finding of M11. The pixelart
  full-program N-level/parity row is **not justified** until the emitter
  renders string array elements like the reference (or a documented
  intentional-difference decision exists); the settings-section evidence
  (256/256) stands. AC-8 was respected — no row was forced.

## Architect concerns and remediation (verified)

1. **String escaping** (`1cd07ab`): re-escape of every JSONC decode;
   inputhud AC-2 verified end-to-end (476/476, literal `\n` bytes 0x5C
   0x6E).
2. **Mode subsets** (`1cd07ab`): exact-path entries; un-evidenced
   (mode, key) pairs fail `settings-unknown-key` with spans (verified:
   `gamemodes.assault.heroLimit`, `gamemodes.general.roleLimit`), the 7
   evidence programs still validate with matching sections.
3. **Provenance** (`1cd07ab`): un-evidenced `roleLimit: "off"` rejected
   `settings-unknown-value`; the table no longer claims skirmish_elim
   evidence.
4. **Open from the batch**: the class-3 emission divergence above (not a
   #86 acceptance failure; AC-8 held).

## Parity baseline statement

- v1-gates: **6 rows unchanged** (`FIXTURES` untouched in the batch; gates
  6/6 at `1cd07ab`).
- `PARITY_CASES`: **7 rows** (+1 bounded `synthetic/settings`); no
  real-world settings fixture added.
- pixelart full-program row: **not added** — class-3 divergence above;
  settings-section claim stands. Honest statement: **no parity count was
  forced**; the post-batch baseline contingent on #86 verification
  resolves to: pixelart settings-section parity + bounded synthetic parity
  + 3 acquired settings sections; the four previously named full candidates
  reduce to pixelart for full-program status, which is blocked by the
  class-3 emission gap.

## What remains for the #82 final gate

1. The **class-3 emission divergence** (string elements in `Array(...)`
   actions) — remediation decision (emitter fix or documented intentional
   difference + normalizer), then pixelart full-program verification.
2. **AC-2**: verified at `1cd07ab` (inputhud resolved) — no open item.
3. **ADR-0007**: committed (`9e1408b`), re-checked.
4. **Inventory refresh**: this snapshot (next-blocker corrections recorded).
5. **Re-confirm no other class 3** and all suites green at the remediation
   commit — suites re-run at `1cd07ab` (oracle 20/20, adapter 22/22,
   differential 7 rows, v1-gates 6/6, all cargo suites green).
6. **Next-blocker records** in `m11-inventory-rebaseline.md` (parabola,
   inputhud, lucioball) are corrected here; the rebaseline doc is
   superseded where the two differ.
