# M11 inventory reassessment: #83/#84/#85 verification and corrected provenance

related_issue: "#82"
freshness: snapshot
as_of_commit: f5fe337
status: verification
owner: QA

This is the QA-owned independent verification of issues #83 (included-file
diagnostic identity), #84 (deferred-surface doc alignment) and #85
(settings-free parity candidate pre-check), plus a corrected-provenance
reassessment of the phase-1 gap inventory
([`m11-gap-inventory.md`](m11-gap-inventory.md), snapshot `2ecc024`).
Every result below was re-derived at `f5fe337` from the working tree or the
pinned references, not copied from the Engineer records. The inventory
classifications are re-stated here; implementation proposals and
prioritization are PM-owned and intentionally absent.

Verification environment: working tree clean at `f5fe337` (CI run
31730590203 green). Native binary `target/debug/wright` built from `f5fe337`.
Oracle: pinned overpy 9.7.10 (`compatibility/oracle/package.json`, gitHead
`1e2688954302a402d076944b46db07efb14d7b61`). Pre-fix binary built from
`bee8c19~1` in a disposable worktree for the before/after repro.

## #83 — fix(driver): preserve included-file identity in frontend diagnostics

### Scope of the change (diff of `bee8c19`)

- `crates/wright-driver/src/session.rs` (+35/−5): the native `.opy` path now
  calls `wright_opy::compile_with_overlay_outcome` instead of `compile`, so
  the frontend file registry survives a failed compile; `opy_diag` now
  resolves `span.file` through the retained `FileRecord` registry with the
  resolved display path as fallback (file 0 is the main display path).
- `crates/wright-driver/tests/driver.rs` (+95): two regression tests (see
  below) plus a `temp_dir` helper.
- No `wright-opy`, `wright-language`, or `wright-lsp` code changed:
  `git diff 45df86d..HEAD -- crates/` touches only `session.rs` and
  `tests/driver.rs`. The frontend outcome form (`CompileOutcome`, registry
  retention) pre-existed for language tooling; the driver now uses it.

### Regression tests

Both tests pass in isolation and in a single-threaded run; the parallel run
is flaky (see the coverage/flakiness finding below — not a product defect).

1. `opy_lex_error_in_included_file_names_the_included_file`: a `\`
   lex-error inside `shared.opy` (included from `main.opy`) must be reported
   with `span.path == "shared.opy"`, `line 2`, `col 1`, and the asserted
   position must exist in the included file (bounds check). This covers the
   wrong-path case: the assertion fails if the span is stamped with the
   main file's path.
2. `opy_include_diagnostics_resolve_through_the_registry`: `include-not-found`
   raised from the main file keeps the main file's path; the
   cycle-closing `#!include` inside the included copy of the main file
   names `"main.opy"`. Confirms registry-based resolution is not broken by
   the identity fix.

### Repro before/after (ow1-emulator)

Pre-fix binary (`bee8c19~1`):

```
$ wright compile .../ow1-emulator/1v1_main.opy --root ... --profile compat -f json
lex-error compatibility/fixtures/real-world/ow1-emulator/1v1_main.opy:7:66
```

The pre-fix span was stamped with the root file's display path; line 7 of
`1v1_main.opy` is `#!include "common/macro_functions.opy"` (38 chars), so
column 66 did not exist there — the exact defect recorded in the inventory.

Post-fix binary (`f5fe337`), same command:

```
$ target/debug/wright compile compatibility/fixtures/real-world/ow1-emulator/1v1_main.opy \
    --root compatibility/fixtures/real-world/ow1-emulator --profile compat -f json
lex-error "unexpected character '\'", span { file: 1, path: "common/env.opy", start: 7:66, end: 7:67 }
```

`common/env.opy` line 7 is 66 bytes (`macro GAMDMODE_DESCRIPTION = (GAMEMODE_NAME "(" GAMEMODE_CODE ")\n"\`),
the `\` is the final byte at column 66 — the reported span exists in the
included file.

- 6v6-adjustments: `lex-error` `unexpected character '\'` at
  `constants/adj_constants.opy:8:85-86` (line 8 is 85 bytes, `\` at
  column 85) — previously reported as `main.opy:8:85`.
- meipocalypse: `lex-error` `unexpected character '{'` at
  `meipocalypse.opy:223:37-38` (file 0 — main-file paths were never
  misattributed; unchanged).

### Structured/JSON and CLI text paths

- JSON envelope `wright-result/v1` carries the corrected path: the
  `diagnostics[].span.path` above is `common/env.opy` with `file: 1`, the
  included file id.
- Human text names the included file:

```
$ target/debug/wright compile .../1v1_main.opy --root ... --profile compat
error[lex-error] (frontend): unexpected character '\'
  --> common/env.opy:7:66
```

### LSP/service path

No LSP change was needed: `crates/wright-language/src/service.rs` was
already source-aware. `source_identity` (service.rs:350) maps file id 0 to
the requesting document URI and any other id through the registry
(relative include paths joined against the workspace root);
`diagnostic_location` (service.rs:399) uses the same mapping for published
diagnostics, so included-file diagnostics — lex and parse alike — publish
under the included source's identity regardless of the stage.

Suites: `cargo test -p wright-language` green (8+9+6+1+31 tests across
5 binaries); `cargo test -p wright-lsp --test lsp` green (22/22).

Protocol coverage note (not a blocker): the existing included-source
protocol test `lsp_included_diagnostics_retire_when_the_source_disappears`
(lsp.rs:1555) publishes an included-file error under `bad.opy`, but the
fixture's error is a **parse**-error (`this is not valid opy` →
`parse-error`). No LSP or service test exercises a **lex**-error inside an
included file under the included identity; that stage is covered by the
driver regression test and by the shared `source_identity` path, not by a
protocol test.

### Suites

- `cargo test -p wright-driver`: green single-threaded; flaky in the
  default parallel run (see finding).
- `cargo test -p wright-opy`: green (19 unit + 1 differential row, 6
  PARITY_CASES unchanged).

### Finding — regression-test race (introduced by bee8c19)

The new `temp_dir` helper and the pre-existing `temp_file` helper each own
an independent `AtomicUsize` counter starting at 0 and build
`wright-driver-test-{pid}-{n}` names. Under parallel execution two tests
can therefore allocate the **same** directory; whichever test's
`remove_dir_all` runs first deletes the other test's fixtures mid-test.
Measured on `cargo test -p wright-driver --test driver`: 3 of 5, then 5 of
8 parallel runs failed (missing `shared.opy` / `include-not-found` instead
of `lex-error`); `-- --test-threads=1` passes deterministically (1/1). The
green CI run is consistent with a timing-dependent race. This is a
test-harness defect in the #83 commit, not a product defect; the fix
behavior itself is verified independently above. Correcting the helper
names is an Engineer task, not done here.

## #84 — docs: align deferred .opy surface with phase-1 corpus evidence

`git show c57dbdb`: only `docs/opy/support-matrix.md` changed (+15 lines).
Every bullet was reproduced against observed behavior at `f5fe337`:

| Doc claim (support-matrix deferred section) | Observed at f5fe337 | Stage claim | Verdict |
| --- | --- | --- | --- |
| `\` line continuation rejected at lexing, `lex-error` `unexpected character '\'` on ow1-emulator and 6v6-adjustments | ow1: `lex-error` at `common/env.opy:7:66`; 6v6: `lex-error` at `constants/adj_constants.opy:8:85` | lexing | accurate (paths now corrected by #83; the matrix already named the byte-true files) |
| Pinned oracle 9.7.10 compiles `\`; its failures on those fixtures occur later, on unrelated constructs | ow1 oracle snapshot: `Found 'if', but no 'else'` at `arena.opy:94` chained from `1v1_main.opy:86` (inline `if` without `else` in the `nextHero` macro); 6v6: `Unknown member '_hp_reset'` at `custom_hp.opy:31`. Both failures sit after `common/env.opy:7`/`constants/adj_constants.opy:8` in include order (env.opy is the first include of `1v1_main.opy`) | — | accurate |
| `++`/`--` rejected at parsing; `parse-error` `expected an expression but found '+'` on `++` in overpy-cronch, `cronch.opy:32`; `--` not separately observed | `parse-error` `expected an expression but found '+'` at `cronch.opy:32:21` (`nbPlayersKilled++`). Stage claim for `--` verified independently: a minimal `x--` repro yields `parse-error expected an expression but found ' '` | parsing | stage/code/location accurate; the parenthetical "`--` not separately observed in the phase-1 corpus" is **inaccurate** — see finding below |
| Dict literals rejected at lexing; `lex-error` `unexpected character '{'` on meipocalypse, `meipocalypse.opy:223` | `lex-error` at `meipocalypse.opy:223:37-38` (`{` of the dict literal in the expression statement) | lexing | accurate |
| `"""` rejected at lexing; `lex-error` `unterminated string literal` on zencopter, `heli.opy:38`; pinned oracle 9.7.10 also fails on this construct | `lex-error` at `heli.opy:38:22-23` (line is 22 bytes; the closing `"` of the docstring line opens an unterminated string); oracle snapshot: `Invalid content before string: 'arena'` at `heli.opy:38:17` | lexing | accurate |

No bullet claims a stage or code that is not evidenced, and none conflicts
with observed behavior — with one exception:

**Finding — `--` IS present in the phase-1 corpus.** The deferred bullet
"`--` not separately observed in the phase-1 corpus" is inaccurate: postfix
decrement occurs at three sites in meipocalypse
(`nbRemainingMeis--` at `meipocalypse.opy:221`, `nbMeisFighting--` at
`meipocalypse.opy:251`, `playersWithBarricades[eventPlayer.wallIndex].barricadeHealth--`
at `barricades.opy:196`), and in no other fixture. It is never the *first*
native failure on any fixture because meipocalypse's dict-literal lex-error
at `meipocalypse.opy:223` preempts parsing (lexing precedes parsing, and
`--` lexes as two `-` tokens without error), so `--` was never observed as
a blocking construct — the bullet's intended meaning. The bullet's stage
claim (rejected at parsing) is nevertheless correct and was independently
verified with a minimal `x--` repro (`parse-error expected an expression
but found ' '`). This is a wording/evidence-accuracy flag, not a behavioral
discrepancy.

The oracle claims were checked against the
committed `oracle.json` snapshots (re-derived by the same pinned 9.7.10 in
`run_oracle.py`).

## #85 — settings-free parity candidate pre-check

### Independent re-derivation

All five shortlist files were re-fetched from the GitHub API contents
endpoint at the pinned commit
`eea67adbcf6926c4004e35e25ab4be072624a44e` (base64-decoded), not from the
Engineer's cache, and verified byte-identical to the local corpus cache.

| Candidate | Size (B) | SHA-256 (API fetch, full) | Doc record | Settings block (observed) | Oracle exit | Native first failure | Adapter |
| --- | --- | --- | --- | --- | --- | --- | --- |
| parabola.opy | 3035 | `2dce50c374a00e3d3e664f4753f736f788cfb2d0df3a2149b139e6f8cb484940` | `2dce50c3…484940` | lines 4–19 of 71 (file has no trailing newline) | 0 success (2 warnings) | `lex-error` `{` at 4:10 | `unsupported` (settings boundary) |
| skirmish_elim.opy | 4958 | `a4e72e1f995c5b0aba6c937f0292ccaa03f60423dec8b1a48d11756172ceec74` | `a4e72e1f…ceec74` | lines 1–30 of 160 | 1 failure (`#!obfuscate` 48:1) | `lex-error` `{` at 1:10 | `parse` (`#!obfuscate` 48:1) |
| crosshair.opy | 1404 | `d2c22b59c11fee75ae4febc96c94126059f4e5c5f64cc2e5a28e939968ed8f8f` | `d2c22b59…ed8f8f` | lines 7–22 of 49 (no trailing newline) | 0 success | `lex-error` `{` at 7:10 | `unsupported` (settings boundary) |
| inputhud.opy | 2602 | `ae7a0e004cc80e61d256d0ff2509f51bdc96b27d60a93c9215b327436e7eefc1` | `ae7a0e00…eefc1` | lines 4–20 of 94 | 0 success | `lex-error` `{` at 4:10 | `unsupported` (settings boundary) |
| lucioball_all_heroes.opy | 6114 | `3e482e96da1921979914923d948a2503c1b7bc1afee541c562b850a688545840` | `3e482e96…454840` | lines 6–43 of 223 | 1 failure (lucioball gamemode 39:34) | `lex-error` `{` at 6:10 | `parse` (lucioball gamemode 39:34) |

All five SHA-256 records, sizes, settings-block start lines, oracle exit
codes and verbatim oracle diagnostics match. Oracle re-run (pinned 9.7.10,
`pnpm exec overpy compile`): parabola exit 0 with `w_chase_9999` at
43:28 and `w_9999` at 57:32; skirmish_elim exit 1
`Unknown preprocessor directive '#!obfuscate'` at 48:1; crosshair and
inputhud exit 0 with no diagnostics; lucioball_all_heroes exit 1
`The gamemode 'lucioball' is not available in OW2` at 39:34.

**Finding: 0/5 candidates pass the pre-check gate.** All five carry a
top-of-file `settings {` block — the same deferred construct that blocked
4/9 phase-1 fixtures — and the native frontend stops at the block's
opening brace in every case. Two of five (skirmish_elim, lucioball_all_heroes)
additionally fail the pinned oracle itself, which is evidence for the later
reference-strategy investigation, not for the current surface. **No parity
candidate was obtainable from the OverPy examples source**: no fixture was
acquired, no `oracle.json`, no corpus-manifest record, and no
`PARITY_CASES` row was added (`crates/wright-opy/tests/differential.rs`
still lists the same 6 rows as at `45df86d`; the differential test is
green). The 0/5 result and its non-acquisition decision are recorded in
`docs/opy/m11-settings-free-candidates.md` and are visible in the corpus
state at `f5fe337`.

Minor wording flag (data unaffected): the candidates doc says "First 20
lines of every candidate open with comments and then `settings {`".
skirmish_elim.opy opens directly with `settings {` at line 1 (no leading
comment) and inputhud.opy has a single `#OverPy starter pack` comment line.
The per-candidate start lines in the same sentence are correct.

## Corrected-provenance reclassification (phase-1 fixtures)

After the #83 fix, every phase-1 native first-failure now names the source
file that actually contains the error:

| Fixture | Native first failure at 2ecc024 (reported path) | At f5fe337 (corrected) | Classification change |
| --- | --- | --- | --- |
| ow1-emulator | `1v1_main.opy:7:66` (bogus — col 66 absent) | `common/env.opy:7:66` | **stands**: class 4 primary, class 2 sub (`\` continuation). Location was already recorded byte-true in the inventory matrix |
| 6v6-adjustments | `main.opy:8:85` (bogus — col 85 absent) | `constants/adj_constants.opy:8:85` | **stands**: class 4 primary, class 2 sub (`\` continuation) |
| overpy-meipocalypse | `meipocalypse.opy:223:37` (file 0) | unchanged | stands: class 4 primary, class 2 sub (dict literal) |
| overpy-zencopter | `heli.opy:38` (file 0) | unchanged (38:22) | stands: class 4 primary, class 2 sub (`"""`) |
| overpy-cronch | `cronch.opy:32:21` (file 0) | unchanged | stands: class 2 primary, sub `@Name` (adapter) |
| overpy-pixelart / santa / broken-weapons / client-to-server | settings block, main file, correct | unchanged (7:10, 7:10, 7:10, 14:10) | stands: class 2 primary |

**No phase-1 classification changes.** The two misattributed cases
(ow1-emulator, 6v6-adjustments) were already classified against the
byte-true locations (`env.opy:7:66`, `adj_constants.opy:8:85` appear in the
inventory's outcome matrix and per-fixture sections), so the correction
affects the reported *path* only, never the class or sub-finding. The
inventory's "Span provenance defect" section (lines 237–249 of
`m11-gap-inventory.md`) is therefore **resolved** at `f5fe337`: the defect
it described no longer reproduces. That section is superseded by this
document; the inventory itself was not edited (QA scope for this task is
this reassessment only), so the stale note remains in place until the next
inventory refresh.

## Resulting inventory picture

| Source | Class | Count |
| --- | --- | --- |
| phase-1 fixtures, primary class 2 (explicit unsupported surface) | settings block `{` (4), `++` (1) | 5 |
| phase-1 fixtures, primary class 4 (reference-oracle limitation) with class 2 native sub-findings | `\` continuation (2), dict literal (1), `"""` (1) | 4 |
| phase-1 fixture, class 6 (missing evidence) | zombies (metadata-only) | 1 |
| settings-free pre-check (5 candidates) | all settings-blocked at the native lexer; 2/5 additionally fail the pinned oracle | 0/5 pass, none acquired |
| parity corpus | unchanged at 6 rows (`overpy-cake` remains the only real-world parity fixture) | 6 |

The corrected provenance removes the last diagnostic-quality defect on the
path to these findings; the findings themselves are unchanged: no
supported-surface construct fails (no class 3 anywhere), no class 1 or
class 5 finding exists, and the settings-free candidate source (OverPy
examples at `eea67ad`) is exhausted by the settings block until the
settings surface (or an alternative reference) exists. The native evidence
boundary remains far narrower than the oracle's for every fixture.

## Evidence boundary

- **Pinned-oracle coverage limit**: oracle evidence is pinned overpy
  9.7.10 only. Everything the oracle "accepts" or "rejects" is a 9.7.10
  observation. Specifically: the oracle compiles the `\` continuation
  (ow1/6v6 fail later on unrelated constructs — verified by include order
  and failure position), rejects `"""` docstrings (zencopter), rejects
  `#!obfuscate` (skirmish_elim) and the `lucioball` gamemode
  (lucioball_all_heroes), and its meipocalypse failure is artifact
  availability (missing `generateWalls.js`), not syntax.
- **Newer-OverPy syntax unverified**: whether newer OverPy versions accept
  `"""` docstrings, `#!obfuscate`, custom `_hp_*` members, or the inline
  `if` without `else` pattern is **not verified** — no newer OverPy is
  pinned or installed, and none of these was checked against one. All
  version-sensitivity claims remain inconclusive.
- **Dict-literal inference**: OverPy 9.7.10's acceptance of the meipocalypse
  dict literal is inferred from the ENOENT surfacing after line 223, not
  from a completed compile.
- **Native evidence boundary**: native runs stop at the first unsupported
  token; for settings-blocked fixtures (pixelart, santa, broken-weapons,
  client-to-server and all five candidates) the program body is never
  lexed. Any statement about those files' bodies rests on the oracle, not
  the native frontend.
- **Driver suite race**: `cargo test -p wright-driver` is not reliable in
  the default parallel run at `f5fe337` (see the #83 finding); product
  behavior is verified by the single-threaded run and the CLI repros above.
- **Candidates acquisition**: the candidates were never committed; all
  bytes were verified against the GitHub API at the pinned commit, but the
  files exist only in `target/corpus-cache` locally and in this document's
  hash records.

## REQ/AC compliance notes

- **#83 (included-file diagnostic identity)**: verified fixed. The
  diagnostic contract (source-located spans) now holds for lex errors in
  included files in the driver path, matching the language-service
  behavior. Remaining gap: the flaky parallel driver suite (test-harness
  race introduced with the regression tests) and the missing
  lex-error-in-include LSP protocol test (coverage note, not a blocker).
- **#84 (support-matrix deferred-surface claims)**: verified with one
  wording flag. Every bullet's rejection stage, code, fixture, and location
  matches observation; the `--` parenthetical ("not separately observed in
  the phase-1 corpus") is inaccurate — `--` occurs at three meipocalypse
  sites, it is simply never the first native failure. The oracle-side
  claims match the pinned-9.7.10 snapshots.
- **#85 (settings-free parity candidate pre-check)**: verified. 0/5 pass;
  all five settings-blocked; 2/5 fail the pinned oracle; no acquisition,
  no parity row, no adapter registration; recorded in the candidates doc
  and consistent with the corpus state at `f5fe337`.
- **Inventory REQ-008 spirit (no feature implementation)**: the
  `45df86d..f5fe337` range changes no frontend/backend/language-service
  code — only the driver diagnostic mapping, its tests, and docs.
