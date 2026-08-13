# Issue #86 independent QA verification report

related_issue: "#86"
freshness: snapshot
as_of_commit: 6ea29ec
status: verification
owner: QA

Independent verification of issue #86 (typed custom-game-settings support in
the native frontend) against its corrected acceptance criteria. All evidence
below was re-derived at `6ea29ec` from the working tree (clean), the pinned
oracle 9.7.10, and the GitHub API; nothing was taken from the Engineer
report without re-derivation. Head of record: commit `6ea29ec` (batch
`544aa16..6ea29ec`, 12 commits), CI run 31745141113 (headSha `6ea29ec`).
This is a report only — no implementation proposals, no code/fixture/doc
changes.

## A. CI

Run 31745141113: all six jobs **success**, none skipped: v1 release gates
(28s), Rust quality 1.85.0 (54s), native-vs-reference differential (20s),
OverPy compatibility oracle (25s), Rust quality stable (41s), OverPy-to-HIR
adapter (14s). Run headSha `6ea29ec2f2dbef98e853ae45df8b7699b996b293` —
matches HEAD. Two Node-20 deprecation annotations only (no failures).

## B. AC-1 — settings parses; settings never the first failure: **PASS**

All nine settings-bearing programs run natively
(`wright compile <main> --root <dir> --profile compat -f json`):

| Program | exit | First failure (code, line:col, construct) |
| --- | --- | --- |
| overpy-pixelart | **0** | none — full compile |
| overpy-santa | 1 | `parse-error` 192:99–100 — named argument `rate=` (`chase(..., rate=SPEED, ...)`) |
| overpy-broken-weapons | 1 | `parse-error` 53:55–56 — range syntax `float[0.5:10]` in `createWorkshopSetting` |
| overpy-client-to-server | 1 | `parse-error` 55:53–55 — chained inline conditional (ternary) |
| overpy-parabola | 1 | `parse-error` 35:37–38 — numeric member `Team.2` in `createDummy(Hero.TORBJORN, Team.2, ...)` |
| overpy-crosshair | 1 | `parse-error` 31:36–41 — bytes literal `b" \n"` |
| overpy-inputhud | 1 | `parse-error` 41:63–65 — inline conditional `"..." if any([...` |
| skirmish_elim | 1 | `unsupported-directive` 48:1 — `#!obfuscate` (settings block 1–30 parsed) |
| lucioball_all_heroes | 1 | `lex-error` 66:1–2 — unterminated (multiline) string literal |

Settings is **never** the first failure in any of the nine. skirmish_elim
and lucioball were re-fetched from the GitHub API at `eea67ad` (sha256
`a4e72e1f…ceec74` / `3e482e96…454840`, matching the recorded hashes) and run
against the native frontend; the committed parabola/crosshair/inputhud are
byte-identical to the corpus cache. Every claimed line:col was verified
against the source text (column-exact `substr` checks): 53:55 = `[` of
`float[0.5:10]`; 192:99 = `=` of `rate=`; 55:53 = `if` keyword; 35:37 = `2`
of `Team.2`; 31:36–41 = the `" \n"` string of the bytes literal; 41:63 = `if`
of the conditional.

## C. AC-2 — section-level emission: **PARTIAL (pixelart/santa pass; inputhud fails; parabola/crosshair unverified in-tree)**

- **pixelart**: emitted settings section from `wright compile` equals the
  oracle.json section, whitespace-collapsed — **256/256 chars, equal**
  (verified end-to-end at the CLI, not just via tests). Section ordered
  before `variables` in the native output ✓.
- **santa**: emitter test `settings_emission_matches_oracle_for_santa`
  (whitespace-collapsed section equality) — passes.
- **parabola / crosshair**: no in-tree emission test; the emission table
  covers their keys (verified against the 20 entries), and their
  descriptions contain no escapes, but section equality is **not verified
  by any committed test** (they do not compile natively, so no CLI emission
  exists). Evidence boundary recorded.
- **inputhud**: **FAILS** — see Discrepancy 1. The `\n` escapes in the
  description are decoded to real newline characters by the native JSONC
  parser (`parse_string_value`, `crates/wright-opy/src/settings.rs`:
  `'n' => value.push('\n')`) and emitted unescaped (`escape_string` only
  escapes `"`), while the oracle emits literal `\n` text
  (`Description: "…Zezombye.\n\nYou are…"`). End-to-end repro
  (`settings { "main": { "description": "Line one\n\nLine two" }, … }` +
  `gamemodes.general.heroLimit`):
  - native `od -c`: `L i n e   o n e \n \n L i n e` (real 0x0A bytes)
  - oracle `od -c`: `L i n e   o n e \   n \   n   L i n e` (0x5C 0x6E text)
  Under the whitespace-collapse normalizer the real newlines vanish while
  the literal `\n` text survives — the collapsed sections differ. No
  committed test exercises inputhud's description.

## D. AC-3 — pixelart primary full-program candidate: **NOT JUSTIFIED as a full-program row**

- Full native compile: exit 0 ✓ (re-run; machine report `target/m11-nlevel.json`
  also records `nativeExit: 0`).
- Settings section equality: 256/256 ✓.
- **Full-program normalized equality: FAILS.** With the v1 normalizer
  (HUD-collapse + whitespace-collapse, `scripts/v1-gates.py`), the first
  difference is at normalized char 411:
  native `SetGlobalVariable(owo,Array(".","▒▒…"))` vs oracle
  `SetGlobalVariable(owo,Array(CustomString("."),CustomString("▒▒…")))` —
  the native emitter renders array string elements without the
  `CustomString()` wrapper. This is **pre-existing emitter behavior** (the
  `Value::String` rendering is untouched by the batch), not a settings
  regression. The machine report independently records
  `normalizedEqual: False` for pixelart.
- Adapter HIR: the adapter converts pixelart (protocol 1.1.0, typed settings
  subtree present) and emits the same oracle section through the native
  protocol path (`wright compile --kind protocol` on the adapter JSON →
  256/256 section equality). Native≡adapter≡oracle for the settings
  section (transitive); direct native-vs-adapter subtree equality for a
  real-world program is not asserted by any committed test (the
  differential harness covers only `synthetic/settings`, which does pass).
- **Verdict**: a pixelart **settings-section** claim is justified; a
  **full-program** N-level/parity row is not justified under the current
  normalizer (pre-existing emission gap, unrelated to settings). No parity
  row for pixelart is in the tree, consistent with AC-8.

## E. AC-5 — bounded settings parity: **PASS**

`cargo test -p wright-opy --test differential` green (1/1). `PARITY_CASES`
now has **7 rows**: 5 synthetic + `synthetic/settings` + `real-world/overpy-cake`
— exactly one settings row added, and it is the bounded synthetic fixture;
**no real-world settings fixture** in `PARITY_CASES`. The
`adapter/fixtures/synthetic/settings.json` subtree sanity-checks against the
native HIR (differential test compares them span-stripped). Machine report:
`real-world/overpy-pixelart` differentialStatus "recorded in
wright-differential-report.json"; `synthetic/settings` normalizedEqual True,
byteEqual False.

## F. AC-6 — structured rejection: **PASS**

Throwaway repros (both `wright check` and `wright compile`, identical
code+span):

| Repro | code | span | message |
| --- | --- | --- | --- |
| unknown key `gamemodes.general.bogusKey` | `settings-unknown-key` | 5:13 (key) | names the key |
| unknown enum `heroLimit: "banana"` | `settings-unknown-value` | 4:13 | names value + key |
| settings in included file | `settings-placement` | inc.opy:1:1 | "only supported in the main file" |
| second settings block | `settings-placement` | 8:1 (keyword) | "must be the first construct" |
| `settings "file"` form | `settings-invalid` | 1:1 | names the unsupported form |
| unknown gamemode `gamemodes.lucioball` | `settings-unknown-key` | 3:9 | "unknown game mode 'lucioball'" |

Nothing silently dropped; every message names the offending key/value;
check and compile report the same span. The driver regression test
`opy_unknown_settings_key_fails_check_and_compile_identically` asserts the
same for check/compile.

## G. AC-7 — no global braces: **PASS**

- `wright compile .../overpy-meipocalypse/meipocalypse.opy` →
  `lex-error` `unexpected character '{'` at `meipocalypse.opy:223:37` —
  **unchanged**.
- Dict regression: `dict_literal_braces_still_lex_error` unit test in
  `crates/wright-opy/src/preprocess.rs` (lex-error with the right code,
  message, and span line 2) — passes.
- `git diff 544aa16..HEAD -- crates/wright-opy/src/lexer.rs` is **empty**.

## H. AC-9 — support-matrix `in` wording: **PASS**

`git show 093789d -- docs/opy/support-matrix.md`:
- settings entry added under the supported surface (top-of-file JSONC block,
  scoped lexing, typed HIR, emission before `variables`, placement rules,
  `settings-unknown-key`/`settings-unknown-value` rejections).
- Operator list corrected: `in` is only the `for ... in` header keyword;
  expression-level `in`/`not in` explicitly **not supported** and moved to
  the deferred list with the broken-weapons `:107` evidence.
- Claims match observed behavior: `not in [Hero.HANZO, Hero.BRIGITTE]`
  exists at `broken_weapons.opy:107` (verified in source); an independent
  minimal repro of that exact construct → `parse-error expected ']'`.

## I. Regression suites: **PASS**

| Suite | Result |
| --- | --- |
| `python3 -m unittest discover -s compatibility/tests` | 10/10 OK |
| `python3 compatibility/run_oracle.py` | 20/20 PASS (16 pre-existing + 3 acquisitions + synthetic/settings) |
| `node --test` (adapter/) | 22/22 pass |
| `python3 scripts/v1-gates.py` | 6/6 pass (FIXTURES unchanged) |
| `cargo test -p wright-opy` (+ differential) | 38 + 1 green |
| `cargo test -p wright-core` | 10 + 20 + 6 green (incl. settings validation tests) |
| `cargo test -p wright-workshop` | 55 green across 8 binaries (incl. 10 settings emitter tests) |
| `cargo test -p wright-driver` | 15 + 6 + 7 green; driver integration suite 3/3 stable (the `bee8c19` temp-dir race is resolved by `924f636`) |
| `cargo test -p wright-language` | 55 green |
| `cargo test -p wright-lsp --test lsp` | 22/22 |

Snapshot integrity: `git diff 544aa16..HEAD -- compatibility/fixtures` is
**additions only** (3 acquisitions + synthetic/settings; no pre-existing
oracle.json modified). Adapter fixture diffs: exactly 6 files × 2 lines,
`protocol.version` 1.0.0 → 1.1.0 only. `PROTOCOL_VERSION` is 1.1.0 in both
producers (`wright-opy/src/lower.rs`, `adapter/lib/adapter.js`).

## J. Docs: **PASS**

- `docs/hir/opy-hir-v1.md`: version 1.0.0 → **1.1.0** (envelope + §2.1 +
  §13 version history); §2.5 settings subsection (node grammar, JSON
  example, gamemodes requirement); §8 validation item 6 (settings span/
  key/domain checks); §11 "custom game settings blocks" bullet **removed**;
  §10 dump coverage mentions settings.
- `docs/cli.md`: `settings-invalid`, `settings-placement` (frontend) and
  `settings-unknown-key`, `settings-unknown-value` (validation) documented.
- `crates/wright-ir/src/settings/table.rs`: mandatory provenance header
  (pinned oracle 9.7.10 en-US, the 7 oracle-success programs, commit
  `eea67ad`, LICENSE-BOUNDARY observed-behavior policy); **20 entries** +
  name maps (7 mode, 2 map, 10 hero, 1 team) — counts verified.
- Emitter module doc: "reparses to equivalent WIR" amended with the
  settings exception; roundtrip boundary test
  `settings_emission_is_rejected_by_the_workshop_parser` exists and passes
  (ws parser rejects settings-bearing emission).
- Adapter lockstep: driver.js pre-extraction (mirrors `find_blocks`), the
  `compiledCustomGameSettings` gate removed, adapter.js settings mapping,
  pixelart + client-to-server convert (verified by direct runs), santa +
  broken-weapons recorded as FAILURE fixtures with `__doWhile__` (verified
  verbatim); `unsupported-settings.opy` mini-fixture replaced by
  `settings.opy` + `settings.json`.
- `scripts/m11-inventory.py` extended (synthetic/settings + 3 acquisitions;
  13 fixture records); `scripts/corpus-manifest.json` carries the three
  acquisitions with matching sha256; `scripts/acquire-corpus.py` extended.

## Corrections table — next-blocker records (rebaseline prediction vs actual first failure)

| Program | Rebaseline prediction (`8b782ad`) | Actual first failure at `6ea29ec` | Status |
| --- | --- | --- | --- |
| broken-weapons | `not in` at :107 | range syntax `float[0.5:10]` at **53:55** | Engineer correction **verified**; `not in` exists at :107 and fails independently, but is masked |
| santa | comprehension/conditional macro, first expansion :213 | named argument `rate=` at **192:99** | Engineer correction **verified** (the macro at :210/:213 still exists but is never reached) |
| client-to-server | chained conditional :55 | ternary at **55:53** | confirmed |
| parabola | inline conditional + bytes :46 | numeric member `Team.2` at **35:37** | **correction** (both the conditional at :46 and `++` at :70 exist; `Team.2` precedes them) |
| inputhud | comprehension :83 | inline conditional at **41:63** | **correction** (the comprehension at :83 exists; the conditional at :41 precedes it) |
| crosshair | bytes literal :31 + concat :34–41 | bytes literal at **31:36** | confirmed |
| pixelart | none | none (exit 0) | confirmed |
| skirmish_elim | `#!obfuscate` :48 | `unsupported-directive` at **48:1** | confirmed |
| lucioball_all_heroes | oracle gamemode validation (native not previously recorded) | multiline string `lex-error` at **66:1** | **new record** (native blocker; settings validation never runs — the lex error preempts) |

AC-4 note: the issue's "Known next blockers" table is accurate for 4 of 7
natively-compiling programs; parabola and inputhud have earlier blockers
than recorded. All later blockers are outside #86 scope and are not #86
failures; the rebaseline table in `m11-inventory-rebaseline.md` should be
corrected on the next refresh (this report is the record).

## Discrepancies vs the Engineer report

1. **"inputhud `\n` renders as literal newline matching oracle" — FALSE.**
   Native renders actual newlines (0x0A); oracle renders literal `\n` text
   (0x5C 0x6E); proven end-to-end with a minimal compiling program. Under
   the whitespace-collapse normalizer the sections differ, so inputhud's
   AC-2 section equality fails. Root cause: `\n` decoded in the JSONC
   parser, not re-escaped at emission.
2. **"santa + inputhud verified via emitter tests" — inputhud has no
   emitter test.** Only pixelart and santa have oracle-section emitter
   tests; parabola/crosshair/inputhud have none (inputhud additionally
   cannot pass one until the `\n` rendering is fixed).
3. **parabola/inputhud next blockers not reported.** The Engineer summary
   omitted parabola's `Team.2` at :35 and inputhud's conditional at :41
   (both earlier than the rebaseline predictions).
4. Everything else in the Engineer summary verified: pixelart exit 0;
   broken-weapons 53:55; santa 192:99; client-to-server 55:53; ow1/6v6 `\`
   lex-errors unchanged (spot-reconfirmed this session at
   `common/env.opy:7:66` and `constants/adj_constants.opy:8:85`); meipocalypse
   223:37; cronch 32:21; zencopter 38:22; 256/256 pixelart section; 20 table
   entries; adapter conversions and `__doWhile__` records; protocol 1.1.0;
   pre-existing adapter fixtures 2 lines each.

## Pixelart full-program row justification (D)

**Not justified.** Full compile (exit 0) and settings-section equality hold,
but full-program normalized output equality fails on a pre-existing
emission difference (`Array` string elements lack `CustomString()`), and
the machine report records `normalizedEqual: False` itself. A
settings-section N-level claim is supported; a full-program N-level/parity
row would require the normalizer or the emitter to change — which is an
implementation decision outside this report's scope. AC-8 is respected: no
forced parity row exists in the tree.

## Recommendation on remaining gate items

- AC-2 for inputhud (and the inputhud `\n` claim) is the only failing
  acceptance criterion found; the fix is an emission detail
  (re-escape `\n`/`\t`/`\r` in `escape_string`), outside QA authority to
  implement. Until resolved, the inputhud section-equality claim must not
  be made.
- The pixelart full-program parity row stays contingent (AC-3/AC-8); the
  settings-section evidence stands.
- The next-blocker records in `m11-inventory-rebaseline.md` (parabola,
  inputhud, lucioball) need a doc correction on the next inventory refresh.
