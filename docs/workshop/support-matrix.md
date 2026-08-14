# Workshop Support Matrix

Status: accepted baseline — living Workshop support matrix
Scope: the evidence-backed Workshop feature and localization surface Wright
supports for native localized Workshop input/output

This document inventories the Workshop surface evidenced by the compatibility
corpus, records the localization matrix, and specifies the supported feature
set of the `wright-workshop` frontend and emitter.

## Evidence sources

| Source | Provenance | Use |
| --- | --- | --- |
| `compatibility/fixtures/**/oracle.json` `compile.workshop` (en-US) | Pinned OverPy 9.7.10 reference output, GPL-3.0-only, recorded per fixture | The primary surface inventory below. |
| Pinned OverPy 9.7.10 package surface (`actionKw`, `valueFuncKw`, `constantValues`, `eventKw`) | Observed for scoping only; not copied | Confirms spellings and value domains beyond the corpus. |
| Oracle emission test `--language zh-CN` | Generated reference output (GPL) | Shows localized samples are generatable for review; not committed as catalog data. |

Every count below is derived from the checked-in corpus snapshots, so the
matrix is regenerable from the repository alone.

## Feature × corpus matrix

Sections list the surface observed in the en-US corpus Workshop text.

### Variables
- `variables { global: <index>: <name> }` — control-flow, expressions-values.
- `variables { player: <index>: <name> }` — declarations-rules.
- Explicit indices and names are both evidenced.

### Subroutines
- `subroutines { <index>: <name> }` — declarations-rules.

### Rules
- `rule ("<name>") { event { ... } conditions { ... } actions { ... } }` — all
  fixtures; conditions/actions blocks are optional.

### Events
- `Ongoing - Global;` — all fixtures with `@Event global`.
- `Ongoing - Each Player;` plus team/slot lines (`All; All;`) —
  declarations-rules.
- `Subroutine; <name>;` — declarations-rules (def bodies).

### Conditions
- `Has Spawned(Event Player) == True;` — declarations-rules.

### Actions
Disable Inspector Recording, Set Global Variable, Modify Global Variable,
Set Player Variable, Call Subroutine, If, Else If, Else, End, For Global
Variable, While, Wait, Create Beam Effect, Create HUD Text, Play Effect.
Receiver-call actions (emitted for native `.opy` `eventPlayer.<method>(...)` /
player-receiver forms, `synthetic/receiver-calls` fixture): Set Move Speed,
Set Max Health, Set Player Health, Teleport, Set Aim Speed, Set Gravity,
Set Damage Dealt, Set Damage Received, Set Ultimate Charge.

### Values
Add, Subtract, Multiply, Divide, Compare (with inline operators `==`, `>`,
`<`, `<=`, `>=`, `!=`), And, Or, Not, Count Of, Absolute Value, Array,
Vector, Custom String, Value In Array, Mapped Array, First Of, String
Replace, String Slice, String Split, All Players, Random Real, Random Value
In Array, Has Spawned.
Receiver-call values (emitted for native `.opy` receiver-method forms in
conditions and arguments, `synthetic/receiver-calls` fixture): Is Alive,
Position Of, Health.
Literals: numbers (signed), strings (escaped), `True`/`False`,
`Global.<name>` references, `Event Player`.

### Enums and constants
- Color: `Color(Yellow)`, `Color(White)`, `Color(Red)`, `Color(Orange)`.
- Beam: `Grapple Beam`, `Good Beam`.
- Dynamic Effect: `Bad Explosion`.
- Wait: `Ignore Condition`.
- Vector constant: `Up`.
- Hud Position: `Left`.
- Hud Reevaluation: `Visible To Sort Order String and Color`, `Visible To
  and String`.
- Spec Visibility: `Default Visibility`.
- Team: `All Teams` (inside `All Players(All Teams)`).

### Settings and extensions
- Emitted from native `.opy` `settings { ... }` blocks into the top-of-file
  Workshop `settings` section. Reparsing emitted settings in the Workshop
  frontend is a non-goal (a `.ws` decompiler is out of scope).

## Localization matrix

| Locale | Evidence | Status |
| --- | --- | --- |
| `en-US` | Full corpus Workshop text | Supported. |
| `zh-CN` (and other OverPy-supported locales) | Oracle can emit localized reference output (verified for `zh-CN`); no committed samples yet | Investigation. Samples and catalog aliases require provenance/licensing review ([`docs/licensing.md`](../licensing.md), [ADR-0004](../adr/0004-overpy-licensing-boundary.md)). |

Localization coverage is therefore explicit: `en-US` is the primary supported
locale. Cross-locale behavior is not inferred from English-only fixtures.

## Catalog and IR design

1. Builtin references (`Action::Call`/`Value::Call` `name`) use canonical
   identifiers (`crates/wright-workshop/src/catalog/data/catalog.json`) so no
   locale-specific spelling becomes semantic identity.
2. Enum spellings ("Grapple Beam", "Ignore Condition", …) map to
   locale-independent canonical identities.
3. Supported events cover `Global`, `EachPlayer`, `Subroutine`.
4. Comparison operators appear inline in `Compare(...)`; parsing accepts
   operator tokens as arguments.
5. Catalog data updates follow the deterministic pipeline in
   [`catalog-pipeline.md`](catalog-pipeline.md).

## Supported surface and priorities

- **Supported (corpus-evidenced, en-US):** variables, subroutines, rules,
  the three corpus events, conditions, corpus actions/values/enums above,
  deterministic en-US emission, same-locale round-trip, and analyzer integration.
- **Deferred (data/licensing gated):** additional client locales,
  cross-locale equivalence, additional events.
- **Explicitly out of scope:** `.opy` reconstruction from Workshop text,
  OverPy decompiler architecture, editor/browser integrations.
