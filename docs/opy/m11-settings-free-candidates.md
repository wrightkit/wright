# M11 settings-free candidate pre-check evidence (#85)

related_issue: "#85"
freshness: snapshot
as_of_commit: 5d234dc
status: raw-evidence (Engineer)
owner: QA (classification)

This is the Engineer-owned raw evidence for the issue #85 pre-check of the PM
shortlist of settings-free parity candidates. It records only what was
observed; it does **not** classify findings into the six gap classes of
[`m11-gap-inventory.md`](m11-gap-inventory.md) (QA owns classification).

## Result

**0/5 candidates pass the pre-check gate (settings-free + compilable by the
pinned oracle).** All five have a top-of-file custom-game-settings block
(`settings {`), the same deferred construct that blocked 4/9 phase-1
fixtures, so **no candidate was acquired and no parity row was added**. Two
candidates additionally fail the pinned oracle itself, which is evidence for
the later reference-strategy investigation.

## Reference identity

- Sources: `Zezombye/overpy` at commit
  `eea67adbcf6926c4004e35e25ab4be072624a44e` (GPL-3.0-only), the same pinned
  commit as the phase-1 overpy fixtures; bytes taken from the corpus cache
  (`target/corpus-cache/Zezombye__overpy__eea67ad…/examples/`), same
  provenance pattern as `scripts/acquire-corpus.py`.
- Oracle: pinned overpy 9.7.10 (`compatibility/oracle/package.json`, gitHead
  `1e2688954302a402d076944b46db07efb14d7b61`).

## Pre-check method

Oracle (run in `compatibility/oracle/`):

```
pnpm exec overpy compile --input <examples-dir>/<name> --output /tmp/out.txt \
  --language en-US --root <examples-dir> --main-file <name>
```

Native (raw first-failure, read-only run against the same tree):

```
target/debug/wright compile <examples-dir>/<name> --profile compat -f json \
  --root <examples-dir>
```

Adapter (read-only run against the same tree):

```
node adapter/bin/wright-adapter.js --input <examples-dir>/<name> --root \
  <examples-dir> --main-file <name> --output /tmp/out.json
```

## Per-candidate pre-check

| Candidate | Size | SHA-256 | Settings block | Oracle exit | Oracle status | Native first failure | Adapter |
| --- | --- | --- | --- | --- | --- | --- | --- |
| parabola.opy | 3035 B | `2dce50c3…484940` | lines 4–19 (of 71) | 0 | success (2 warnings) | `lex-error` `unexpected character '{'` at 4:10 | `unsupported` (settings boundary) |
| skirmish_elim.opy | 4958 B | `a4e72e1f…ceec74` | lines 1–30 (of 160) | 1 | failure | `lex-error` `unexpected character '{'` at 1:10 | `parse` (`#!obfuscate` 48:1) |
| crosshair.opy | 1404 B | `d2c22b59…ed8f8f` | lines 7–22 (of 49) | 0 | success | `lex-error` `unexpected character '{'` at 7:10 | `unsupported` (settings boundary) |
| inputhud.opy | 2602 B | `ae7a0e00…eefc1` | lines 4–20 (of 94) | 0 | success | `lex-error` `unexpected character '{'` at 4:10 | `unsupported` (settings boundary) |
| lucioball_all_heroes.opy | 6114 B | `3e482e96…454840` | lines 6–43 (of 223) | 1 | failure | `lex-error` `unexpected character '{'` at 6:10 | `parse` (lucioball gamemode 39:34) |

First 20 lines of every candidate open with comments and then `settings {`,
except skirmish_elim, which opens directly with `settings {` at line 1
(parabola:4, skirmish_elim:1, crosshair:7, inputhud:4,
lucioball_all_heroes:6); the blocks carry `main` description and
`gamemodes` configuration. None of the five is settings-free.

## Oracle diagnostics (verbatim)

- parabola.opy (exit 0): two warnings —
  `Chasing a variable to 9999 is not enough because a custom game can last
  up to 16200 seconds. Use Math.INFINITY or 99999. (w_chase_9999)` at
  `line 43, col 28`; `.startHealingOverTime(..., 9999) is not enough because
  a custom game can last up to 16200 seconds. Use Math.INFINITY or 99999.
  (w_9999)` at `line 57, col 32`.
- crosshair.opy, inputhud.opy (exit 0): no diagnostics.
- skirmish_elim.opy (exit 1):
  `Error: Unknown preprocessor directive '#!obfuscate'` at `line 48, col 1`.
  `#!obfuscate` is a preprocessor directive outside the native supported
  surface (support-matrix lists `#!include`/`#!define`/`#!undef` only).
- lucioball_all_heroes.opy (exit 1):
  `Error: The gamemode 'lucioball' is not available in OW2` at
  `line 39, col 34` (inside the settings block; OverPy's own OW2 validation).

## First meaningful gap per candidate

All five: the top-of-file custom-game-settings block `settings {`, the
support-matrix deferred item ("Rule `disabled` markers and
custom-game-settings blocks"); native stops at `lex-error` `unexpected
character '{'` on the block's opening brace (locations in the table above),
and the adapter rejects the three oracle-successful candidates at its corpus
boundary. The two oracle-failing candidates have an additional oracle-side
blocker (`#!obfuscate`, lucioball gamemode validation) that the pinned
9.7.10 reference itself rejects; whether a newer OverPy accepts either is
**unverified** (inconclusive on version).

## Decision and scope effects

- No candidate passed the pre-check, so none was acquired: no fixture
  directory, no `fixture.json`, no `oracle.json`, no corpus-manifest record,
  and no `scripts/acquire-corpus.py` FIXTURES extension.
- No parity row was added to `crates/wright-opy/tests/differential.rs`, and
  no adapter success/failure fixture was registered in
  `adapter/test/adapter.test.js`.
- `scripts/m11-inventory.py` was not extended: it iterates acquired fixture
  directories with `oracle.json` snapshots, which do not exist for these
  candidates; this document is the inventory record for them.
- No language feature was implemented and no pinned reference was changed.
