# M5 Workshop Production Support Matrix

Status: reviewable audit for milestone M5 (v0.2), issue #28
Scope: the evidence-backed Workshop feature and localization surface Wright
needs for native localized Workshop input/output

This document is the audit deliverable of [#28]. It inventories the Workshop
surface evidenced by the compatibility corpus, records the localization
matrix, identifies gaps in the current IR/catalog, and prioritizes the M5
feature set. It does not prescribe implementation details; later M5 issues
use it as acceptance evidence.

[#28]: https://github.com/wrightkit/wright/issues/28

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

### Values
Add, Subtract, Multiply, Divide, Compare (with inline operators `==`, `>`,
`<`, `<=`, `>=`, `!=`), And, Or, Not, Count Of, Absolute Value, Array,
Vector, Custom String, Value In Array, Mapped Array, First Of, String
Replace, String Slice, String Split, All Players, Random Real, Random Value
In Array, Has Spawned. Literals: numbers (signed), strings (escaped),
`True`/`False`, `Global.<name>` references, `Event Player`.

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
- Not present in the corpus. Deferred (no evidence).

## Localization matrix

| Locale | Evidence | Status |
| --- | --- | --- |
| `en-US` | Full corpus Workshop text | Supported (v0.2). |
| `zh-CN` (and other OverPy-supported locales) | Oracle can emit localized reference output (verified for `zh-CN`); no committed samples yet | Investigation. Samples and any catalog aliases require provenance/licensing review ([#30], ADR-0004). |

Localization coverage is therefore explicit: only `en-US` is claimed in v0.2.
Cross-locale behavior is not inferred from English-only fixtures.

## Gaps in the current IR/catalog

Identified without prescribing implementation:

1. WIR builtin references (`Action::Call`/`Value::Call` `name`) are plain
   strings; native parsing needs a canonical-identity contract so no
   locale-specific spelling becomes semantic identity.
2. Enum spellings ("Grapple Beam", "Ignore Condition", …) have no
   locale-independent identity table; round-trip requires one.
3. WIR events cover `Global`, `EachPlayer`, `Subroutine`; the corpus needs no
   more, so expansion is deferred until evidence demands it.
4. The corpus has no settings/extension blocks; WIR has no representation and
   none is added without evidence.
5. Comparison operators appear inline in `Compare(...)`; parsing must accept
   operator tokens as arguments.

## Prioritized M5 feature set

- **P0 (supported, corpus-evidenced, en-US):** variables, subroutines, rules,
  the three corpus events, conditions, the corpus actions/values/enums above,
  deterministic en-US emission, same-locale round-trip, and integration with
  the M4 analyzer stack.
- **P1 (deferred, blocked on data/licensing):** additional client locales,
  cross-locale equivalence, settings/extensions, additional events.
- **Explicitly out of v0.2:** `.opy` reconstruction from Workshop text,
  OverPy decompiler architecture, editor/browser integrations.

## Follow-up evidence (noted, not edited)

`COMPATIBILITY.md` and `ARCHITECTURE.md` still describe the workshop
normalizer, language matrix, and tooling surface as open questions; updating
those documents is a separately approved follow-up, not part of this issue.
