# M11 settings boundary investigation (Track A)

related_issue: "#82"
freshness: snapshot
as_of_commit: 096891f
status: verification
owner: Architect

This is the Architect-owned Track A investigation of the `settings { ... }`
custom-game-settings surface, feeding [Issue #86](https://github.com/wrightkit/wright/issues/86)
(the adopted implementation issue) and the M11 settings decision. It records
what was measured against the pinned oracle and the committed corpus; it does
not change the oracle, the adapter, or the supported surface.

## Reference identity

- Oracle: pinned overpy 9.7.10 (content commit `889d974`, per
  [ADR-0007](../adr/0007-reference-pinning-policy.md) and
  [`m11-oracle-version-investigation.md`](m11-oracle-version-investigation.md)).
- Sources: OverPy examples at pinned commit `eea67ad` (candidates
  parabola/skirmish_elim/crosshair/inputhud/lucioball_all_heroes) and the
  committed M11 fixtures (pixelart, santa, broken-weapons, client-to-server).
- All hashes below match the QA records in
  [`m11-settings-free-candidates.md`](m11-settings-free-candidates.md) and
  [`m11-inventory-reassessment.md`](m11-inventory-reassessment.md).

## Settings surface matrix

Nine corpus programs carry a top-of-file `settings { ... }` block: the four
committed phase-1 fixtures (pixelart, santa, broken-weapons, client-to-server)
and the five settings-free candidates (parabola, skirmish_elim, crosshair,
inputhud, lucioball_all_heroes). Line ranges re-derived from the committed
sources and the pinned examples tree:

| Program | Block lines | Size (lines) | Block contents |
| --- | --- | --- | --- |
| parabola.opy | 4–19 | 16 | main description; gamemodes skirmish + general (heroLimit, respawnTime%) |
| skirmish_elim.opy | 1–30 | 30 | main description; gamemodes ffa (scoreToWin) + skirmish + general (healthPackRespawnTime%, roleLimit, spawnHealthPacks); heroes team1 general (abilityCooldown%, healingReceived%, health%) |
| crosshair.opy | 7–22 | 16 | main description; gamemodes skirmish + general |
| inputhud.opy | 4–20 | 17 | main modeName + description; gamemodes skirmish + general |
| lucioball_all_heroes.opy | 6–43 | 38 | main description; lobby (team1Slots, team2Slots); gamemodes lucioball (enabledMaps, gameLengthInSec, scoreToWin, resetPlayersAfterGoalScored, scoreLeadToWin) + general (heroLimit, respawnTime%, gamemodeStartTrigger); heroes allTeams reinhardt (primaryFireKb%) + general (abilityCooldown%, enableSpawningWithUlt, combatUltGen%) |
| pixelart.opy | 7–31 | 25 | gamemodes assault/control/escort/hybrid (enabledMaps, roleLimit) + skirmish (enabledMaps) |
| santa.opy | 7–40 | 34 | lobby (ffaSlots); gamemodes ffa (enabledMaps) + general (enableHeroSwitching, heroLimit, enableRandomHeroes, respawnTime%); heroes allTeams mei (enablePrimaryFire, enableSecondaryFire, enableAbility1/2, health%, passiveUltGen%, combatUltGen%) + enabledHeroes |
| broken_weapons.opy | 7–46 | 40 | main description; gamemodes assault/control (enabled, enableCompetitiveRules, roleLimit) + escort/hybrid (enableCompetitiveRules, roleLimit); heroes allTeams disabledHeroes |
| clientToServer.opy | 14–30 | 17 | main modeName + description; gamemodes skirmish (enabledMaps) + general (heroLimit, respawnTime%) |

**Key-value shapes and counts (union over the 9 blocks):**

* **34-key union** — 33 distinct leaf keys under strict leaf-only counting,
  plus the dual-context `general` group key (appears as `gamemodes.general`
  in 6 blocks and as `heroes.<team>.general` in 3), which the Track A matrix
  counts once. The union spans: ints (`ffaSlots`, `team1Slots`, `team2Slots`,
  `gameLengthInSec`, `scoreToWin`, `scoreLeadToWin`), floats (`health%:
  266.4`), bools (`enabled`, `enableCompetitiveRules`, `enableHeroSwitching`,
  `enableRandomHeroes`, `enableSpawningWithUlt`,
  `resetPlayersAfterGoalScored`), `%`-suffixed scalars (8 keys: `respawnTime%`,
  `healthPackRespawnTime%`, `health%`, `healingReceived%`, `abilityCooldown%`,
  `combatUltGen%`, `passiveUltGen%`, `primaryFireKb%`), strings
  (`description`, `modeName`, with `\n` escapes), and lists (`enabledMaps`,
  `enabledHeroes`, `disabledHeroes`).
* **4 enum keys** — string-valued enums: `heroLimit` ("off"),
  `roleLimit` ("off" / "2OfEachRolePerTeam"), `gamemodeStartTrigger`
  ("immediately"), `spawnHealthPacks` ("enabled").
* **8 mode groups** — `general` plus 7 named modes: `assault`, `control`,
  `escort`, `hybrid`, `skirmish`, `ffa`, `lucioball`.
* **5 maps** — `blizzWorldWinter`, `busanStadiumClassic`, `estadioDasRas`,
  `kingsRowWinter`, `workshopIsland` (all in `enabledMaps` lists).
* **10 heroes** — `ashe`, `bastion`, `dva`, `doomfist`, `echo`, `hammond`,
  `mei`, `moira`, `reinhardt`, `zenyatta` (in `enabledHeroes`/`disabledHeroes`
  lists and hero-config groups).
* **2 teams** — `allTeams` and `team1` group keys.
* **Nesting ≤ 5** — `settings > heroes > allTeams > mei > health%` is the
  deepest path.

## Decisive oracle emission finding

The pinned oracle **translates** the JSONC settings block into the Workshop
artifact's `settings` section, emitted **before `variables`** — it is not
dropped and not passed through as a blob. All four committed settings-bearing
oracle.json snapshots (status `success`) begin with `settings\n{` at byte 0
and place `variables` after the settings section. Quoted evidence
(`workshop` field, head of each snapshot, verbatim):

* **broken_weapons** — `settings { main { Description: "..." } modes {
  disabled Assault { Competitive Rules: On / Limit Roles: 2 Of Each Role Per
  Team } ... } heroes { General { disabled heroes { Ashe Bastion D.Va ... } } } }`
  — note `disabled <Mode>` and `disabled heroes` for `"enabled": false` and
  `disabledHeroes`, `Limit Roles: 2 Of Each Role Per Team` for
  `"roleLimit": "2OfEachRolePerTeam"`, `Competitive Rules: On` for
  `"enableCompetitiveRules": true`.
* **santa** — `settings { lobby { Max FFA Players: 6 } modes { Deathmatch {
  enabled maps { King's Row Winter } } General { Allow Hero Switching: Off /
  Hero Limit: Off / Respawn As Random Hero: On / Respawn Time Scalar: 30% } }
  heroes { General { Mei { Primary Fire: Off / Secondary Fire: Off /
  Cryo-Freeze: Off / Health: 266.4% / Ice Wall: Off / Ultimate Generation -
  Passive Blizzard: 0% / Ultimate Generation - Combat Blizzard: 0% } enabled
  heroes { Mei } } } }` — localized mode/map/hero names and scalar rendering
  (`ffaSlots` → `Max FFA Players: 6`, `respawnTime%` → `Respawn Time Scalar:
  30%`, `enableRandomHeroes` → `Respawn As Random Hero: On`).
* **pixelart** — `settings { modes { Assault { enabled maps { } / Limit Roles:
  2 Of Each Role Per Team } ... Skirmish { enabled maps { Workshop Island } } } }`
  — empty `enabledMaps` arrays still emit `enabled maps { }`.
* **clientToServer** — `settings { main { Mode Name: "https://workshop.codes/5AWEW"
  / Description: "..." } modes { Skirmish { enabled maps { Workshop Island } }
  General { Hero Limit: Off / Respawn Time Scalar: 30% } } }` — `modeName` →
  `Mode Name:`.

The emission uses en-US localized names (the sole pinned locale) and a
key→name/scalar-format translation that is **fixture-evidenced data**, not
copied OverPy source (see LICENSE-BOUNDARY observed-behavior policy; the
emission table for the implementation is built from these 4 committed
snapshots plus the parabola/crosshair/inputhud oracle runs).

## Semantic-inertness conclusion

The settings block is **semantically inert for the rule program**: it
configures the lobby/match (descriptions, slots, modes, maps, hero limits,
per-hero toggles) and produces no variables, subroutines, rules, conditions,
or actions. In the oracle artifact it is a self-contained first section; it
does not interact with `variables`, `subroutines`, or `rules`. Any HIR
representation therefore needs no analyzer, lowering, or Workshop-IR
interaction — the payload is carried and emitted, not interpreted.

## Architecture audit summary

* **No hidden representation.** Wright has no other path that carries
  settings: the native frontend stops at the block's opening `{`
  (`lex-error` `unexpected character '{'`), the adapter rejects at its
  boundary, and nothing in `wright-core`/`wright-ir` models it. The settings
  surface is entirely outside today's pipeline.
* **opy-hir-v1 §11 deferral.** [`../hir/opy-hir-v1.md`](../hir/opy-hir-v1.md)
  §11 lists "custom game settings blocks (`settings { ... }`)" as
  intentionally out of scope for v1 and rejected by the adapter as
  unsupported — the protocol-level record of the deferral that #86 amends
  (v1-additive, not v2).
* **Adapter dual rejection.** The adapter rejects settings twice: the driver
  refuses any program with `compiler.compiledCustomGameSettings !== ""`
  (`adapter/lib/driver.js:55`, `unsupported` code, "custom game settings
  blocks are outside the Opy HIR v1 corpus boundary"), and the
  `unsupported-settings` mini-fixture pins the rejection in the adapter test
  suite (`adapter/test/fixtures/unsupported-settings.opy`).

## Design-direction comparison (D1–D4)

| Direction | Description | Verdict |
| --- | --- | --- |
| D1 | Keep settings out of scope; reject at the boundary (status quo) | Rejected: leaves the dominant real-world blocker (9/14 programs) and produces no parity/N-level settings evidence |
| D2 | Parse and drop the settings block silently | Rejected: silent semantic loss — the oracle emits a `settings` section, so dropping diverges from the reference artifact |
| D3 | Opaque/lossless pass-through of the raw block | **Rejected (architecturally)**: the pinned oracle *translates* settings into Workshop `settings` syntax; a pass-through blob is not valid Workshop settings and cannot be compared against or emitted as the oracle artifact |
| D4 | **Typed native support (minimum correct boundary)**: scoped settings lexing, v1-additive HIR settings node, translated emission before `variables`, structured rejection of out-of-corpus surface | **Recommended and adopted** (Issue #86) |

D3's rejection is the decisive architectural point: because the reference
translates rather than preserves, the only correct boundary is a typed,
emittable representation (D4). Dropping (D2) or blob pass-through (D3) both
break the S/N comparison contract with the oracle.

## Marginal-unlock table

What scoped settings support unlocks across the 14 evidence programs
(9 phase-1 fixtures + 5 candidates):

| Category | Count | Programs |
| --- | --- | --- |
| Settings is the first native blocker | 9/14 | pixelart, santa, broken-weapons, client-to-server, parabola, skirmish_elim, crosshair, inputhud, lucioball_all_heroes |
| Full candidates (settings support alone removes the first blocker; oracle-success bodies) | 4/14 | pixelart, santa, broken-weapons, client-to-server |
| Partial (first failure moves past settings to a later construct) | 3/14 | parabola (`attacker.textIndex++` at :70), crosshair (bytes literals + string concat), inputhud (remaining expression surface; targeted for a full N-level row in #86 AC-5) |
| Oracle-blocked regardless (settings fine, oracle rejects later) | 2/14 | skirmish_elim (`#!obfuscate` 48:1), lucioball_all_heroes (OW2 gamemode validation 39:34) — recorded as documented S divergence |
| Untouched (settings not the first blocker) | 5/14 | cronch, meipocalypse, zencopter, ow1-emulator, 6v6-adjustments |

## Recommended sub-decisions

1. **Scoped raw-block lexing.** Recognize the top-of-file `settings` keyword
   plus the JSONC block (trailing commas, quoted keys, `\n` escapes) without
   introducing global `{`/`}` tokens — meipocalypse's dict-literal diagnostic
   must stay `lex-error` at `meipocalypse.opy:223:37`.
2. **en-US only.** The sole pinned locale; the emission table is en-US only.
3. **Emission before `variables`.** The translated section matches the oracle
   artifact shape and position.
4. **Structured rejection for out-of-corpus surface.** Unknown keys/values/
   enums → stable `settings-*` codes with spans, never silent drop; no OW2
   gamemode validation in v1 (lucioball remains a documented oracle D
   divergence).
5. **Emission table as fixture-evidenced data.** Key→localized-name/scalar
   mapping is built from the 7 oracle-success snapshots (4 committed +
   parabola/crosshair/inputhud) with provenance, per LICENSE-BOUNDARY.

## Blocked questions — resolution or routing

* **Emission-table provenance** — resolved as fixture-evidenced data from the
  oracle-success snapshots (sub-decision 5); routed to #86 scope item 3 for
  the acquisition of parabola/crosshair/inputhud.
* **v1-additive HIR amendment** — resolved: optional top-level `settings`
  payload in `wright/opy-hir` v1 (ordered group tree, typed leaf values,
  spans), consumer (driver/HIR) update in the same batch per protocol §7.1;
  routed to #86 scope item 2.
* **Adapter lockstep** — resolved: pinned adapter maps its settings AST to
  the HIR settings node, adapter fixtures updated, PARITY_CASES extension;
  routed to #86 scope item 5.
* **Settings-in-includes rejection** — resolved: no corpus evidence; reject
  with a stable code + span; routed to #86 scope item 1.
* **`.ws` input rejection** — resolved: decompiler is a non-goal; explicit
  rejection; routed to #86 non-goals.
* **OW2 gamemode validation** — deliberately not replicated in v1; oracle
  divergence documented; no question remains open for this milestone.
