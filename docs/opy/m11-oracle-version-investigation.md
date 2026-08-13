# M11 oracle version investigation (Track B)

related_issue: "#82"
freshness: snapshot
as_of_commit: 096891f
status: verification
owner: Architect

This is the Architect-owned version-sensitivity investigation behind
[ADR-0007](../adr/0007-reference-pinning-policy.md). It records what was
measured with isolated, temporary OverPy installs; it does not change the
pinned oracle (overpy@9.7.10, content commit `889d974`).

## Reference identity

- **Pinned oracle**: `compatibility/oracle/package.json` pins `overpy@9.7.10`;
  npm tarball content is byte-identical to git commit `889d974` ("v9.7.10 -
  add dmon", tag `v9.7.10`). The npm-registry `gitHead` field records
  `1e268895` (the `v9.7.9` tag commit) because the release pipeline publishes
  before committing; the integrity hash
  `sha512-oX17nauJcPTaKIrRFY/rD0Rl8atqFUVv9Hg2TKH+A68/fC8+ZO344Mkd1A/Y0oOVp1hr5tktMBjzMEDDnMEYUw==`
  pins the actual content.
- **Newer reference measured**: `overpy@9.7.13` (registry `latest`, published
  2026-08-12T15:26:40Z, integrity
  `sha512-dCPsfwXHfV59BrzQSWW0Ow9X7b8SvkzllkNMTNRbVzzxJxHPuFX9P8pcYbZ5rcl9LVJ8N1JUlG+3y2qempfTqA==`).
  Its tarball content is byte-identical to git master HEAD commit `d854bf0`
  ("v9.7.13 - add enableSecondaryFire for domina, vendetta, mizuki"), so one
  install covers both "registry latest" and "repo HEAD".
- Intermediate releases `9.7.11`/`9.7.12` (2026-08-12, same-day batch) differ
  only in hero-settings data; no separate install was needed for the matrix.
- Source-tree check: all evidence files (`skirmish_elim.opy`,
  `lucioball_all_heroes.opy`, `Zencopter/heli.opy`, `parabola.opy`, the
  examples directory listing) are byte-identical between the pinned examples
  commit `eea67ad` and the newer commits.

## What changed between the versions

`git compare 1e268895…d854bf0` and a line-level diff of the bundled
`overpy.js` (102 changed lines, all data) show the only differences are hero/
settings schema data (`customGameSettingsSchema.json`, `src/data/*.ts`,
decompiler result fixtures, `types.d.ts` deletion). **No lexer, parser,
preprocessor, or compiler logic changed.** Consequently the newest OverPy
still cannot compile three of its own examples (`skirmish_elim.opy`,
`lucioball_all_heroes.opy`, `Zencopter/heli.opy`).

## Version-sensitivity matrix

All runs: pinned 9.7.10 vs 9.7.13, direct CLI invocation, en-US, exit codes +
full logs compared; Workshop outputs byte-compared for every success.

| Construct (source) | 9.7.10 | 9.7.13 | Classification |
| --- | --- | --- | --- |
| `#!obfuscate` (skirmish_elim.opy:48; minimal repro) | reject `Unknown preprocessor directive '#!obfuscate'` 48:1 | identical | stable-across-versions (rejected) |
| `lucioball` gamemode (lucioball_all_heroes.opy:39; settings) | reject `The gamemode 'lucioball' is not available in OW2` 39:34 | identical | stable-across-versions (rejected); `onlyInOw1` data unchanged |
| `"""` docstrings (heli.opy:38; minimal repro) | reject `Invalid content before string: 'arena'` 38:17 | identical | stable-across-versions (rejected) |
| `_hp_reset` custom member (custom_hp.opy:31; minimal repro) | reject `Unknown member '_hp_reset' of 'eventPlayer'` 31:17 | identical | stable-across-versions (rejected) |
| inline `if` without `else` (arena.opy:94; minimal repro) | reject `Found 'if', but no 'else'` | identical | stable-across-versions (rejected) |
| settings blocks (parabola/crosshair/inputhud/pixelart/santa/broken-weapons/client-to-server) | accept (exit 0) | accept, byte-identical output | stable-across-versions (accepted) |
| `\` line continuation (env.opy:7 / adj_constants.opy:8; minimal repro) | accept (exit 0) | accept, byte-identical output | stable-across-versions (accepted) |
| dict literals (meipocalypse.opy:223; minimal repro with access) | accept (exit 0) | accept, byte-identical output | stable-across-versions (accepted) |
| `++` postfix (cronch.opy:32; minimal repro) | accept (exit 0) | accept, byte-identical output | stable-across-versions (accepted) |
| `--` decrement (meipocalypse; minimal repro) | accept (exit 0) | accept, byte-identical output | stable-across-versions (accepted) |
| w_9999 / w_chase_9999 warnings (parabola.opy:43/57) | 2 warnings | identical | stable-across-versions |
| meipocalypse ENOENT (full example tree) | ENOENT `generateBarricadeRules.js` | identical ENOENT | environment-or-artifact-dependent (helper not committed upstream at either commit) |
| ow1-emulator / 6v6-adjustments / zencopter / santa full closures | per-fixture outcome | full logs byte-identical | stable-across-versions |

## Evidence unlocked per reference strategy

All 14 evidence programs (9 phase-1 fixtures + 5 settings-free candidates):

| Program | Oracle status 9.7.10 | Oracle status 9.7.13 |
| --- | --- | --- |
| pixelart, santa, cronch, broken-weapons, client-to-server, parabola, crosshair, inputhud (8) | success | success, byte-identical output |
| meipocalypse | failure (artifact) | failure (artifact, identical) |
| zencopter, ow1-emulator, 6v6-adjustments, skirmish_elim, lucioball_all_heroes (5) | failure | failure (identical diagnostics) |

**A newer primary or secondary reference unlocks 0 additional oracle compile
evidence across all 14 programs.** The five "inconclusive on version" flags
from the M11 inventory are resolved: each is a stable-across-versions
rejection, not a 9.7.10 quirk. What a newer reference *would* unlock is only
hero-settings schema data for future programs using the newest OW2 heroes —
no current fixture uses those. Reference choice affects only reference-side
evidence: native-side gaps (settings blocks at lex, `\`, `++`, dict literals,
`"""`), the adapter boundary (settings, `@Name`), and the meipocalypse
artifact gap are version-independent.

## Strategy evaluation

1. **Keep 9.7.10 sole pinned** — zero churn across `oracle-metadata.json`,
   lockfiles, `oracle.json`, and adapter; every recorded claim keeps its exact
   meaning; the sensitivity matrix documents stability instead of silently
   re-baselining. Risk: none for current evidence.
2. **9.7.10 baseline + second newer reference** — needs a second install, a
   second metadata record, and a runner change (`run_oracle.py` and
   `oracle.json` schemaVersion 1 are single-reference); doubles CI installs
   and snapshot surface with zero evidence gain on the current corpus.
3. **Re-baseline primary to 9.7.13** — 16 `oracle.json` identity blocks + 8
   adapter fixture `frontend` stamps + 2 lockfiles change (review-only, since
   content is byte-identical); differential parity unaffected (generator
   stripped before comparison); historical "9.7.10" claims would need
   re-derivation or a mapping statement; "latest" moves within days.

## Decision

ADR-0007 (Accepted 2026-08-14): keep 9.7.10 as the sole pinned primary
reference, version-exact and content-pinned, changed only on demonstrated
behavioral need. Re-run this matrix at each evaluation point; do not
re-baseline on release recency. Details in
[`../adr/0007-reference-pinning-policy.md`](../adr/0007-reference-pinning-policy.md).

## Blocked questions

* Whether a post-9.7.13 OverPy adds the five rejected constructs — cannot be
  answered by a fixed snapshot; re-run the matrix to claim any new acceptance.
* Full meipocalypse dict-literal compile evidence — blocked by the missing
  `generateBarricadeRules.js` helper at both the pinned and HEAD commits;
  needs artifact reconstruction, not a version change.
* npm `gitHead` field semantics — the registry field lags content by one
  release for all 9.7.x versions; content must be verified by integrity hash
  and byte comparison, not trusted from the metadata field.
