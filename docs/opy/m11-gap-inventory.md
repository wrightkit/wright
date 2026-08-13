# M11 phase-1 real-world corpus: classified gap inventory (#81)

related_issue: "#81"
freshness: snapshot
as_of_commit: 2ecc024370b105e06a5ed936b41d665eef2d0209
status: verification
owner: QA

This is the QA-owned classified gap inventory for SPEC-M11 phase 1 (issue #81):
a per-fixture classification of the real-world corpus added in `ccdc8bf`
(plus `50d2a88`, `4d87450`, `9a290cf`, `2ecc024`), with independently
re-derived evidence. It records what was observed at `2ecc024`; it does not
propose or prioritize implementation work (PM owns that decision after this
inventory).

## Headline finding

None of the nine phase-1 real-world fixtures can be consumed end to end by
Wright's current stack:

- **Native frontend: 9/9 fail** at the first out-of-surface construct
  (settings block `{`, `\` line continuation, `"""` docstring, `++`
  postfix, dict literal `{`).
- **Pinned adapter: 9/9 fail** (5 at the settings boundary, 1 at `@Name`,
  3 on pinned-OverPy parse errors, 1 at a missing JS helper) — so there are
  zero new success snapshots and zero new differential parity rows.
- **4/9 fixtures (pixelart, santa, broken-weapons, client-to-server) fail
  both engines at a top-of-file custom-game-settings block**, a documented
  deferred item; for those, the entire program body is unexercised by both
  Wright engines (e.g. all 185 KB of `pixelart.opy` after line 6).
- The 5 oracle-success fixtures prove only that pinned OverPy 9.7.10
  compiles them; the supported surface plus adapter cannot consume them.

The corpus therefore evidences that the declared supported surface, the
adapter boundary, and the pre-existing parity corpus (`overpy-cake` remains
the only real-world parity fixture) do not cover typical real-world programs
as they exist in the wild. No supported-surface construct was found to fail,
so no finding is classified as a Wright correctness bug (class 3).

## Classification schema

Primary class per fixture, with per-finding sub-rows:

1. supported parity
2. explicit unsupported surface
3. Wright correctness bug (oracle success + native fail/divergence on a
   **supported-surface** construct with a repro — none found, so not used)
4. reference-oracle limitation
5. intentional documented difference
6. inconclusive / missing evidence

**Primary-class rule:** the primary class names the dominant evidence gap.
Where the pinned oracle itself fails to compile the fixture (oracle
`failure`), no parity reference exists regardless of native status, so the
reference-oracle limitation (class 4) is primary and native-side findings are
sub-rows. Where the oracle succeeds, the primary gap is the Wright-side
unsupported surface (class 2). A native lex/parse error on a construct
outside the declared supported surface is never a class 3 (an unmet
support-matrix claim is class 3 only when the construct is inside the
declared surface).

## Classified inventory

| Fixture | Primary class | Sub-findings |
| --- | --- | --- |
| `real-world/overpy-pixelart` | 2 — explicit unsupported surface | — |
| `real-world/overpy-santa` | 2 — explicit unsupported surface | — |
| `real-world/overpy-broken-weapons` | 2 — explicit unsupported surface | — |
| `real-world/overpy-client-to-server` | 2 — explicit unsupported surface | — |
| `real-world/overpy-cronch` | 2 — explicit unsupported surface | 2 — `@Name` directive (adapter) |
| `real-world/overpy-meipocalypse` | 4 — reference-oracle limitation | 2 — dict literal (native) |
| `real-world/overpy-zencopter` | 4 — reference-oracle limitation | 2 — docstring lexing (native) |
| `real-world/ow1-emulator` | 4 — reference-oracle limitation | 2 — `\` continuation (native) |
| `real-world/6v6-adjustments` | 4 — reference-oracle limitation | 2 — `\` continuation (native) |
| `real-world/zombies` | 6 — inconclusive / missing evidence | metadata-only record, no content |

## Per-fixture findings and evidence boundaries

Evidence legend: **native** = `target/debug/wright compile <main> --profile
compat -f json --root <dir>` at `2ecc024`; **adapter** = pinned
`wright-adapter` (re-derived via `node --test` in `adapter/`); **oracle** =
`compatibility/run_oracle.py` snapshot against pinned overpy@9.7.10
(`compatibility/oracle/package.json`, gitHead `1e2688954302a402d076944b46db07efb14d7b61`).

### overpy-pixelart — class 2

- Oracle: **success** (exit 0). Adapter: **unsupported**
  ("custom game settings blocks are outside the Opy HIR v1 corpus boundary").
- Native: **lex-error** `unexpected character '{'` at `pixelart.opy:7:10` —
  the top-of-file `settings {` block; support-matrix deferred item
  ("Rule `disabled` markers and custom-game-settings blocks").
- Evidence boundary: native exercised only lines 1–6 of the 185,541-byte
  file; the settings block and the entire program body are unexercised by
  both Wright engines. Oracle compiled the full file.

### overpy-santa — class 2

- Oracle: **success**. Adapter: **unsupported** (settings boundary).
- Native: **lex-error** at `santa.opy:7:10` — top-of-file `settings {`
  block (same deferred item).
- Evidence boundary: native exercised lines 1–6 of 25,297 bytes; the rest
  unexercised by both Wright engines.

### overpy-broken-weapons — class 2

- Oracle: **success**. Adapter: **unsupported** (settings boundary).
- Native: **lex-error** at `broken_weapons.opy:7:10` — top-of-file
  `settings {` block (same deferred item).
- Evidence boundary: native exercised lines 1–6; the rest unexercised by
  both Wright engines.

### overpy-client-to-server — class 2

- Oracle: **success**. Adapter: **unsupported** (settings boundary).
- Native: **lex-error** at `clientToServer.opy:14:10` — `settings {` block
  at the top of the file (same deferred item; the fixture's own feature list
  includes "custom game settings").
- Evidence boundary: native exercised lines 1–13; the rest unexercised by
  both Wright engines.

### overpy-cronch — class 2

- Oracle: **success** (exit 0, one `w_ow2_rule_condition_chase` info
  diagnostic). Adapter: **unsupported** ("construct '@Name' is outside the
  Opy HIR v1 corpus boundary").
- Native: **parse-error** `expected an expression but found '+'` at
  `cronch.opy:32:21` — postfix increment `nbPlayersKilled++`, an
  OverPy-supported statement outside the declared native statement surface
  (support-matrix §Statements lists `=`/augmented assignment, `if`/`elif`/
  `else`, `for`, `while`, `pass`; the operator table lists no `++`).
- Sub-finding: `@Name "countdown timer"` at `cronch.opy:35:5` inside
  `def countdownTimer():` — the adapter rejects it at its corpus boundary;
  the native parser rejects `@` with `parse-error` (support-matrix:
  "other `@` directives fail explicitly" — documented explicit failure, not
  a bug). Native never reaches it because the `++` error precedes it.
- Evidence boundary: native exercised lines 1–31; oracle exercised the full
  file (full compile).

### overpy-meipocalypse — class 4 (sub: 2)

- Oracle: **failure** — pinned 9.7.10 dies with
  `ENOENT: no such file or directory, lstat '.../meipocalypse/generateWalls.js'`.
  Root cause: the source references the JS helper
  (`#!define generateWalls(map, walls) __script__("generateWalls.js")` at
  `meipocalypse.opy:57`; invoked at `zones.opy:3`), which is not committed
  with the fixture. **Not a version/newer-syntax issue** — 9.7.10 supports
  `__script__`; the failure is artifact availability (redistribution
  decision: metadata-only for the helper, matching the zombies policy).
- Adapter: **parse** (same ENOENT message, recorded verbatim).
- Native: **lex-error** `unexpected character '{'` at
  `meipocalypse.opy:223:37` — a dict literal in an expression statement
  (`getPlayers(Team.1).money += { Mei.GENERIC: 10, ... }`). Dict literals
  are outside the declared literal surface (support-matrix lists literals
  and arrays `[...]` only) — explicit rejection via `lex-error`, so class 2
  sub-finding. The support-matrix deferred section does not name dict
  literals (matrix gap, see flags).
- Evidence boundary: native exercised only `meipocalypse.opy` lines 1–222;
  main-file lexing fails before preprocessing, so none of the 10 included
  files (settings.opy, shop.opy, zones.opy, waves.opy, …) was ever spliced
  or lexed. The oracle parsed the full include closure — the ENOENT
  surfaced while expanding `generateWalls()` from `zones.opy`, after line
  223 — so OverPy 9.7.10 demonstrably *parsed* the dict literal (inferred
  from failure position, not a completed compile).

### overpy-zencopter — class 4 (sub: 2)

- Oracle: **failure** — pinned 9.7.10:
  `Invalid content before string: 'arena'` at `heli.opy:38:17`. Root cause:
  the `"""` docstring block (`"""@Rule "tp to arena"` at line 38). The
  construct lives in the overpy examples tree at the pinned commit itself
  (`eea67ad`), which 9.7.10 rejects — **plausibly a newer-OverPy syntax
  (docstrings added after 9.7.10); not verified against a newer OverPy**
  (inconclusive on version).
- Adapter: **parse** (same message verbatim).
- Native: **lex-error** `unterminated string literal` at `heli.opy:38` — the
  same `"""` construct; triple-quoted strings are outside the declared
  string surface (support-matrix: double-quoted strings with escapes) —
  class 2 sub-finding (deferred lexing surface).
- Evidence boundary: both engines stop at the same construct at line 38 of
  11,155 bytes; lines 39+ (including the second docstring at line 43) are
  unexercised by both.

### ow1-emulator — class 4 (sub: 2)

- Oracle: **failure** — pinned 9.7.10:
  `Found 'if', but no 'else'` at `arena.opy:94:14` (chained from
  `1v1_main.opy:86:1`). Root cause: an inline `if` without an inline `else`
  in a `\`-continued macro body (`macro nextHero(player):` in
  `misc/arena.opy`). Whether a newer OverPy accepts the pattern is
  **unverified — inconclusive on version**. The oracle also emits a
  `w_already_imported` warning for `watermark.opy` (first diagnostic);
  the snapshot records status failure/exit 1 with empty workshop text.
- Adapter: **parse** (same message verbatim).
- Native: **lex-error** `unexpected character '\'` — the backslash
  line-continuation at `common/env.opy:7:66` inside
  `macro GAMDMODE_DESCRIPTION = (...)("\n"\` — the first `\` in the first
  included file. Line continuations are outside the declared lexing surface —
  class 2 sub-finding. Note the span defect below: the diagnostic is
  reported as `1v1_main.opy:7:66`, where column 66 does not exist in the
  main file.
- Evidence boundary: native exercised `1v1_main.opy` lines 1–7 and
  `common/env.opy` lines 1–6 only; the other 143 files are unexercised.
  The oracle exercised the full 144-file closure through parsing and failed
  late (`misc/arena.opy:94`), so the oracle's evidence breadth is far larger
  than the native one.

### 6v6-adjustments — class 4 (sub: 2)

- Oracle: **failure** — pinned 9.7.10:
  `Unknown member '_hp_reset' of 'eventPlayer'` at `custom_hp.opy:31:17`
  (chained from `main.opy:14:1`). Root cause: `eventPlayer._hp_reset = false`
  assigns an undeclared player member; the project's custom `_hp_*` members
  are rejected by 9.7.10's member validation. The project targets a newer
  toolchain — **plausibly newer-OverPy custom-member syntax; unverified**
  (inconclusive on version).
- Adapter: **parse** (same message verbatim).
- Native: **lex-error** `unexpected character '\'` at
  `constants/adj_constants.opy:8:85` — the same `\` line-continuation
  construct as ow1-emulator, inside the first included file's
  `GAMDMODE_DESCRIPTION` macro — class 2 sub-finding (same span defect).
- Evidence boundary: native exercised `main.opy` lines 1–7 and
  `adj_constants.opy` lines 1–7 only; the other 115 files are unexercised.
  The oracle exercised the full 117-file closure and failed late
  (`custom_hp.opy:31`).

### zombies — class 6

- Metadata-only record in `scripts/corpus-manifest.json`
  (`real-world/zombies`, repo `WallerTrevor/zombies`, commit
  `9394dd30…`, 32 files with SHA-256, `"committed": false`, acquisition
  instructions via `scripts/acquire-corpus.py`). Missing evidence: the
  content itself is not redistributed, so no fixture dir, oracle snapshot,
  adapter outcome, or native outcome exists at this commit. The manifest
  decision (hash + acquisition instructions only) is documented as
  required by REQ-002.

## Span provenance defect (noted, not class 3)

For lex-errors inside included files, the native frontend reports the
included file's line/column coordinates under the *including* file's path
and identity: ow1-emulator's `\` error is emitted as `1v1_main.opy:7:66`
(byte is `env.opy:7:66`) and 6v6-adjustments' as `main.opy:8:85` (byte is
`adj_constants.opy:8:85`). Neither column exists in the reported file.
Repro: any main file that includes a file containing a lex-error. This is a
diagnostic-provenance defect on the documented diagnostics contract
(source-located spans), but it is **not class 3 by the strict definition**:
the triggering construct (`\`) is outside the supported surface, and no
oracle parity comparison exists for diagnostic locations. Recorded for
Engineer confirmation; classification unchanged.

## Diversity matrix (REQ-001)

| Structure category (REQ-001) | Fixtures |
| --- | --- |
| substantial single-file | pixelart (185,541 B), santa, cronch, broken-weapons, client-to-server, zencopter |
| multi-file / include / preprocessor-heavy | meipocalypse (11 files), ow1-emulator (144), 6v6-adjustments (117); zombies manifest (32, uncommitted) |
| variable / control-flow-heavy | pixelart, santa, cronch, meipocalypse, ow1-emulator, 6v6-adjustments |
| subroutine / macro-heavy | cronch, broken-weapons, ow1-emulator, 6v6-adjustments |
| real Workshop API / enums / actions | client-to-server, pixelart, zencopter, meipocalypse, ow1-emulator, 6v6-adjustments |

Origins independent of OverPy examples: `Overwatch-1-Emulator/ow1-emulator`,
`6v6-Adjustments/6v6-adjustments`, `WallerTrevor/zombies` (metadata-only) — 3
independent origins. All five categories are covered.

## Provenance verification (REQ-002)

- SHA-256 of **all 286 committed source files** across the 9 fixtures match
  the recorded `files` maps in `fixture.json` (re-computed, 0 mismatches).
- GitHub API contents endpoint (base64-decoded) byte-equality vs committed
  files at the pinned commits: `Zezombye/overpy examples/pixelart.opy` @
  `eea67ad` ✓; `Overwatch-1-Emulator/ow1-emulator src/1v1_main.opy` @
  `25cd6ce8` ✓; `6v6-Adjustments/6v6-adjustments src/main.opy` @
  `624480db` ✓ (3 files verified; ≥2 required).
- Every `fixture.json`: `sourceCommit` is 40-hex; `sourceUrl` and
  `licenseUrl` are commit-pinned (contain the sourceCommit); `license` is
  GPL-3.0-only for the eight overpy fixtures (incl. pre-existing cake) and
  BSD-2-Clause for ow1-emulator and 6v6-adjustments; zombies manifest entry
  records GPL-3.0-only with license first-lines.
- No non-`.opy` source files were committed: fixture directories contain
  only `.opy` sources plus `fixture.json`/`oracle.json`.
- `scripts/corpus-manifest.json` documents the zombies metadata-only
  decision (acquisition instructions + hashes, `committed: false`, no
  content).
- Pre-existing `oracle.json` snapshots are unmodified: `git diff
  45df86d..HEAD -- compatibility/fixtures` contains additions only.

## Machinery results (REQ-003..006, re-derived at `2ecc024`)

| Check | Command | Result |
| --- | --- | --- |
| Compatibility runner | `python3 -m unittest discover -s compatibility/tests` | 10/10 OK (asserts 16 fixtures) |
| Oracle snapshots | `python3 compatibility/run_oracle.py` | 16/16 PASS (statuses match `expectedStatus`; overpy 9.7.10 pinned, metadata unchanged) |
| Adapter suite | `node --test` (adapter/) | 21/21 pass (6 success fixtures; 10 failure fixtures with recorded codes + verbatim messages; 5 mini) |
| Differential parity | `cargo test -p wright-opy` | OK; PARITY_CASES unchanged at 6 rows (not extended) |
| N-level gates | `python3 scripts/v1-gates.py` | 6/6 pass; FIXTURES unchanged from `45df86d` |
| CLI build | `cargo build -p wright-cli` | OK |
| M11 inventory runner | `python3 scripts/m11-inventory.py` | regenerated `target/m11-nlevel.json` + `target/m11-gap-inventory.json` (runtime artifacts, uncommitted) |

Per-fixture native/adapter/oracle outcome matrix (all re-derived; span
columns are first-failure location):

| Fixture | Oracle status | Native first failure | Adapter |
| --- | --- | --- | --- |
| overpy-pixelart | success | lex-error `{` 7:10 (settings block) | unsupported (settings boundary) |
| overpy-santa | success | lex-error `{` 7:10 (settings block) | unsupported (settings boundary) |
| overpy-cronch | success | parse-error `++` 32:21 | unsupported (`@Name`) |
| overpy-broken-weapons | success | lex-error `{` 7:10 (settings block) | unsupported (settings boundary) |
| overpy-client-to-server | success | lex-error `{` 14:10 (settings block) | unsupported (settings boundary) |
| overpy-meipocalypse | failure (ENOENT generateWalls.js) | lex-error `{` 223:37 (dict literal) | parse (ENOENT) |
| overpy-zencopter | failure (`"""` docstring 38:17) | lex-error unterminated string 38 | parse |
| ow1-emulator | failure (if-without-inline-else arena.opy:94) | lex-error `\` env.opy:7:66 | parse |
| 6v6-adjustments | failure (`_hp_reset` custom_hp.opy:31) | lex-error `\` adj_constants.opy:8:85 | parse |

## Requirement compliance (REQ-001..009)

| Requirement | Result | Evidence |
| --- | --- | --- |
| REQ-001 (>=6 real-world fixtures, diversity matrix, >=2 non-overpy origins, >=1 >100 KB, >=1 multi-file) | PASS | 10 fixtures; matrix above; 3 independent origins; pixelart 185,541 B; 3 multi-file fixtures |
| REQ-002 (provenance/metadata completeness, redistribution review, non-redistributable hash/acquisition-only) | PASS | per-fixture fields verified; 286/286 hashes; 3 upstream byte-checks; zombies metadata-only |
| REQ-003 (oracle snapshots for every new fixture, pinned 9.7.10, metadata unchanged) | PASS | 9 new `oracle.json`; run_oracle 16/16; additions-only diff |
| REQ-004 (adapter HIR fixture per translatable fixture; verbatim errors otherwise) | PASS | 0 translatable of 9 new; errors recorded verbatim in `adapter/test/adapter.test.js`; `node --test` 21/21 |
| REQ-005 (native differential parity for supported-surface fixtures; hard-fail preserved; gap fixtures excluded) | PASS | PARITY_CASES unchanged (6); `cargo test -p wright-opy` OK |
| REQ-006 (per-fixture N-level records via v1 normalizer; v1-gates.py untouched) | PASS | `target/m11-nlevel.json`; v1-gates.py FIXTURES unchanged; gates 6/6 |
| REQ-007 (committed gap inventory, human + machine-readable, six classes, snapshot + as_of_commit) | PASS | this document + `m11-gap-inventory.json` companion |
| REQ-008 (no feature implementation; only harness/fixture/test-registry/doc changes; suites green) | PASS | no `crates/` changes in `45df86d..HEAD`; all suites green |
| REQ-009 (bounded doc-drift: language-services rename contract; opy-hir-v1 name_span) | PASS | `2ecc024` touches only `docs/language-services.md` + `docs/hir/opy-hir-v1.md`; rename text matches `wright-language/src/service.rs` (`RenameEdit`/`TargetSpan`) and `wright-driver/src/edit.rs` (`input_identity`, whole-word `rename_occurrences`); `name_span` normalization in `differential.rs` |

## Misclassification risks and flags

- **zencopter primary (class 4)**: the native frontend fails on the *same*
  construct (`"""`); do not read class 4 as "oracle-only". The docstring is
  also evidence of a newer-OverPy syntax that 9.7.10 rejects — unverified
  against a newer OverPy.
- **ow1-emulator / 6v6-adjustments primary (class 4)**: the native `\`
  line-continuation coverage gap exists independently of the oracle status;
  class 4 names the parity blocker, not the absence of a native gap.
- **meipocalypse dict literals**: OverPy 9.7.10's acceptance of the dict
  literal is *inferred* from the ENOENT surfacing after line 223 was parsed,
  not proven by a completed compile.
- **Matrix gap**: the support-matrix deferred section names settings blocks
  and `@` directives but not `\` continuations, `++`, dict literals, or
  `"""` docstrings; those classifications rest on "outside the declared
  surface" + the explicit `lex-error`/`parse-error` codes, not on a named
  deferred item.
- **Span defect**: included-file lex-errors are emitted with the including
  file's path (see above); not classified as class 3 by the strict
  definition, but it is a real diagnostic-quality defect with a repro.
- **No class 1 and no class 5 findings**: no new fixture achieves parity,
  and no fixture was found to be an intentional documented difference.
- **Evidence asymmetry**: for every fixture the native evidence boundary is
  much narrower than the oracle's (native stops at the first unsupported
  token; the oracle either compiles the full file or fails late). Any
  statement about fixture coverage must be read against the per-fixture
  evidence boundaries above.
