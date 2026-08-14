# Issue #99 independent evidence review — candidate stability/performance lint rules

related_issue: "#99"
freshness: snapshot
as_of_commit: 7077fc862c1e21a9336f4072123cb0b1244b065e
status: review
owner: Architect

Independent evidence review for the M12 bounded, evidence-backed Workshop
stability rule set (#99), performed at commit
`7077fc862c1e21a9336f4072123cb0b1244b065e` with a clean working tree. All
corpus facts below were re-derived by running the current binaries
(`target/debug/wright`, verified fresh — no source file newer than the
binary) against the pinned `compatibility/fixtures/real-world/` fixtures and
by reading the pinned oracle artifacts line-by-line. Nothing is taken from an
Engineer or PM report on trust. This is an evidence review only; no code,
tests, fixtures, or other documents were modified. The snapshot records what
was observed; it is not a statement of current implementation status beyond
those observations.

During this review the PM-authored spec
[`docs/m12-issue99-spec.md`](m12-issue99-spec.md) (`SPEC-99-stability-rules`,
status `specified`, owner PM, freshness `live`) appeared in the working tree
(created while this review was in progress; the working tree was clean at
start). It selects exactly two new rules — `repeated-value` (`exact`,
`warning`) and `while-without-wait` (`static-indicator`, `warning`) — to
extend the existing three. Section 2 below re-derives the corpus facts those
selections rely on, including every specific occurrence count the spec cites,
and section 3 verifies the architecture constraints, including whether the
specified evidence-class assignments are sound.

The issue (#99) requires, per shipped rule: stable ID, default severity,
rationale, explicit evidence classification, known limitations,
positive/negative fixtures, and at least one real-project/corpus evidence
case where such evidence exists (otherwise an explicit documented reason why
synthetic validation suffices). Resource/cost claims must not overstate what
static analysis can prove; heuristic findings must be identifiable; defaults
must avoid high noise; existing rules must remain compatible.

## 1. Corpus audit and analyzable boundary

`compatibility/fixtures/real-world/` contains 13 pinned projects. For each,
the OPY path was exercised as `wright lint <entry .opy>` (entry per
`fixture.json` `source`, or `main.opy` where present) and the Workshop path
as `wright lint <extracted compile.workshop text>` (raw, then
settings-stripped). Workshop text was extracted from each `oracle.json`
`compile.workshop`; four fixtures have no workshop artifact because the
pinned oracle itself failed to compile them.

| Fixture | OPY path result | Workshop path result | Settings-stripped | Blocker |
| --- | --- | --- | --- | --- |
| overpy-cake | OK — 1 finding (`min-wait-loop` on `source.opy:54:5`) | OK — 1 finding (`min-wait-loop`, span 46:9–49:13) | n/a (no settings) | none |
| overpy-pixelart | OK — 0 findings | rejected: `settings` top-level section | **OK — 0 findings** | settings section |
| overpy-santa | parse-error `expected ')'` at `santa.opy:192:99` | lex-error `unexpected character '\''` at ws line 13 (`King's Row Winter` in settings) | rejected: unknown value `Vector Towards` (orig ws line 109) | OPY: chase kwargs/`ChaseReeval` enum at 192; WS: settings + catalog gap |
| overpy-parabola | parse-error `expected a member name after '.'` at `parabola.opy:35:37` | rejected: `settings` top-level section | rejected: unknown action `Create Dummy Bot` (orig ws line 61) | OPY: `Hero.TORBJORN`/`Team.2` enum member spelling; WS: settings + catalog gap |
| overpy-client-to-server | parse-error `expected ')'` at `clientToServer.opy:55:53` | rejected: `settings` top-level section | rejected: unknown value `Is Dummy Bot` (orig ws line 50) | OPY: ternary-chain method call; WS: settings + catalog gap |
| overpy-crosshair | parse-error `expected ')'` at `crosshair.opy:31:36` | rejected: `settings` top-level section | parse-error `expected an action` (orig ws line 29) | OPY: `HudReeval` enum kwarg; WS: oracle emits quoted pseudo-script in `actions` |
| overpy-cronch | parse-error `expected an expression but found '+'` at `cronch.opy:32:21` | parse-error `expected an action` at ws line 20 | n/a (no settings) | OPY: `++` operator; WS: oracle emits quoted pseudo-script in `actions` |
| overpy-inputhud | parse-error `expected ')'` at `inputhud.opy:41:63` | rejected: `settings` top-level section | rejected: unknown value `Is True For Any` (orig ws line 30) | OPY: `any([...])` list comprehension; WS: settings + catalog gap |
| overpy-broken-weapons | parse-error `expected ']'` at `broken_weapons.opy:53:55` | rejected: `settings` top-level section | rejected: unknown value `Workshop Setting Real` (orig ws line 69) | OPY: `float[0.5:10]` slice syntax; WS: settings + catalog gap |
| overpy-zencopter | lex-error `unterminated string literal` at `heli.opy:38:22` | no artifact (oracle compile failed) | n/a | OPY: `"""@Rule ..."""` docstring directives |
| overpy-meipocalypse | lex-error `unexpected character '{'` at `meipocalypse.opy:223:37` | no artifact (oracle compile failed) | n/a | OPY: `{...}` dict literal in `+=` |
| 6v6-adjustments | lex-error `unexpected character '\'` at `constants/adj_constants.opy:8:85` | no artifact (oracle compile failed) | n/a | OPY: backslash in constants |
| ow1-emulator | lex-error `unexpected character '\'` at `common/env.opy:7:66` | no artifact (oracle compile failed) | n/a | OPY: backslash in constants |

**Analyzable-corpus boundary.** Exactly two fixtures parse through the native
OPY frontend (`overpy-cake`, `overpy-pixelart`), matching the expected
boundary. Exactly one fixture lints end-to-end through the raw Workshop path
(`overpy-cake`). A settings-stripped variant makes `overpy-pixelart` fully
lintable with 0 findings. Every other fixture is blocked on either (a) the
`settings` top-level section — deliberately rejected by the Workshop frontend
per the documented non-goal ("a `.ws` decompiler is a non-goal, #86",
`crates/wright-workshop/src/emitter.rs:9-10`; the parser rejects any
top-level section other than `variables`/`subroutines`/`rule`,
`crates/wright-workshop/src/parser.rs:85-96`) — (b) value/action spellings
absent from the minimal canonical catalog (`crates/wright-workshop/src/catalog/data/catalog.json`;
`Vector Towards`, `Dot Product`, `Distance Between`, `Create Dummy Bot`,
`Is Dummy Bot`, `Is True For Any`, `Workshop Setting Real`, and even the
canonical `distance` id are all absent), or (c) an OPY-frontend parse/lex
gap on a construct used by the fixture. These are evidence boundaries, not
#99 implementation work: the issue's non-goals exclude OPY/OSTW compatibility
expansion merely to widen lint coverage.

## 2. Candidate rules and evidence

Workshop text below is the pinned oracle `compile.workshop` output. Source
references are the fixture `.opy` files. All sub-expression comparisons use
whitespace-normalized structural equality (the same notion as
`structurally_equal` in `crates/wright-analyzer/src/analysis.rs:411-476`,
which ignores arena ids and compares call name + argument structure).

### 2.1 Duplicate computation in loop bodies — `repeated-value` (spec-selected candidate)

**Evidence found — overpy-santa** (`compatibility/fixtures/real-world/overpy-santa/`):

- Workshop line 108: `For Global Variable(i, 0, Count Of(Global.rectangleChimneys), 1);`
  (rule `init AB AD`, event `Ongoing - Global`; source `santa.opy:71-78`,
  `for i in range(len(rectangleChimneys)):` at `santa.opy:72`).
- Workshop lines 109–112: four `Modify Global Variable(...)` actions in the
  loop body. Let AB = `Vector Towards(Multiply(Value In Array(Global.rectangleChimneys, Multiply(Global.i, 4)), Vector(1, 0, 1)), Multiply(Value In Array(Global.rectangleChimneys, Add(Multiply(Global.i, 4), 1)), Vector(1, 0, 1)))` and
  AD = the same with `Add(Multiply(Global.i, 4), 3)`. Counted occurrences of
  the whitespace-normalized sub-expressions:
  - line 109: AB ×1 (`rectangleChimneysAB`, Append To Array)
  - line 110: AD ×1 (`rectangleChimneysAD`, Append To Array)
  - line 111: AB ×2 — `Dot Product(AB, AB)` (`rectangleChimneysABDot`)
  - line 112: AD ×2 — `Dot Product(AD, AD)` (`rectangleChimneysADDot`)
- So within the loop body AB is computed 3 times (lines 109, 111 twice) and
  AD 3 times (lines 110, 112 twice) — **6 `Vector Towards` sub-expression
  occurrences total across the four actions, in two structurally distinct
  families** (AB and AD differ in the array-index arithmetic, so
  `structurally_equal` distinguishes them). Lines 111 and 112 each contain a
  `Dot Product(X, X)` whose two arguments are structurally identical
  sub-expressions **within a single action**. Under the spec's
  "one finding per additional occurrence" rule, santa's loop body yields
  **4 findings** (AB: 2 additional occurrences; AD: 2 additional). The
  spec's REQ-005 "appears 6 times" is the total occurrence count; the
  per-family counts (3 each) are what structural identity distinguishes.
- Source mirror: `santa.opy:73` (AB ×1), `:74` (AD ×1), `:76`
  `dotProduct(vectorTowards(...), vectorTowards(...))` (AB ×2), `:77` (AD ×2).

**Evidence found — overpy-parabola** (`compatibility/fixtures/real-world/overpy-parabola/`):

- Workshop line 79: `For Global Variable(I, 0, 25, 1);`
  (source `parabola.opy:45`, `for I in range(MAX_TEXTS):`).
- Workshop line 80: a single `Create In-World Text(...)` action whose
  `Update Every Frame` position argument contains exactly two occurrences of
  `Distance Between(Eye Position(Local Player), Value In Array((Local Player).textPos, Evaluate Once(Global.I)))` (verified substring count = 2),
  and exactly **6 occurrences** of
  `Subtract((Local Player).time, Value In Array((Local Player).timeOffsets, Evaluate Once(Global.I)))`
  in the same action (verified substring count = 6; matches the spec's
  REQ-005 count). Under per-additional-occurrence reporting, this single
  action fires **6 findings** (1 additional `Distance Between` + 5
  additional `Subtract`) — the concrete density the spec's Q-001 asks PM/QA
  to judge.
  Source mirror: `distance(localPlayer.getEyePosition(), localPlayer.textPos[evalOnce(I)])`
  at `parabola.opy:48` and `:51`; `localPlayer.time - localPlayer.timeOffsets[evalOnce(I)]`
  at `:47` (×2), `:48` (×2), `:50`, `:51` (×2) — both inside the one
  `createInWorldText(...)` at `parabola.opy:46`.
- Note the array reads inside the duplicated sub-expression are wrapped in
  `Evaluate Once` (Workshop's native anti-recomputation idiom), but the
  `Distance Between` itself is recomputed each tick — so the duplication is
  real even where the author already used `Evaluate Once` for the inner reads.

Both cases sit in `For Global Variable` loop bodies, so the duplicated
evaluation recurs on every iteration (per corpus structure; see the runtime
claim caveat in §2.2).

**Existing rule corpus case — overpy-cake** (already shipped `min-wait-loop`):
`wright lint` on `source.opy` reports 1 finding
`min-wait-loop | warning | static-indicator` at span `54:5–54:10`
(source `while true:` at `source.opy:54`, `wait(0.016)` at `:56`); the raw
Workshop path reports the same finding at span `46:9–49:13` (workshop
`While(True);` at line 46, `Wait(0.016, Ignore Condition);` at line 48).

**Evidence classification with justification.** The spec assigns
`repeated-value` evidence class `exact`. That assignment is **sound under the
claim the spec's finding message makes** — "guaranteed re-evaluation"
(REQ-001): a structurally identical value sub-expression scheduled in the
same loop scope is, by the WIR action/value model, re-evaluated every time
its enclosing action executes, regardless of runtime values. Two supporting
facts:

- Workshop's WIR value model has **no side-effecting value node** — every
  `Value` variant is a pure expression and all state mutation lives in
  `Action` variants (`crates/wright-ir/src/wir/mod.rs:128-169`). Between two
  argument evaluations of one action no action can run.
- Within one atomic evaluation context (both args of the `Dot Product` on
  santa lines 111/112; both `Distance Between` occurrences in parabola's
  single action), re-evaluation is therefore result-identical for
  deterministic sub-expressions: pure waste. This is the same structural-fact
  basis as `duplicate-condition` (`Exact`).

What `exact` must NOT be read to claim, and the spec's REQ-006 already
forbids: a measured runtime/server-cost magnitude, and result-equality across
separate actions. Across separate actions (santa line 109 vs 111) the
recomputation is still guaranteed to be scheduled, but the result can differ
if an intervening action mutates a read variable (`Global.rectangleChimneys`,
`Global.i`) — no dataflow analysis exists in the current analyzer, so a
cross-action finding proves re-scheduling, not result-equality. Both claims
stay within the spec's wording: REQ-001 states the static fact ("the same
computation is evaluated again"), REQ-006 forbids cost multiplication. The
`exact` assignment is therefore consistent with the `Analysis::evidence()`
single-source-of-truth contract as long as the implementation keeps that
message wording.

**Known limitations (candidate).** Rule-local scope only; no dataflow;
structural equality only (arena-id-independent, call-name + argument shape;
does not track value-flow or prove intervening-state absence); no
cross-rule comparison; sub-expressions containing non-deterministic values
(`Random Value`/`Random Real` — `randomReal` is a catalog value id — or
time-dependent values) are still guaranteed to be re-evaluated (the `exact`
trigger holds), but the finding may not indicate a defect there — two random
draws can be intentional — so the `known_limits` text must state that
result-equality is only guaranteed for deterministic sub-expressions within
one atomic evaluation context; `Evaluate Once`-wrapped inner reads reduce
but do not eliminate the outer recomputation; loop coverage is `While` +
`For Global Variable` only (`For Player Variable` is not modeled, per the
spec's REQ-006).

**Real-project evidence case.** Yes — two: overpy-santa (workshop lines
108–112; source `santa.opy:72-77`) and overpy-parabola (workshop lines
79–80; source `parabola.opy:45-51`). Both were read directly from the pinned
oracle text because neither project currently passes through Wright's lint
pipeline (frontend gaps, §4) — the corpus evidence exists at line level, and
positive/negative *fixtures* for the rule must still be Wright-authored
synthetic `.opy` (the existing fixture pattern in
`crates/wright-analyzer/tests/fixtures/`).

### 2.2 Hot-loop/timing candidates ("loop with no wait")

**Evidence found.** The corpus contains exactly two `While` loops:
overpy-cake workshop line 46 (`While(True);` with `Wait(0.016, Ignore
Condition)` in body) and overpy-client-to-server workshop line 54
(`While(True);` with `Wait(0.016, Ignore Condition)` in body; source
`while true:` at `clientToServer.opy:44`, `wait()` at `:48`). **Every While
loop in the emitted workshop text contains a Wait.** overpy-client-to-server
is not analyzable on any path (OPY parse-error at `clientToServer.opy:55:53`;
workshop path blocked on settings), so it provides no runnable evidence
either. There is therefore **zero real-project corpus evidence for a
"loop with no wait" rule**.

Corpus-negative scan boundary (matches the spec's REQ-005 claim, with one
precision): the scan covers the 9 fixtures with emitted `compile.workshop`
text. The 4 fixtures without an artifact cannot be scanned: their OPY sources
do contain `while RULE_CONDITION` patterns (overpy-meipocalypse
`fightforyourlife.opy:55`, `mei_types.opy:88`, `:121`; overpy-zencopter
`heli.opy:138`, `:159`, `:175`, `:200`), but the pinned oracle produced no
workshop text for them, so no wait-presence claim is verifiable. The
corpus-negative statement holds for the analyzable corpus; the spec should
state the boundary that precisely.

The corpus `For Global Variable` loops (santa line 108, parabola line 79,
cake lines 15/18/21/31) all lack waits, but a "no wait" trigger applied to
`For Global Variable` would fire on all six corpus For loops — including
overpy-cake's four intentional geometry-building loops — with no documented
runtime-semantics basis distinguishing them. That is precisely the
high-noise default #99 warns against.

**Evidence classification.** The spec assigns `while-without-wait`
evidence class `static-indicator`. That assignment is sound and matches the
existing `min-wait-loop` precedent (statically known trigger — no `wait` call
in the body tree; impact — loop frequency — is an indicator, not a
measurement). The rule has **no real-project evidence case**, so per #99's
acceptance it ships only with the explicit documented synthetic justification
the spec records in REQ-005 (trigger is a statically exact structural fact;
corpus-negative scan over the emitted workshop text; negligible
false-positive surface). This is a PM scope decision (already made in the
spec, status `specified`) and is architecturally unblocked; the evidence
status remains synthetic-only. One wording constraint: the finding message
must not claim a measured runtime cost or a guaranteed crash for
non-constant conditions (the spec's REQ-002 already requires this).

**Runtime-claim caveat (applies to all timing candidates).** No corpus
fixture carries a runtime/server-cost measurement. Static analysis proves
the *schedule* (a loop body with a `Wait(0.016)` runs at the maximum
Workshop rate; a loop body with the identical value computed N times
performs N evaluations), never the *magnitude* of server cost. Any finding
wording must keep the existing hedged register (`min-wait-loop`: "Sustained
high-frequency loops can degrade server performance"; `expensive-loop-check`:
"may be expensive per evaluation"), and must not claim a precise runtime or
CPU cost.

## 3. Architecture constraint verification

### 3.1 ADR-0008 (tooling-first semantic platform)

- **Tooling-first product.** Lint/static analysis is a declared primary
  product surface (`0008-tooling-first-semantic-platform.md`, decision 1).
  The candidate rules are squarely in scope for M12 (#89/#99). ✅
- **Support claims are corpus-defined.** Rule-evidence claims must be
  grounded in the pinned corpus; this review records exactly which facts the
  corpus supports (§2) and which it does not (§2.2). ✅
- **No invented language features.** The rules must operate on the existing
  WIR value/action model as-is. They must not require catalog expansion to
  fire: note that the santa/parabola corpus lines reference catalog-absent
  spellings (`Vector Towards`, `Dot Product`, `Distance Between`), so the
  rule's fixtures are Wright-authored `.opy` (which lower through the native
  frontend to canonical call names, as `expensive-loop.opy` already does for
  `distance`). No new Workshop semantics are introduced. ✅
- **No community "anti-crash" doctrine as guaranteed-safe rules.** No
  candidate may assert that a pattern "crashes servers" or otherwise state a
  guarantee; all findings are indicators with hedged impact language. ✅
- **Boundary check:** the frontend gaps in §4 are evidence boundaries
  (recorded, not #99 work); expanding the catalog or OPY parser to lint more
  of the corpus is a separate compatibility concern, out of #99's non-goals.

### 3.2 #97 registry contract (`crates/wright-analyzer/src/registry.rs`)

- New first-party rules are added by pushing a `RegistryEntry` in
  `LintRegistry::default` (documented at `registry.rs:199-209`), **appended
  after the existing three** so the canonical order
  (`min-wait-loop`, `duplicate-condition`, `expensive-loop-check`) and the
  determinism contract (`LintRegistry::run` — registry order × program index
  order, `registry.rs:295-322`) are preserved. ✅
- Stable rule ID must match the finding `code`; metadata (severity, evidence
  class) is derived from the same analysis instance that produces findings
  (single source of truth, `registry.rs:218-220`). The candidates can meet
  this contract. ✅
- Unknown rule IDs in `LintConfig` remain silently stored and non-blocking —
  no change needed. ✅
- **Implementation constraint:** several existing registry tests hardcode
  the rule count and metadata shape (`registry_has_three_first_party_rules_with_stable_ids`
  and related tests in `crates/wright-analyzer/tests/registry.rs`); adding
  rules requires updating those assertions. This is an implementation
  obligation, not a contract change; existing rule *semantics* are untouched
  (additive entries only).

### 3.3 #98 lint surface

- New rules plug into `wright lint`, `CompilerSession::lint`, and the
  tool/agent lint requests automatically via the registry; the documented
  envelope contract in `docs/cli.md` (`stable IDs`, `effectiveSeverity`,
  `evidence`, `knownLimits`, `tags`, `input_identity`, spans) applies to new
  rules without surface changes. ✅
- `LintConfig` disable/severity overrides operate per rule ID with no
  per-rule special cases. ✅

### 3.4 Evidence-class contract (`crates/wright-analyzer/src/analysis.rs:40-57`)

- The contract supports both spec-assigned classifications: `exact`
  (`repeated-value`) and `static-indicator` (`while-without-wait`).
  `Heuristic` is not required by the new set; `RuntimeValidated` remains
  reserved/unused. ✅
- Per-rule classification soundness: the single-source-of-truth rule means
  the finding message, `RuleMeta.evidence`, and `Analysis::evidence()` must
  agree. Both assignments are sound under the spec's message wording
  (§2.1, §2.2); the implementation obligation is to keep that wording (no
  cost-magnitude or cross-action result-equality claims). ✅
- **Known-limitations field must be explicit** for each candidate:
  rule-local scope, no dataflow, structural-equality limits,
  non-deterministic-subexpression caveat, and (for timing rules) no runtime
  measurement. The contract's `known_limits` field already carries this
  pattern for the existing rules. ✅

### 3.5 Dependencies and existing-rule compatibility

- Adding the candidates requires **no new dependencies** (registry/analysis
  changes are within `wright-analyzer` using existing `wir`/`Cfg`
  facilities; the existing `structurally_equal` machinery is in-module). No
  plugin host / #96 work is needed; no new crates. ✅
- Existing rule semantics remain byte-compatible: entries are additive,
  `LintRegistry::run` order for existing rules is unchanged, and `check`/
  `analyze` output is unaffected (no `Finding` shape change required — the
  candidates reuse `Finding` as-is). ✅

## 4. Frontend gaps recorded as evidence boundaries

These are recorded so #99's evidence claims are not mistaken for
"the corpus lints cleanly", and so nobody re-scopes #99 into compatibility
work. None is #99 implementation work (issue non-goals exclude OPY/OSTW
compat expansion merely for lint coverage).

1. **Native OPY frontend gaps (11/13 fixtures):** `santa.opy:192:99` (chase
   kwargs / `ChaseReeval` enum), `parabola.opy:35:37` (`Hero.TORBJORN` /
   `Team.2` enum member spellings), `clientToServer.opy:55:53` (ternary-chain
   method call), `crosshair.opy:31:36` (`HudReeval` enum kwarg),
   `cronch.opy:32:21` (`++`), `inputhud.opy:41:63` (`any([...])`),
   `broken_weapons.opy:53:55` (`float[0.5:10]` slice syntax),
   `heli.opy:38:22` (`"""@Rule ..."""` docstring directives),
   `meipocalypse.opy:223:37` (`{...}` dict literal),
   `adj_constants.opy:8:85` / `env.opy:7:66` (backslash).
2. **Workshop `settings` section deliberately rejected** (parser rejects any
   top-level section other than `variables`/`subroutines`/`rule`,
   `parser.rs:85-96`; `.ws` decompiler documented non-goal, `emitter.rs:9-10`).
   Blocks the raw Workshop path for 7 fixtures; santa additionally fails on
   the apostrophe in `King's Row Winter` within settings.
3. **Minimal canonical catalog** (`crates/wright-workshop/src/catalog/data/catalog.json`,
   33 canonical value/action ids): `Vector Towards`, `Dot Product`, `Distance Between`,
   `Create Dummy Bot`, `Is Dummy Bot`, `Is True For Any`,
   `Workshop Setting Real` are absent, blocking settings-stripped santa/
   parabola/client-to-server/inputhud/broken-weapons from the raw Workshop
   path. The corpus facts for those fixtures were therefore read directly
   from the pinned oracle text (file:line cited in §2.1), not through
   Wright's pipeline.
4. **Oracle output not always native Workshop text:** overpy-cronch (ws
   line 20) and overpy-crosshair (ws line 29) `compile.workshop` contains
   quoted pseudo-script strings inside `actions`, which the native parser
   rejects (`expected an action`). Four fixtures have no workshop artifact
   because the pinned oracle failed to compile (oracle `status=failure`:
   meipocalypse ENOENT `generateWalls.js`, zencopter `Invalid content before
   string: 'arena'`, 6v6-adjustments `Unknown member '_hp_reset'`,
   ow1-emulator `Found 'if', but no 'else'`).

## 5. Recommendation

The PM spec (`SPEC-99-stability-rules`, status `specified`) selects a
two-new-rule set. Verdict per rule on the independently re-derived evidence:

- **`repeated-value` — evidence-supported, architecture-clear.** Two
  real-project corpus cases at line level (santa workshop 108–112 /
  `santa.opy:72-77`; parabola workshop 79–80 / `parabola.opy:45-51`), plus
  the WIR value-model purity fact backing the re-evaluation claim. The
  `exact` classification is sound under the spec's "guaranteed re-evaluation"
  message wording (REQ-001) with REQ-006's no-cost-multiplication guard.
  Verified occurrence counts: santa AB ×3 / AD ×3 (6 total, two families;
  4 per-additional-occurrence findings), parabola `Distance Between` ×2 and
  `Subtract` ×6 (6 per-additional-occurrence findings on one action).
  Positive/negative fixtures must be Wright-authored synthetic `.opy`
  (corpus projects don't currently lint). Requirement "at least one
  real-project/corpus evidence case" is satisfied.
- **`while-without-wait` — synthetic-only, architecture-clear with wording
  constraint.** Zero corpus positive evidence (both corpus While loops in
  emitted text contain `Wait(0.016)`; client-to-server isn't analyzable).
  The `static-indicator` classification matches the `min-wait-loop`
  precedent. Ships only via the documented synthetic justification REQ-005
  records — a PM scope decision already made in the spec. Finding message
  must not claim measured cost or guaranteed crash for non-constant
  conditions (REQ-002 already requires this).
- **`min-wait-loop` corpus case (existing):** overpy-cake fires on both the
  OPY and raw-Workshop paths (already shipped; unchanged).

Resolved architecture concerns:

1. **Evidence-class assignments are sound** — `repeated-value` = `exact`
   (guaranteed re-evaluation; result-equality only within one atomic
   evaluation context for deterministic sub-expressions; no cost
   multiplication) and `while-without-wait` = `static-indicator`. Both fit
   the `EvidenceClass` contract with the single-source-of-truth rule as long
   as the finding messages keep the spec's wording.
2. **Rule scope** (within-action vs cross-action) is fixed by REQ-001's loop
   scope definition; the known-limitations text must carry the cross-action
   result-equality caveat.
3. **Timing-rule claims** are bounded by REQ-002/REQ-006 hedged wording; no
   runtime measurement exists in the corpus.
4. **Implementation obligations (not blockers):** update the count-asserting
   registry tests (`registry_has_three_first_party_rules_with_stable_ids`
   and related, `crates/wright-analyzer/tests/registry.rs`); keep new
   entries appended in the canonical order.

Open items for PM/QA (not architecture blockers): Q-001 finding density —
santa's loop fires 4 and parabola's single action fires 6 findings under
per-additional-occurrence reporting, which concretely tests the density
revisit option; Q-002 corpus-evidence re-verification automation;
Q-003 `while-without-wait` synthetic-evidence sufficiency.

No architecture blocker found for the specified two-rule set. Current
workflow state: architecture review for the #99 rule set — evidence review
recorded, no code changed. Next authoritative role: **PM** — confirm the
spec (including the Q-001 density judgement once fixtures land) and route to
architecture review sign-off / implementation.
