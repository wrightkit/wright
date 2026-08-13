# M11 inventory rebaseline: settings/oracle decisions and updated classifications

related_issue: "#82"
freshness: snapshot
as_of_commit: 096891f
status: verification
owner: QA

**This snapshot supersedes [`m11-gap-inventory.md`](m11-gap-inventory.md)
(as_of_commit `2ecc024`) and [`m11-inventory-reassessment.md`](m11-inventory-reassessment.md)
(as_of_commit `f5fe337`).** It re-baselines the phase-1 gap inventory after
the PM decisions recorded in the #82 decision record: settings enter v1 as
typed native support (batch issue #86), the oracle reference policy adopts
strategy 1 (ADR-0007), and the per-fixture classifications are updated
accordingly. It records the Track A (settings boundary) and Track B (oracle
version sensitivity) findings with independent QA verification; it does not
propose implementation batches or priorities (PM owns those).

Reference evidence read for this snapshot: the four committed settings-bearing
`oracle.json` snapshots (pixelart, santa, broken-weapons, client-to-server);
the two prior inventory docs; `m11-settings-free-candidates.md`; the
Architect-owned Track A/Track B investigation docs
(`m11-settings-boundary-investigation.md`,
`m11-oracle-version-investigation.md`, `docs/adr/0007-reference-pinning-policy.md` —
all present and committed in the history at write time: ADR-0007 + Track B at
`9e1408b`, Track A at `017b693`); `PARITY_CASES` in
`crates/wright-opy/tests/differential.rs`; the native frontend behavior at
`096891f` (binary `target/debug/wright`, `--profile compat -f json`).

## Track A — settings surface and decision

### Settings surface matrix (QA re-derived)

Nine of the 14 evidence programs carry a top-of-file `settings { ... }`
block: the four committed phase-1 fixtures (pixelart, santa,
broken-weapons, client-to-server) and the five candidates (parabola,
skirmish_elim, crosshair, inputhud, lucioball_all_heroes). The other five
(cronch, meipocalypse, zencopter, ow1-emulator, 6v6-adjustments) contain no
`settings` keyword. QA re-derived the surface by parsing the JSONC blocks of
all nine (comment-stripping and trailing-comma normalization were needed;
string contents are preserved):

| Claim (PM/Track A record) | QA measurement at 096891f | Verdict |
| --- | --- | --- |
| JSONC format (quoted keys, trailing commas, comments) | all nine blocks are JSON-style with quoted keys; trailing commas present (lucioball list, inputhud); `\n` escapes in strings | confirmed |
| Always top-of-file, first construct | block starts at source lines: skirmish_elim 1, parabola 4, inputhud 4, lucioball 6, pixelart 7, santa 7, crosshair 7, broken-weapons 7, client-to-server 14 — always before any declaration/rule | confirmed |
| 34-key union across main/lobby/gamemodes.<mode>/gamemodes.general/heroes.<team>.<hero> | **33 distinct leaf keys** under strict leaf-only counting; the 34th is the dual-context `general` group key (`gamemodes.general` in 6 blocks, `heroes.<team>.general` in 3), counted once — the Track A convention. Full leaf union: `abilityCooldown%`, `combatUltGen%`, `description`, `disabledHeroes`, `enableAbility1`, `enableAbility2`, `enableCompetitiveRules`, `enableHeroSwitching`, `enablePrimaryFire`, `enableRandomHeroes`, `enableSecondaryFire`, `enableSpawningWithUlt`, `enabled`, `enabledHeroes`, `enabledMaps`, `ffaSlots`, `gameLengthInSec`, `gamemodeStartTrigger`, `healingReceived%`, `health%`, `healthPackRespawnTime%`, `heroLimit`, `modeName`, `passiveUltGen%`, `primaryFireKb%`, `resetPlayersAfterGoalScored`, `respawnTime%`, `roleLimit`, `scoreLeadToWin`, `scoreToWin`, `spawnHealthPacks`, `team1Slots`, `team2Slots` | confirmed (33 + `general` = 34) |
| 4 string enums | `heroLimit` (observed value set {"off"}), `roleLimit` ({"off","2OfEachRolePerTeam"}), `gamemodeStartTrigger` ({"immediately"}), `spawnHealthPacks` ({"enabled"}) | confirmed (key set; observed value sets are single/two-member) |
| 8 mode names | 7 named mode groups in the JSONC (`assault`, `control`, `escort`, `hybrid`, `skirmish`, `ffa`, `lucioball`) + the `general` group = 8 mode groups; the oracle additionally maps `ffa` → emitted mode `Deathmatch` | confirmed (as mode groups incl. `general`) |
| 5 map ids | `blizzWorldWinter`, `busanStadiumClassic`, `estadioDasRas`, `kingsRowWinter`, `workshopIsland` (all in `enabledMaps` lists) | confirmed, exact |
| 10 hero ids | `ashe`, `bastion`, `dva`, `doomfist`, `echo`, `hammond`, `mei`, `moira`, `reinhardt`, `zenyatta` (9 in broken-weapons `disabledHeroes` + `mei` from santa's `enabledHeroes`/hero group; reinhardt also in lucioball) | confirmed, exact |
| 2 team scopes | `allTeams` (santa, broken-weapons, lucioball) and `team1` (skirmish_elim); no `team2` in the corpus | confirmed |
| int/float/bool/enum/list/object value types | int (`respawnTime%` 30, `ffaSlots`), float (`health%: 266.4`), bool (`enablePrimaryFire: false`, `enableCompetitiveRules`), enum strings (4 keys above), list (`enabledMaps`/`enabledHeroes`/`disabledHeroes`), object (all group levels) | confirmed |
| nesting ≤ 5 | deepest path `settings > heroes > allTeams > mei > health%` = 5; no block exceeds 5 | confirmed |
| en-US only | oracle runs pinned `--language en-US`; source strings in other scripts (e.g. inputhud's Chinese strings) are payload content, not settings semantics | confirmed |

### Oracle emission finding (QA re-derived)

The pinned oracle **translates** the JSONC block into a native Workshop
`settings` section at the top of the artifact, **before** `variables`. QA
verified this in all four committed oracle-success snapshots (each begins
`settings\n{` at byte 0) and re-ran the three candidate programs
(parabola, crosshair, inputhud — exit 0 each; parabola's output has
`settings` at byte 0 followed by `variables`; crosshair and inputhud have
`settings` at byte 0 and no `variables` section because they declare no
variables).

Cited emission strings, verified verbatim in the snapshots:

| Evidence | Where |
| --- | --- |
| `Respawn Time Scalar: 30%` | santa, client-to-server |
| `Hero Limit: Off` | santa, client-to-server |
| `disabled <Mode>` (`disabled Assault`, `disabled Control`) | broken-weapons |
| `enabled maps { ... }` (incl. empty `enabled maps { }` for empty arrays) | pixelart, santa, client-to-server |
| localized names | santa: `Max FFA Players: 6` (ffaSlots), `King's Row Winter`, `Mei`, `Primary Fire`, `Cryo-Freeze`, `Ice Wall`, `Health: 266.4%`, `Ultimate Generation - Passive Blizzard: 0%`; broken-weapons: `Limit Roles: 2 Of Each Role Per Team`, `Competitive Rules: On`; client-to-server: `Mode Name:` |

The emission is a key→localized-name/scalar translation of fixture-evidenced
data, not a preserved blob and not a dropped construct: 7 oracle-success
settings-bearing programs (4 committed snapshots + parabola/crosshair/inputhud
observed in the #85 pre-check and re-confirmed here) all emit a settings
section. Only the 4 committed snapshots are byte-level evidence in the repo;
the 3 candidate emissions are ephemeral oracle runs (no committed snapshot),
which is why the #86 emission table records provenance per LICENSE-BOUNDARY
and needs Architect review.

### Direction decision and D3 rejection rationale

- **Direction 4 adopted (batch issue #86)**: typed native settings — scoped
  settings lexing (no global `{`/`}` tokens; meipocalypse's dict-literal
  `lex-error` at `meipocalypse.opy:223:37` must stay as-is), a v1-additive
  HIR settings node (settings are semantically inert: the oracle artifact's
  settings section interacts with no variables/subroutines/rules), en-US
  emission before `variables`, structured rejection of out-of-corpus
  surface, adapter lockstep, and the emission table as fixture-evidenced
  data. Contracts S/D/N.
- **D3 rejection (architectural)**: an opaque pass-through blob is not valid
  Workshop settings syntax — the reference *translates* settings into the
  artifact's `settings` section, so a blob can neither be compared against
  nor emitted as the oracle artifact. Drop (D2) is equally rejected: silent
  semantic loss vs. the oracle artifact. Both break the S/N comparison
  contract.

## Track B — oracle version sensitivity and reference policy

- **9.7.10 vs 9.7.13 sensitivity matrix**: 14/14 programs byte-identical
  (8 success / 6 failure); the delta between the versions is 102 changed
  lines in the bundled `overpy.js`, all hero/settings schema data — zero
  lexer/parser/preprocessor/compiler logic changes. A newer primary or
  secondary reference unlocks **zero additional oracle compile evidence**
  on the current corpus (Architect-owned measurement; 9.7.13 was not
  re-installed by QA — see evidence boundary).
- **Strategy 1 adopted (ADR-0007)**: keep 9.7.10 as the sole pinned primary
  reference, version-exact and content-pinned, changed only on demonstrated
  behavioral need; second reference only on demonstrated divergence. No
  oracle migration issue is created. ADR-0007 is filed (`docs/adr/0007-reference-pinning-policy.md`,
  Accepted 2026-08-14, committed at `9e1408b`).
- **Version flags resolved**: the five "inconclusive on version" flags from
  the phase-1 inventory are now recorded as **stable-across-versions
  rejections** (rejected identically by 9.7.10 and 9.7.13): `#!obfuscate`
  (skirmish_elim.opy:48), `lucioball` OW2 gamemode validation
  (lucioball_all_heroes.opy:39), `"""` docstrings (heli.opy:38),
  `_hp_reset` custom member (custom_hp.opy:31), inline `if` without `else`
  (arena.opy:94). None is a 9.7.10 quirk.
- **npm gitHead-lag provenance nuance (QA verified)**: the npm registry
  `gitHead` for 9.7.10 is `1e268895…` — identical to
  `oracle-metadata.json` — but that commit is the **parent** of the
  content commit `889d974` ("v9.7.10 - add dmon", the actual v9.7.10
  content). The registry metadata lags the content by one release (the
  release pipeline publishes before committing); the integrity hash
  `sha512-oX17nauJcPTaKIrRFY/rD0Rl8atqFUVv9Hg2TKH+A68/fC8+ZO344Mkd1A/Y0oOVp1hr5tktMBjzMEDDnMEYUw==`
  pins the actual content. Byte-level equivalence of the installed bundle
  to `889d974` is not independently re-verifiable from the installed
  tarball (built bundle without a provenance marker).

## Updated per-fixture classifications (superseding the phase-1 table)

Classification basis per the PM decision record, with QA-verified evidence.
"No class 3 anywhere" — no supported-surface construct fails on any program;
this remains true after the settings decision.

| Program | Previous class | Updated class | Evidence (QA-verified) |
| --- | --- | --- | --- |
| overpy-pixelart | 2 (settings block) | **PENDING** batch-verification N-level candidate | settings first blocker at `pixelart.opy:7`; body (185,541 B) contains no tracked unsupported construct (no `++`, `\` continuation, dict `{`, `"""`, `#!obfuscate`, `in` operator, conditional expression, comprehension, bytes literal) — the only candidate body verified clean. Oracle success. Verification = #86 AC-1/AC-2 |
| overpy-broken-weapons | 2 (settings block) | **PENDING** batch-verification N-level candidate | settings first blocker at `broken_weapons.opy:7`; oracle success; **body not clean**: first post-settings blocker is `not in [Hero.HANZO, ...]` at `broken_weapons.opy:107` — the binary `in`/`not in` operator is not accepted by the native parser (see finding below) |
| overpy-client-to-server | 2 (settings block) | **PENDING** batch-verification N-level candidate | settings first blocker at `clientToServer.opy:14`; oracle success; **body not clean**: chained inline conditional expression at `clientToServer.opy:55` (`X if C else Y`, native `parse-error expected ')'`) |
| overpy-santa | 2 (settings block) | **2 — explicit unsupported surface (class 2)**; settings part pending batch | settings first blocker at `santa.opy:7`; first post-settings blocker is not `++`: the `#!define createChimneyEffect(idx)` macro at `santa.opy:210` contains a list comprehension + inline conditional, first expanded at `santa.opy:213` (`parse-error expected ']'`); `++` at `santa.opy:304` follows it |
| parabola (candidate) | settings-blocked (unclassified) | **2 — explicit unsupported surface (class 2)**; settings part pending batch | settings block 4–19; first post-settings blocker is the inline conditional expression + bytes literal at `parabola.opy:46` (`parse-error expected ')'`); `attacker.textIndex++` at `parabola.opy:70` follows |
| crosshair (candidate) | settings-blocked (unclassified) | **2 — explicit unsupported surface (class 2)**; settings part pending batch | settings block 7–22; first post-settings blocker is the bytes literal `b" \n"` at `crosshair.opy:31` + implicit adjacent-string concatenation (macro identifiers `FSP`/`NSP` juxtaposed with string `"⊙"` at `crosshair.opy:34–41`) — confirmed |
| skirmish_elim (candidate) | settings-blocked (unclassified) | **4 — reference-oracle limitation**, with documented S divergence | settings block 1–30; oracle exit 1 `Unknown preprocessor directive '#!obfuscate'` at `skirmish_elim.opy:48` (stable-across-versions rejection per Track B). S divergence: native would accept the settings block (post-batch) where the pinned oracle rejects the program; no parity reference exists |
| lucioball_all_heroes (candidate) | settings-blocked (unclassified) | **4 — reference-oracle limitation**, with documented S divergence | settings block 6–43; oracle exit 1 `The gamemode 'lucioball' is not available in OW2` at `lucioball_all_heroes.opy:39` (OW2 validation, deliberately not replicated in v1 — documented oracle D divergence). Same S-divergence structure |
| overpy-meipocalypse | 4 (primary) / 2 (dict literal) | **stands unchanged** | dict literal `lex-error` at `meipocalypse.opy:223:37`; oracle ENOENT (`generateWalls.js`); unaffected by the settings decision |
| ow1-emulator | 4 (primary) / 2 (`\`) | **stands unchanged** | `\` continuation `common/env.opy:7:66` (corrected path); oracle `Found 'if', but no 'else'` at `arena.opy:94` |
| 6v6-adjustments | 4 (primary) / 2 (`\`) | **stands unchanged** | `\` continuation `constants/adj_constants.opy:8:85`; oracle `_hp_reset` at `custom_hp.opy:31` |
| overpy-zencopter | 4 (primary) / 2 (`"""`) | **stands unchanged** | `"""` docstring `heli.opy:38:22`; oracle `Invalid content before string` at 38:17 |
| overpy-cronch | 2 (primary) / 2 (`@Name`, adapter) | **stands unchanged** | `++` `parse-error` at `cronch.opy:32:21`; adapter rejects `@Name` at its boundary |
| zombies | 6 (metadata-only) | **unchanged** | no content redistributed; hash + acquisition record only |

### QA finding — `in`/`not in` as an expression operator is unsupported

The support-matrix operator list claims `in` (and `not`), but the native
parser recognizes `in` **only in `for` statement headers**
(`crates/wright-opy/src/parser.rs:706`, `is_ident("in")` in `parse_for`).
A binary/condition use fails: `@Condition x not in [1, 2]` →
`parse-error expected ']'`; `x = 1 in y` → `unknown-identifier 'in'`.
Corpus evidence contains `in` only in `for … in range(...)` headers; the
synthetic corpus never exercises expression-position `in`. This is the
reason broken-weapons' body is not clean (first `not in` at :107), and it
also makes the matrix's operator-list claim (which presents `in` as a
binary operator) inaccurate as written.

### QA finding — "fully consumable" marginal-unlock claims are not supported by body scans

The PM record ("pixelart, broken-weapons, client-to-server, inputhud — body
greps clean") and the Architect marginal-unlock table (santa in place of
inputhud) both assert four full candidates. QA's independent body scans
verify only **pixelart** as clean. Measured first post-settings blockers:

| Program | First post-settings blocker (QA-verified, native) |
| --- | --- |
| pixelart | none found (clean body) |
| broken-weapons | `not in [Hero.HANZO, Hero.BRIGITTE]` at `broken_weapons.opy:107` (`parse-error expected ']'`) |
| client-to-server | chained inline conditional at `clientToServer.opy:55` (`parse-error expected ')'`) |
| inputhud | comprehension `… for b in [Button.ABILITY_1, …]` at `inputhud.opy:83` (`parse-error expected ']'`); body clean up to :82 |
| santa | comprehension + inline conditional inside `#!define createChimneyEffect`, first expansion at `santa.opy:213` (`parse-error expected ']'`) |
| parabola | inline conditional + bytes literal at `parabola.opy:46` (`parse-error expected ')'`) |
| crosshair | bytes literal `b" \n"` at `crosshair.opy:31` + adjacent-string juxtaposition at 34–41 |

The PENDING classifications above are retained exactly as decided (their
verification is #86 AC-1/AC-2 by definition); the measured blockers are
recorded here so the batch verification and the final gate have concrete
targets and the expected-baseline statement below is read with them.

## Parity baseline statement

- **Today: 6 parity rows** — 5 synthetic (`basic-rule`, `control-flow`,
  `declarations-rules`, `expressions-values`, `preprocessing`) +
  `real-world/overpy-cake` (`PARITY_CASES` in
  `crates/wright-opy/tests/differential.rs`; unchanged at 096891f; no new
  parity row has been added since `45df86d`).
- **Expected post-batch baseline (contingent on #86 verification, no row
  counts promised)**: pixelart / broken-weapons / client-to-server
  (committed fixtures) and inputhud (acquisition in #86) are the named
  N-level/parity candidates; parabola and crosshair are partial candidates.
  The baseline is verification-contingent: QA's body scans above place only
  pixelart's full-consumability on the current surface; the named
  candidates' post-settings blockers (`in` at :107, conditional at :55,
  comprehension at :83) are outside #86's non-goal-bounded scope, so their
  parity status is open until the batch ACs adjudicate. skirmish_elim and
  lucioball are documented S divergences (no parity row). **No parity row
  will be forced.**

## Open evidence gaps (what the final gate still needs)

1. **#86 verification outcome** — settings lexing/emission, HIR node,
   structured `settings-*` rejection, adapter lockstep, and the
   N-level/parity rows or documented divergence records per AC-1..7; the
   post-settings blocker measurements above are inputs to AC-1/AC-2.
2. **ADR-0007 filed** — committed at `9e1408b` (Accepted 2026-08-14);
   re-checked at the final gate against the batch commit.
3. **Refreshed parity rows** — per the contingent baseline; zero forced.
4. **Re-confirmed no class 3** — unchanged by this rebaseline; the final
   gate re-runs the suites (native, adapter, oracle, differential, v1
   gates) at the batch commit.

## Evidence boundary

- **Track B 9.7.13 side not re-run by QA**: 9.7.13 is not installed in this
  tree; the sensitivity matrix and the byte-identical claims are
  Architect-owned measurements recorded in
  `m11-oracle-version-investigation.md` (integrity
  `sha512-dCPsfwXHfV59BrzQSWW0Ow9X7b8SvkzllkNMTNRbVzzxJxHPuFX9P8pcYbZ5rcl9LVJ8N1JUlG+3y2qempfTqA==`),
  adopted via ADR-0007. QA re-verified the 9.7.10 side (8 success / 6
  failure across the 14 programs; settings emission; oracle diagnostics).
- **npm content-pin byte-equivalence**: the installed 9.7.10 bundle carries
  no commit marker; content equivalence to `889d974` rests on the integrity
  hash and the Track B comparison, not on an in-tree check.
- **Candidate emissions**: parabola/crosshair/inputhud settings emissions
  were re-observed in this snapshot's runs (exit 0, settings section at
  byte 0) but are not committed snapshots; only the 4 committed
  oracle.json files are in-tree evidence.
- **Post-settings first-failure measurements**: obtained with minimal
  repros matching the fixture constructs (native frontend at 096891f), not
  by compiling the full fixtures past their settings blocks (the native
  frontend still rejects settings at `lex-error` today); final positions
  are subject to #86 verification.
- **zencopter/`"""` and meipocalypse dict acceptance**: oracle-side claims
  rest on the recorded failure positions and the Track B matrix, not on a
  completed compile of those constructs.
- **Newer-OverPy constructs beyond 9.7.13**: whether any post-9.7.13
  OverPy adds the five rejected constructs is unanswerable by a fixed
  snapshot (per ADR-0007, re-run the matrix at each evaluation point).

## REQ/AC compliance notes

- **Settings decision (Track A)**: emission finding and surface matrix
  verified; D4/D3 rationale recorded; the 33-leaf + `general` key-union
  convention matches the Track A record.
- **Oracle policy (Track B)**: strategy 1 and the resolved version flags
  recorded; ADR-0007 exists (in progress in the commit history until the
  parallel agent commits it); gitHead-lag nuance verified structurally.
- **#82 gate status**: unchanged — NOT ready; this snapshot is evidence
  item (3) of the pending gate list; items (1), (2), (4) remain open.
- **Inventory REQ-008 spirit**: the `2ecc024..096891f` range changes only
  driver diagnostics, driver tests, and docs — no feature implementation
  in the inventory's own commits.
