# Proactive OPY Compatibility Baseline

Status: accepted baseline (planning) — proactive OPY compatibility baseline (#106)
Scope: forward-looking, tiered inventory of the OPY language surface against the
pinned OverPy 9.7.10 reference, classifying every category by implementation
tier and by support dimension; records the #104/#105 residual evidence

This document is the planning counterpart to
[`support-matrix.md`](support-matrix.md): the support matrix records the
corpus-evidenced surface Wright supports **today**, while this baseline records
how the remaining surface is **tiered and sequenced**. A construct is not
called supported merely because it parses; each row states parse, semantic,
compilation, tooling/analysis, and reference coverage separately.

The reference identity is the pinned OverPy 9.7.10 content (`889d974`); see
[`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md)
for provenance. Evidence claims in this document were verified against the
pinned oracle and Wright `main` @ `582c269` (post-#104/#105); the category-6/7/8/9
rows and the residual-evidence table below were refreshed by the #109
manifest implementation.

## Tier taxonomy

| Tier | Meaning |
| --- | --- |
| `baseline-supported` | Implemented and corpus/reference-evidenced; part of the declared supported surface |
| `baseline-planned` | Stable, high-fan-out, systematically implementable; contract is discoverable and reference-testable; not yet implemented |
| `evidence-prioritized` | Complex or broad feature with clear tooling value; corpus/consumer evidence determines ordering |
| `legacy-quirk/demand-driven` | Rare historical quirks, upstream bugs, obsolete aliases, scripting hooks; implemented only when the declared compatibility target requires them |
| `reference-limited/inconclusive` | Cannot be resolved from the pinned reference; needs a demonstrated need, a pin change, or further investigation |

## Support dimensions

For each category the following dimensions are distinguished:

* **Parse** — accepted by the native frontend grammar;
* **Semantic resolution** — resolved to a meaningful HIR/semantic value
  (names, members, enums, call semantics);
* **Compilation** — standalone compile/emission through the Workshop backend
  succeeds with reference-equivalent semantics;
* **Tooling/analysis** — `check`/`analyze`/`lint`/`inspect` and language
  services can operate on the construct;
* **Reference coverage** — oracle probes/fixtures validate the behavior.

## Category inventory

| # | Category | Tier | Parse | Sem | Comp | Tooling | Ref |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | **Expression/postfix/member/call grammar** — operators and precedence, `[]` indexing, `.` member, calls, `++`/`--`, `del`, `in`/`not in`, hex `0x` | `baseline-supported` for the corpus subset (operators, indexing, calls, member/call); sub-forms below | ✅ corpus | ✅ | ✅ (compat profile) | ✅ | ✅ differential |
| 1a | `switch`/`case`/`default`, `do…while`, `not in`, `0x` hex literals | `evidence-prioritized` | ❌ (rejected, documented) | ❌ | ❌ | ❌ | ✅ oracle probes |
| 1b | String modifiers (`f`/`w`/`l`/`b`/`c`/`t`), dict literals, list comprehensions, `lambda` beyond `.map`/`sorted` | `legacy-quirk/demand-driven` (dicts, modifiers) / `evidence-prioritized` (comprehensions) | ❌ | ❌ | ❌ | ❌ | ✅ oracle probes |
| 2 | **Declarations** — `globalvar`/`playervar` (index + initializer forms), `subroutine`, `enum`, `macro` constants (incl. member constants) | `baseline-supported` | ✅ | ✅ | ✅ | ✅ | ✅ |
| 3 | **Assignments & control flow** — `=`, augmented (`+= … **=`, `min=`, `max=`), `if`/`elif`/`else`, `for … in range(...)`, `while`, `pass` | `baseline-supported` | ✅ | ✅ | ✅ | ✅ | ✅ |
| 4 | **Rule directives & annotations** — `@Event`, `@Condition`, bare `@Team`/`@Slot`, rule name, event defaults (`global`, `all` team/player) | `baseline-supported` (bare forms) | ✅ | ✅ | ✅ | ✅ | ✅ |
| 4a | `@Team`/`@Slot` with arguments, `@Name`, `@Hero`, `@Disabled`, `@Delimiter`, `@NewPage`, `@SuppressWarnings` | `evidence-prioritized` | ❌ | ❌ | ❌ | ❌ | ✅ oracle probes |
| 5 | **Preprocessing/include/macro** — `#!include`, `#!define` (object- and function-like), `#!undef`, include cycle detection | `baseline-supported` | ✅ | ✅ | ✅ | ✅ | ✅ |
| 5a | `#!mainFile`, `#!allowMacroRedeclaration`, `#!optimize*`/`#!replace0By*` family, `#!translations`, `#!rulePrefix*`, `__script__` JS hooks | `legacy-quirk/demand-driven` | ❌ | ❌ | ❌ | ❌ | partial |
| 6 | **Builtin actions & values (generic)** — the 225 action / 267 value Workshop surface | `baseline-supported` for the manifest-declared evidence surface (chaseOverTime, isGameInProgress, getPlayersInRadius, worldVector, the corpus call surface); the full surface stays **`baseline-planned`** | ✅ | ✅ | ✅ (catalog subset) | ✅ | ✅ probes |
| 7 | **Receiver/member functions** — `eventPlayer.setMoveSpeed(100)`, `eventPlayer.isAlive()`, variable receivers | `baseline-supported` for the manifest-declared member surface (receiver categories, explicit-arg signatures); **`baseline-planned`** for the full member surface | ✅ | ✅ | ✅ (catalog subset) | ✅ | ✅ |
| 8 | **Builtin enum/constant domains** — 46 upstream domains (incl. `Hero`/`Map`/`Gamemode` literals) | `baseline-supported` for the manifest-declared domains (reference-validated member lists; replaces the former `KNOWN_ENUMS` subset); **`baseline-planned`** (systematic) for the full surface | ✅ (declared domains) | ✅ | partial | partial | ✅ probes |
| 9 | **Aliases** — old function names (`stopChasingVariable`→`stopChasing`, `getCurrentHero`→`getHero`, `hasStatusEffect`→`hasStatus`, …), hero renames (`MCCREE`→`CASSIDY`), `ChaseReeval` contextual alias | `baseline-supported` for the three manifest-declared non-contextual aliases; the remaining alias surface stays `legacy-quirk/demand-driven`; `ChaseReeval` stays `evidence-prioritized` (blocked on named-argument `chase`) | ✅ (declared) | ✅ | ❌ (emission spellings not catalog-covered) | ✅ | ✅ |
| 10 | **Modules** — `random.{randint,uniform,choice,shuffle}` | `baseline-supported` (corpus: `random.uniform`, `random.choice`) | ✅ | ✅ | ✅ | ✅ | ✅ |
| 11 | **Named/keyword arguments** — `chase(A, B, rate=30, …)`, macro defaults, `raycast` `include=`/`exclude=` forms | `evidence-prioritized` | ❌ | ❌ | ❌ | ❌ | ✅ |
| 12 | **Settings/content metadata** — `settings { … }` blocks | `baseline-supported` (JSONC subset + emission table) | ✅ | ✅ | ✅ | ✅ | ✅ |
| 12a | `settings "file"`, richer settings expressions, hero/map/ability content beyond the pin | `legacy-quirk/demand-driven` / `reference-limited` | ❌/partial | ❌ | ❌ | ❌ | partial (data newer than pin unavailable per ADR-0007) |
| 13 | **Source identity & diagnostics** — structured, source-located frontend errors, `wright-result/v1` | `baseline-supported` | ✅ | ✅ | — | ✅ | ✅ S/D |

## Residual #104/#105 evidence (classified, not implemented)

Verified against the pinned oracle and Wright `main` @ `582c269`. Each item is
classified with the tier it belongs to; none is a per-symbol implementation
request. Items marked **implemented (#109)** now resolve through the OPY
semantic compatibility manifest (`crates/wright-opy/src/manifest`).

| Evidence | Oracle 9.7.10 | Wright (`582c269`) | Classification |
| --- | --- | --- | --- |
| **Bare playervar receiver** — `A = B.C` (declared playervar member on a player-valued receiver) | accept (`__playerVar__`) | reject `unsupported-member` | `baseline-planned` (receiver/member semantics + playervar member resolution, category 7) — unchanged |
| **Value member as statement** — `B.isAlive()` on its own line | **reject** ("Expected an action, but got … a value") | accept | **implemented (#109)**: rejected with `value-in-action-position` (the #109 contract supersedes the earlier intentional-divergence candidate note; recorded as a reviewed difference in the manifest probe set) |
| **Generic action gap** — `chaseOverTime(A, 0, 30, ChaseTimeReeval.NONE)` | accept (warning recorded) | check ✅; **compile rejects** `unknown-action 'chaseOverTime'` | **implemented (#109)**: resolves through the manifest and compiles (catalog spelling `Chase Global Variable Over Time`); regression: `synthetic/chase-condition-agentlab` + probe `chase-over-time` |
| **Generic value gap** — `@Condition isGameInProgress() == true` | accept | check ✅; **compile rejects** `unknown-value 'isGameInProgress'` | **implemented (#109)**: resolves through the manifest and compiles (catalog spelling `Is Game In Progress`); probe `is-game-in-progress` |
| **Member value/signature gap** — `getPlayersInRadius(...).setStatusEffect(eventPlayer, 30)` | **reject** (arity: `.setStatusEffect` needs `player, assister, status, duration`) | parse ✅; compile rejects | **implemented (#109)**: rejected at resolution with `invalid-arity` (member signatures in the manifest); probe `invalid-arity-member` |
| **Enum-gated members** — `eventPlayer.setInvisibility(Invis.ALL)`, `eventPlayer.getThrottle()`, `worldVector(...)` (args typed `Invis`/`Transform`) | accept | reject `unsupported-member`/`unknown-value` | **implemented (#109)**: member entries + enum domains (`Invis`, `Status`, `Transform`) in the manifest, compiled via new catalog spellings; probes `enum-gated-members`, `builtin-enums` |
| **Named arguments / `ChaseReeval` alias** — `chase(A, 10, rate=2, ChaseReeval.NONE)` | accept (contextual alias resolution) | reject (named args unsupported) | `evidence-prioritized` (category 11); `ChaseReeval` stays out of the enum table until keyword-argument `chase` exists — unchanged |
| **Ambiguous Workshop enum spelling** — `ChaseTimeReeval.NONE`, `ChaseRateReeval.NONE`, and `Invis.NONE` all emit as bare `None` | — | context-sensitive resolution (#111): `Chase Global Variable Over Time(..., None)` reparses to `ChaseTimeReeval.NONE`, `Set Invisible(..., None)` to `Invis.NONE`; context-free/wrong-context `None` (e.g. `Set Global Variable(g, None)`) still fails with the structured `Unsupported` ambiguity | resolved for the signature-pinned contexts (`reference-limited` only for unpinned `ChaseRateReeval` and bare context-free uses; documented in `docs/workshop/support-matrix.md`) |
| **Constant-0 canonicalization** — `globalvar A = 0` drops the initializer; `= 5`/`= 0.0` preserved via the Initialize rule; `globalvar A 0` is an explicit index | canonical | identical **only with `--profile compat`** (default `off` drops initializers) | intentional profile difference; pending PM decision on the default compile profile — unchanged |
| **Diagnostic provenance limitation** — generic unresolved action/value errors surface at emission (`unknown-action`/`unknown-value`, frontend stage) rather than at semantic resolution | — | — | **implemented (#109)**: identity, position, arity, receiver-category, and enum-domain errors surface during semantic resolution with source locations (`unknown-action`, `unknown-value`, `unknown-member`, `invalid-arity`, `invalid-receiver`, `enum-domain-mismatch`, `action-in-value-position`, `value-in-action-position`, `invalid-call-context`, `invalid-iterable`) |

## Boundaries

* **No per-symbol issues.** These evidence items are grouped into semantic
  categories; none justifies a one-symbol implementation issue.
* **No OSTW work.** Nothing here begins OSTW compatibility.
* **#96 stays deferred.** This baseline is compile-time language-compatibility
  metadata, versioned with the pinned reference identity. The runtime content
  registry, extension boundaries, and independent version identities of #96
  are not triggered by this investigation (see
  [`compat-manifest-spec.md`](compat-manifest-spec.md) for the boundary).
* **Proposed decomposition.** The bounded child-issue categories (C1–C5) and
  their ordering are recorded in the planning comment on issue #106 and are
  pending PM review; they are not created as implementation issues.

## Related documents

* [`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md) — pinned reference identity and provenance
* [`support-matrix.md`](support-matrix.md) — corpus-evidenced current support
* [`compat-manifest-spec.md`](compat-manifest-spec.md) — machine-readable manifest specification (implemented, #109; data in `crates/wright-opy/src/manifest/`)
* [`docs/compatibility.md`](../compatibility.md) — S/D/N/E framework
* [ADR-0007](../adr/0007-reference-pinning-policy.md), [ADR-0004](../adr/0004-overpy-licensing-boundary.md)
