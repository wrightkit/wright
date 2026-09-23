---
kind: wright-spec/v1
id: SPEC-99-stability-rules
title: Bounded evidence-backed Workshop stability rule set
status: accepted
related_issue: "#99"
owner: PM
freshness: live
---

## Goal

Ship a small, high-value first-party stability/performance rule set through the
lint registry (#97) and `wright lint` path (#98), extending the existing
three rules with two new rules (`repeated-value` and
`while-without-wait`), so `wright lint` reports a bounded five-rule first-party
set. Every rule keeps a stable ID, default severity, evidence classification,
documented rationale and limitations, positive/negative fixtures, and linked
corpus evidence or an explicit documented synthetic justification. The set must
not turn community advice into correctness doctrine and must not overstate what
static analysis can prove.

### Selected rule set (PM-approved)

| Stable ID | Default severity | Evidence class | Real-project/corpus evidence |
| --- | --- | --- | --- |
| `min-wait-loop` (existing) | warning | static-indicator | overpy-cake fires at `source.opy:54` / workshop line 46 |
| `duplicate-condition` (existing) | warning | exact | (unchanged, v0.2) |
| `expensive-loop-check` (existing) | info | heuristic | (unchanged, v0.2) |
| `repeated-value` (new) | warning | exact | overpy-santa workshop lines 108-113; overpy-parabola workshop lines 79-80 |
| `while-without-wait` (new) | warning | static-indicator | none positive in corpus (documented synthetic justification; corpus-negative scan recorded) |

Severity decisions: both new rules default to `warning`. `repeated-value` is an
exact structural fact (guaranteed re-evaluation) scoped to loop bodies, where
cost compounds and the ecosystem `Evaluate Once` idiom exists; `while-without-wait`
mirrors the existing `min-wait-loop` (statically known trigger, impact is an
indicator). No new `heuristic`-class rules are added in this set.

## Requirements

- **REQ-001** [repeated-value rule]: The `repeated-value` rule is registered in
  `LintRegistry::default` with stable ID `repeated-value`, default severity
  `warning`, evidence class `exact`, and non-empty `summary`, `documentation`,
  `known_limits`, and `tags`. When a Workshop rule contains a loop (`While` or
  `For Global Variable`) whose evaluated surface contains a value expression
  that (i) is structurally identical to a value expression evaluated earlier
  in the same loop scope, (ii) contains at least two `Call` value nodes in its
  subtree (the root counting), and (iii) is not a proper sub-expression of a
  larger value expression that also occurs at least twice in the same loop
  scope, `wright lint` (and `CompilerSession::lint` / tool `lint`) reports
  **exactly one finding for that duplicated shape**, at the shape's first
  occurrence in the scope, with `code: repeated-value`, `severity: warning`,
  `evidence: exact`, a span on the first-occurring value, and a message stating
  the statically known occurrence count.
  Acceptance: positive fixtures mirroring the two corpus shapes (identical
  `Distance Between(...)` twice inside one action in a `For` loop; identical
  `Vector Towards(...)` across sibling `Modify Global Variable` actions in a
  `For` loop) produce exactly the per-shape counts below (code, severity,
  evidence, span, count); the negative fixture set below produces none;
  repeated runs are deterministic; as a smoke check, `wright lint` on the
  overpy-cake real-project fixture reports no `repeated-value` findings on
  single-call array reads, no same-shape identical-span duplicates, and the
  verified per-shape density recorded in Q-001 (10 findings). Distinct shapes
  whose first-occurrence source spans collide via macro-expansion span
  assignment (e.g. the `cakePos[0]` and `cakePos[1]` corner families both
  reported at `38:9`) are distinct genuine findings, not duplicates, and do
  not violate this criterion (PM Decision A, recorded at spec closure).
  - Loop scope definition (observable contract): the loop's own condition
    (`While`) plus the value positions of every action in the loop body.
    Nested loops are analyzed as their own separate scopes; `For Global
    Variable` start/stop/step bounds are excluded (they are not re-evaluated
    per iteration). Structural identity uses the same structural-equality
    semantics as the existing `duplicate-condition` rule (arena-id-independent,
    recursive).
  - Finding-reporting rule (amended; supersedes the earlier
    per-additional-occurrence wording): one finding per distinct duplicated
    shape per loop scope. A shape is duplicated when it occurs at least twice
    in the scope; it is reported only when no strictly larger duplicated shape
    in the same scope contains it (nested duplicates are reported once, at the
    maximal enclosing shape). Findings are ordered deterministically by the
    source position of each shape's first occurrence, loop scopes in program
    order (registry determinism contract). The finding's message states the
    statically known occurrence count without multiplying it into a cost
    claim.
  - Non-trivial filter (amended): a duplicated expression is in scope only if
    its subtree contains at least two `Call` value nodes (the root counting).
    Single-call expressions (including bare `Value In Array` reads and bare
    single-call predicates with plain operands) are never flagged. Bare
    literals, strings, booleans, enums, variable references, and plain
    `Vector`/`Array` constructions are likewise never flagged.
  - Negative fixtures: identical expressions used in different rules; identical
    expressions outside any loop; duplicated trivial literals/vectors inside a
    loop; non-identical sibling expressions in a loop; duplication crossing a
    nested-loop boundary; duplication in `For Global Variable` bounds;
    duplicated single-call expressions inside a loop (e.g. a bare array read
    used twice in one action, or a single-call predicate with plain operands).

- **REQ-002** [while-without-wait rule]: The `while-without-wait` rule is
  registered in `LintRegistry::default` with stable ID `while-without-wait`,
  default severity `warning`, evidence class `static-indicator`, and non-empty
  `summary`, `documentation`, `known_limits`, and `tags`. When a `While` loop's
  body tree contains no `wait` action call (using the same notion of `wait` as
  `min-wait-loop`), `wright lint` reports one finding per offending `While`
  loop, with `code: while-without-wait`, `severity: warning`,
  `evidence: static-indicator`, and a span on the `While` action. The finding
  message states the static fact (the loop body contains no wait call, so the
  loop cannot yield while its condition holds) and must not claim a measured
  runtime cost or a guaranteed crash for non-constant conditions.
  - Scope: `While` loops only. `For Global Variable` loops are never flagged.
    A `wait` call anywhere in the body tree (including nested `If` branches)
    suppresses the finding, even when the wait duration is not statically
    known. A `wait` at the minimum duration is `min-wait-loop`'s finding, not
    this rule's.
  - Negative fixtures: `While` loop with a wait (literal duration); `While`
    loop with a non-literal/computed wait duration; `While` loop whose wait is
    nested inside an `If` in the body; `For Global Variable` loop with no wait.
    Positive fixture: `While(true)` loop with no wait yields exactly one
    finding.

- **REQ-003** [registry integration and compatibility]: Both new rules are
  appended after the existing three in `LintRegistry::default`, yielding the
  canonical order `min-wait-loop`, `duplicate-condition`, `expensive-loop-check`,
  `repeated-value`, `while-without-wait`. Each new entry's evidence class is
  taken from its `Analysis` implementation (single source of truth), and each
  satisfies the registry contract: stable ID, non-empty metadata, deterministic
  execution. The existing three rules' IDs, default severities, evidence
  classes, relative order, and behavior are unchanged. `LintConfig`
  enable/disable/severity-override and the CLI `--disable-rule` /
  `--rule-severity` flags apply to the new rules identically. The registry test
  that currently pins "exactly three first-party rules" is updated to pin the
  new five-rule set (a contract-pinning test change, not a behavior change).
  Acceptance: `LintRegistry::default().rules()` returns the five IDs in the
  canonical order; existing-rule findings on existing fixtures are unchanged;
  severity overrides fire on the new rules at both CLI and API layers.

- **REQ-004** [positive/negative fixtures]: Each new rule gains positive and
  negative fixtures under `crates/wright-analyzer/tests/fixtures/` following
  the existing `.opy` source + `.json` adapter-payload pattern documented in the
  fixtures `README.md` (Wright-authored, AGPL-3.0-or-later; no third-party
  material). The `repeated-value` positive fixtures reproduce the two corpus
  shapes with corpus-realistic inner calls (each duplicated shape contains at
  least two call nodes): a parabola-style within-action duplicate and a
  santa-style cross-action duplicate inside a `For Global Variable` loop.
  Under the amended per-shape reporting rule (REQ-001), the santa mirror
  asserts **exactly 2 findings** (one per family: AB and AD), not 4; the
  parabola mirror asserts **exactly 2 findings** (one per duplicated shape
  family: the `Distance Between`-family shape and the `Subtract`-family shape,
  mirroring the corpus's two shape families), not 1 and not 6. The positive
  fixtures also pin the subsumption and per-scope semantics: a duplicated shape
  nested inside a larger duplicated shape yields only the maximal finding, and
  the same shape duplicated in two different loop scopes in one rule yields one
  finding per scope. The `repeated-value` negative fixture additionally covers
  duplicated single-call expressions inside a loop (bare `Value In Array`
  reads; single-call predicates with plain operands) producing no findings.
  `while-without-wait`'s positive fixture is a `While(true)` loop with no wait
  and asserts exactly one finding. Expected findings are deterministic and
  asserted by `cargo test -p wright-analyzer`.
  Acceptance: focused fixtures exist with provenance notes; tests pass and pin
  exactly the expected findings (code, severity, support class, span, count) for every
  fixture.

- **REQ-005** [real-project test linkage]: Every new rule documents
  its real-project motivation, or an explicit synthetic justification with the
  negative scan recorded. For `repeated-value`, the rule documentation records
  the two historical pinned cases maintained by `opy-rs`: overpy-santa (the
  `For Global Variable(i, 0, Count Of(Global.rectangleChimneys), 1)` loop; the
  `Vector Towards(...)` sub-expression `A` appears 6 times across the four
  `Modify Global Variable` actions and each `Dot Product` action on lines 111
  and 112 contains the identical `Vector Towards` argument twice) and
  overpy-parabola (workshop lines 79-80: `For Global Variable(I, 0, 25, 1)`;
  the identical `Distance Between(Eye Position(Local Player), Value In Array(
  (Local Player).textPos, Evaluate Once(Global.I)))` appears twice within one
  `Create In-World Text` action, and the identical `Subtract((Local Player).time,
  Value In Array((Local Player).timeOffsets, Evaluate Once(Global.I)))` appears
  6 times in the same action). Both projects are pinned by the owner repository
  to Zezombye/overpy commit `eea67adbcf6926c4004e35e25ab4be072624a44e`,
  GPL-3.0-only. For `while-without-wait`, the documentation records that the
  owner corpus found `While` loops only in overpy-cake and
  overpy-client-to-server, and both contain waits (a negative result);
  synthetic validation is documented as sufficient because the trigger is a
  statically exact structural fact (presence of a `wait` call in the loop body
  tree) with a negligible false-positive surface. No rule claims owner support
  it does not have.
  Note (amendment): the earlier review's per-additional-occurrence counts
  (issue #99 §2.1: overpy-santa 4 findings,
  overpy-parabola 6 findings) are **superseded** by the amended per-shape
  reporting rule (REQ-001, Q-001); the occurrence structure this
  requirement records (6 `Vector Towards` occurrences in two families; 2
  `Distance Between` and 6 `Subtract` occurrences) is unchanged and remains the
  rule's real-project motivation.
  Acceptance: each new rule's metadata note states its owner case or synthetic
  justification and does not assert owner backing for
  `while-without-wait`.

- **REQ-006** [documentation and evidence labeling]: Each new rule's
  `documentation` and `known_limits` (rendered through the #98 lint surface:
  the `rules` envelope entries and the tool/agent `lintRules` response) state
  the rationale tied to the evidence case, the evidence classification, and the
  known limitations: loop coverage is `While` + `For Global Variable` only (the
  loop analysis does not model `For Player Variable` loops); `repeated-value`
  is structural and does not perform value-flow or cross-rule comparison; a
  duplicated expression is reported once per loop scope at its maximal shape,
  so nested duplicates are subsumed and single-call expressions (e.g. bare
  array reads) are never flagged; a finding's span is the first occurrence of
  its duplicated shape, and when source spans are assigned by macro expansion
  (e.g. OverPy `#!define` inlining), distinct shapes can share a
  first-occurrence span, as observed in overpy-cake at `38:9`, where the
  `cakePos[0]` and `cakePos[1]` corner shapes collide; findings remain
  distinct and are distinguished by their value/action identity in the
  structured envelope, so span identity is not a proxy for finding identity
  (presentation artifact, not a defect); `while-without-wait` does not
  distinguish constant-true from data-dependent conditions. Messages and documentation must
  not claim precise runtime/server CPU cost from static evidence alone;
  `repeated-value` may state the statically known occurrence count but must not
  multiply it into a cost claim for arbitrary programs. Findings self-label via
  the `evidence` field (`exact`, `static-indicator`); heuristic findings remain
  identifiable as `evidence: heuristic` on the existing `expensive-loop-check`.
  The lint surface documentation (`docs/cli.md`) is updated so its rule-ID
  examples and `--disable-rule`/`--rule-severity` wording cover the new
  five-rule set.
  Acceptance: registry contract test asserts non-empty metadata for all five
  rules; documentation wording is reviewed for cost-overstatement (no numeric
  server-cost claims from static evidence).

- **REQ-007** [CI and regression gate]: `cargo test -p wright-analyzer` is green
  with the new fixtures, and the workspace test suite / CI is green at
  acceptance. Existing-rule behavior is unchanged apart from additive
  registry entries and the rule-count test update required by REQ-003.
  Acceptance: full test run and CI green on the implementation commit; no
  changes to the observable behavior of the three existing rules.

## Non-goals

- Adding more than the two new rules in this set (the shipped first-party set is
  exactly five rules; the issue explicitly rejects large rule-count targets).
- OPY/OSTW frontend or compatibility expansion so that overpy-santa,
  overpy-parabola, or other corpus projects parse through a Wright-owned
  frontend; source-language support belongs to the provider repositories
  (per issue non-goals; corpus evidence for `repeated-value` is textual and
  linked, not a firing path).
- Treating community "anti-crash" advice (e.g., `Server Load` thresholds, slow
  motion triggers) or Workshop.Codes Wiki recommendations as guaranteed-safe
  rules.
- Claiming precise runtime/server CPU cost from static evidence; using the
  reserved `runtime-validated` evidence class in this set.
- Automatically rewriting code or suggesting edits as part of these rules.
- Rules over `For Player Variable` loops (not modeled by the loop analysis).
- Third-party plugin loading or #96 content-pack work.
- Changing the IDs, severities, evidence classes, or behavior of the three
  existing rules.
- Requiring the workshop parser to support the `Evaluate Once` value spelling
  used in the parabola corpus (a compatibility matter outside #99).

## Architecture constraints and references

- **ADR-0008** (`../adr/0008-tooling-first-semantic-platform.md`): tooling
  first, evidence-backed claims, support claims are corpus-defined, no invented
  language features. Constraint: every shipped rule must carry an evidence
  classification and a corpus or explicit synthetic justification; findings
  must not overstate static analysis.
- **#97 registry contract** (`../../crates/wright-analyzer/src/registry.rs`): stable
  rule IDs matching finding `code`, `RuleMeta` fields, fixed canonical order,
  deterministic `LintRegistry::run`, `LintConfig` enable/disable/severity.
  Constraint: new rules are first-party `RegistryEntry` values; third-party
  plugin loading remains out of scope.
- **#98 lint surface** ([`../cli.md`](../cli.md) "`wright lint` and the lint configuration";
  `CompilerSession::lint`; `ToolRequest::Lint`/`LintRules`): structured findings
  with `evidence`, rule metadata in the result envelope, deterministic config
  across CLI and tool/agent paths. Constraint: new rules surface through the
  existing path with no new protocol surface.
- **EvidenceClass contract** (`../../crates/wright-analyzer/src/analysis.rs`):
  `exact` / `static-indicator` / `heuristic` / `runtime-validated` (reserved).
  Constraint: `repeated-value` uses `exact`; `while-without-wait` uses
  `static-indicator`; the finding's `evidence` field mirrors the producing
  rule's class (single source of truth).
- **Structural-equality semantics** (`structurally_equal` in
  `../../crates/wright-analyzer/src/analysis.rs`): arena-id-independent recursive
  equality over `Value`. Constraint: `repeated-value` reuses this semantics so
  duplicate-condition and repeated-value agree on identity.
- **Fixture pattern** (`../../crates/wright-analyzer/tests/fixtures/README.md`):
  `.opy` source + pinned adapter `.json` payload, Wright-authored. Constraint:
  new fixtures follow the same provenance and regeneration process.

## Related contracts

- The lint registry and configuration contract is implemented by
  `crates/wright-analyzer/src/registry.rs`.
- The public lint surface is defined by [the CLI/driver contract](../cli.md)
  and the shared driver/tool result contracts.
- OPY source-language compatibility and corpus ownership belong to
  `wrightkit/opy-rs`; Wright keeps only focused analyzer/provider integration
  regressions required by Wright-owned behavior.
- Historical implementation, verification, dependency, and acceptance state
  for this feature remains in Issues #97–#100 and their PR/CI history rather
  than this durable specification.
