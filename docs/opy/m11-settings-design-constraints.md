# M11 settings design constraints (#86)

related_issue: "#86"
freshness: snapshot
as_of_commit: 8b782ad2f40ea27fee0e103b5160accb4fa8b21e
status: settled (Architect, pre-implementation)

Implementation constraints settled by the Architect for issue #86. The Engineer
must treat these as settled; deviations require a new Architect pass. Ground
truth for the emission table: the pinned oracle 9.7.10 en-US output of the 7
oracle-success settings programs (4 committed snapshots plus parabola,
crosshair, inputhud oracle runs acquired in the batch).

## 1. Scoped settings lexing (no global braces)

- The lexer never gains global `{`/`}` tokens; meipocalypse's dict literal
  keeps failing as `lex-error "unexpected character '{'"` at
  `meipocalypse.opy:223:37` (lexer.rs punctuation arm stays untouched).
- Mechanism: pre-lex text extraction in the preprocessor; the settings block
  never enters the token stream.
- New `crates/wright-opy/src/settings.rs`:
  - `SettingsBlock { text, span, keyword_span }` (text = raw JSONC between
    braces; span = whole block; keyword_span = diagnostic anchor).
  - `find_blocks(text, file_id)` — logical-line scan (skips blanks and `#`/
    `/* */` comments per lexer rules) collecting every line whose first token
    is `settings`. Rules: 0 blocks -> Ok(vec![]); first block must be the
    first non-comment construct (`settings-placement` otherwise); after
    `settings` require `{` (`settings "file"` form -> `settings-invalid`);
    second/later block -> `settings-placement` at its keyword span; brace
    matching respects strings and nesting; unterminated -> `settings-invalid`.
  - `sanitize_for_lex(text, block)` — copy of main text with every char of the
    block region replaced by `' '`, newlines preserved, so tokens after the
    block keep exact original line/col. No lexer change at all.
- `preprocess.rs`: call `find_blocks(main_text, 0)` before `lex`; lex the
  sanitized text; `Preprocessed` gains `settings: Option<SettingsBlock>`;
  `include()` runs `find_blocks` on included text -> settings in included
  files rejected with `settings-placement` (file_id of the included file).
- `lib.rs compile_with_overlay_outcome`: after successful parse, if
  `preprocessed.settings` present, `settings::parse_block(&block)` ->
  `program.settings = Some(...)`; errors flow through the existing error path
  (registry retained for span mapping).
- `parser.rs` unchanged except `cst::Program` gains `settings: Option<...>`
  (default None). Top-level placement is enforced by `find_blocks`, not the
  parser.

## 2. Typed HIR node (v1-additive, wire 1.1.0)

- No existing node/field changes; wire version becomes `1.1.0` in both
  producers (`wright-opy/src/lower.rs` PROTOCOL_VERSION and
  `adapter/lib/adapter.js`). `check_envelope` gates only the major, so v1
  consumers accept it.
- Wire shape (`wright-core/src/hir/types.rs`, existing conventions:
  `#[serde(default)]`, `skip_serializing_if = "Option::is_none"`, never null):

```rust
pub struct Settings { pub span: Option<Span>, #[serde(default)] pub children: Vec<SettingsNode> }

#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SettingsNode {
  Group  { name: String, #[serde(default)] children: Vec<SettingsNode>, span: Option<Span> },
  Number { name: String, value: f64, span: Option<Span> },
  Bool   { name: String, value: bool, span: Option<Span> },
  String { name: String, value: String, span: Option<Span> },
  List   { name: String, #[serde(default)] elements: Vec<SettingsListElement>, span: Option<Span> },
}

pub struct SettingsListElement { pub value: String, pub span: Option<Span> }
```

`Program` gains `#[serde(default, skip_serializing_if = "Option::is_none")]
pub settings: Option<Settings>`.
- No `Enum` leaf kind: enum-ness and list domains are table data at validation
  and emission, never wire data (lets the JS adapter parse JSONC
  syntactically without table duplication). All list elements in the corpus
  are strings.
- Leaves carry spans at key/value-token granularity (every node, every list
  element, and the block).
- JSONC grammar in `parse_block`: quoted keys, `"`/`'` strings with `\`
  escapes, int/float numbers (f64; source spelling discarded), `true`/`false`,
  arrays, nested objects, trailing commas in objects and arrays, duplicate-key
  rejection, non-object root rejection, `gamemodes` group required (mirror
  OverPy parse behavior).
- Domain validation lives at ONE site: `wright_core::hir::validate::validate_program`
  against the table in wright-ir. The native path reaches it via
  `session.rs::load_hir` calling `protocol.validate()` before `to_ir()` (first
  time the native path is protocol-validated; both check and compile share
  `load()`, so no check/compile split-brain). Add settings to
  `check_unknown_kinds`; span checks reuse `check_span`; keys non-empty; list
  elements strings; domain checks; gamemodes presence. `dump.rs` gains a
  settings section. Re-export types in `hir/mod.rs`.

## 3. WIR carrier + emission

- New `crates/wright-ir/src/settings/mod.rs`: non-serde tree types
  (`Settings`, `SettingsNode`, `SettingsListElement`, `wright_ir::source::Span`)
  + `table.rs`. wright-ir is the only neutral layer shared by wright-core and
  wright-workshop (workshop does not see wright-core).
- `wright_ir::hir::Program` gains `settings: Option<Settings>`; convert.rs
  copies protocol -> internal (span mapping via existing self.span).
- `wir::Program` gains `settings: Option<Settings>` (+ Default; no struct
  literals exist). `lower::lower` copies inertly (file ids align 1:1). No
  semantics in WIR/analyzer/LSP (wright-analyzer/wright-language keep zero
  settings references).
- `wir/validate.rs`: recursive span checks for the settings tree only; no
  semantic checks in WIR validation.
- Emission (`wright-workshop/src/emitter.rs`): in `run()`, before the
  `variables` block, emit `settings { ... }` when present (indent per `line()`,
  blank line after; section order settings, variables, subroutines, rules).
  Table-driven: top-level renderers (main/lobby = `Name: value`; gamemodes =
  `modes { <Mode> { ... } }` with `disabled ` prefix for `enabled: false`;
  heroes = `heroes { <Team> { ... } }`), localized names, list elements one
  per line (empty list -> empty braces block), bools On/Off, numbers via
  `format_number`, percent keys append `%`, strings via `escape_string`.
- `.ws` settings input stays rejected (ws parser unchanged); the emitter's
  "reparses to equivalent WIR" claim gets a documented settings exception;
  roundtrip fixture list unchanged + boundary test asserting settings-bearing
  emission is rejected by the ws parser.
- wright-transform must preserve the settings carrier (transforms rebuild
  programs; carrier must pass through under every profile).

## 4. Emission table (fixture-evidenced data)

- Home: `crates/wright-ir/src/settings/table.rs`. Mandatory provenance header
  (observed from pinned oracle 9.7.10 en-US output of the 7 oracle-success
  settings programs at commit `eea67ad`; observed-behavior data, not copied
  OverPy source).
- Shape: exact-path entries with wildcard slots (mode/team/hero), literal
  per-domain member maps, no generic transforms:

```
TableEntry { path: Vec<PathPart>, workshop_name: &'static str, kind: KeyKind }
KeyKind = String | Bool | Number | Percent | Enum{domain} | ListMap | ListHero
```

- Evidenced entries (Engineer must verify each against snapshot text):
  main.description=Description(string), main.modeName=Mode Name(string),
  lobby.ffaSlots=Max FFA Players(number), gamemodes.<mode>.enabled
  (bool; false -> `disabled <Mode>`, only false evidenced),
  gamemodes.<mode>.enabledMaps=enabled maps(ListMap),
  gamemodes.<mode>.roleLimit=Limit Roles(Enum roleLimit:
  2OfEachRolePerTeam -> 2 Of Each Role Per Team),
  gamemodes.<mode>.enableCompetitiveRules=Competitive Rules(bool),
  gamemodes.general.heroLimit=Hero Limit(Enum: off -> Off),
  gamemodes.general.respawnTime%=Respawn Time Scalar(Percent),
  gamemodes.general.enableHeroSwitching=Allow Hero Switching(bool),
  gamemodes.general.enableRandomHeroes=Respawn As Random Hero(bool),
  heroes.<team>.enabledHeroes=enabled heroes(ListHero),
  heroes.<team>.disabledHeroes=disabled heroes(ListHero),
  heroes.allTeams.mei.enablePrimaryFire=Primary Fire(bool),
  heroes.allTeams.mei.enableSecondaryFire=Secondary Fire(bool),
  heroes.allTeams.mei.enableAbility1=Cryo-Freeze(bool),
  heroes.allTeams.mei.enableAbility2=Ice Wall(bool),
  heroes.allTeams.mei.health%=Health(Percent),
  heroes.allTeams.mei.passiveUltGen%=Ultimate Generation - Passive Blizzard(Percent),
  heroes.allTeams.mei.combatUltGen%=Ultimate Generation - Combat Blizzard(Percent).
- Slot sets (evidenced): modes {assault, control, escort, hybrid, skirmish,
  ffa} (per-key subsets), teams {allTeams}, heroes {mei} config groups + 10
  ListHero names (ashe, bastion, dva, doomfist, echo, moira, reinhardt,
  hammond, zenyatta, mei); mode names {assault->Assault, control->Control,
  escort->Escort, hybrid->Hybrid, skirmish->Skirmish, ffa->Deathmatch,
  general->General}; maps {workshopIsland->Workshop Island,
  kingsRowWinter->King's Row Winter}; team {allTeams->General}.
- Keys outside the table (e.g. team1Slots, scoreToWin, gamemodeStartTrigger,
  spawnHealthPacks, healthPackRespawnTime%, abilityCooldown%, healingReceived%,
  primaryFireKb%, enableSpawningWithUlt, resetPlayersAfterGoalScored,
  scoreLeadToWin, gameLengthInSec, heroes.<team>.general, roleLimit under
  general) -> `settings-unknown-key` (only evidenced in oracle-failing
  programs; corpus-bounded).
- Diagnostic codes (docs/cli.md): `settings-invalid` and `settings-placement`
  (frontend stage, wright-opy); `settings-unknown-key` and
  `settings-unknown-value` (validation stage, wright-core). All carry
  leaf/keyword spans. No emission-stage settings codes (validated HIR is
  always emittable).

## 5. Adapter lockstep

- OverPy consumes settings inside `OverPyCompiler.parseLines`; the
  `__settings__` AST is NOT pushed into astRules; the compiled section lands
  in `compiler.compiledCustomGameSettings` — the adapter cannot recover it
  after parseLines, so it must pre-extract.
- `adapter/lib/driver.js`: remove the `compiledCustomGameSettings !== ""`
  gate; before `compiler.parseLines(content)` run a JS extraction mirroring
  find_blocks (top-level keyword scan, brace match, second-block error,
  computed line/col base); pass the block through to convertProgram.
- `adapter/lib/adapter.js`: convertProgram gains a settings mapping (small JS
  JSONC parser -> the §2 wire shape with adapter-computed spans); the
  `__settings__` branch stays as defensive rejection. PROTOCOL_VERSION 1.1.0.
- Tests: replace the unsupported-settings mini-fixture with a
  settings-success mini-fixture; generate `adapter/fixtures/synthetic/
  settings.json`; SUCCESS_FIXTURES gains synthetic/settings.

## 6. Bounded parity boundary

- New PARITY_CASES row `("synthetic/settings", "adapter/fixtures/synthetic/
  settings.json")`; fixture `compatibility/fixtures/synthetic/settings/`
  (settings block using only evidenced keys + one minimal supported rule;
  expectedStatus success; oracle.json). Differential normalization unchanged
  (spans stripped recursively; typed tree survives). Real-world settings
  fixtures are NOT added to PARITY_CASES. v1-gates FIXTURES stays unchanged in
  this batch.

## 7. Verification checklist (Architect final pass)

1. No silent settings loss (emission iff settings present; settings-free
   programs emit none). 2. No opaque pass-through (typed nodes; no raw-text
   settings field). 3. HIR v1-additive (diff shows only additions; existing
   adapter fixtures still parse; 1.1.0 both producers). 4. Table provenance
   header + entries match snapshot regions. 5. Zero settings references in
   wright-analyzer/wright-language; WIR validate span-only; ws parser still
   rejects settings. 6. meipocalypse lex-error unchanged; lexer diff empty.
   7. Unknown key fails check AND compile with same settings-unknown-key code
   and span. 8. Marginal unlock: pixelart/broken-weapons/client-to-server move
   past settings; santa stops at `++` :304. 9. Adapter gate removed, suite
   green, 4 committed oracle.json byte-unchanged. 10. Roundtrip amendment
   present; settings emission rejected by ws parser.

## Open rendering details (Engineer to resolve against acquired snapshots)

- `\n` escapes inside settings string values (inputhud description) — match
  the acquired oracle snapshot.
- `enabled: true` rendering — only false evidenced; true -> no prefix
  (documented).
- Locale: table is en-US-only; emitter renders table names for any requested
  locale; all N-level claims are en-US (documented).
