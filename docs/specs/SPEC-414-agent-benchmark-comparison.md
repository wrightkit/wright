---
kind: wright-spec/v1
id: SPEC-414-agent-benchmark-comparison
title: Multi-condition, multi-model agent benchmark with tool-call analysis
status: proposed
related_issue: "#414"
owner: PM
freshness: live
---

This spec refines #414 and extends the current [agent benchmark contract](../agent-benchmark.md). The
no-specialized-injection condition stays the product acceptance path. Everything else here is an experiment
and is labeled as one in every report.

## Goal

Answer, with deterministic evidence, how much Wright changes a general coding agent's outcome on realistic
Workshop requirements, how agents actually call Wright compared with its design intent, and how both depend on
the environment (network, documentation) and the model.

- Q1: does the agent deliver a usable result more often with Wright, and with the optional guide from
  `wrightkit/skills`, than without?
- Q2: do agents use Wright's commands as designed, and which commands or flags cause friction?
- Q3: how do offline, web, and local-wiki conditions change Q1 and Q2?
- Q4: do the answers hold across models?
- Q5: which owner (agent, Wright, `opy-rs`, `workshop-rs`, upstream) is responsible for each failure?
- Q6: what context and tokens does each task consume, where do they go, and does Wright reduce the cost of a usable result?

Hypothesis H1, fixed before the first full run: for scenarios where both conditions reach a usable result, `bin` and `bin+skill` use fewer total tokens and less peak context than `none`, because compile-and-fix iterations and documentation reading shrink. H1 can be falsified: Wright output also enters the context. Report it either way.

Pilot observations that shaped this spec (Sonnet only, 2 scenarios, 18 runs; not conclusions):

- With `wright` on PATH and no hint, the agent used it in 3 of 6 runs; with the guide, in 6 of 6.
- Upstream-oracle validity was 1/6 without Wright, 3/6 with it, 4/6 with it plus the guide.
- Grading by Wright alone hides engine gaps. Upstream disagreed with Wright on accepted spellings
  (`opy-rs#410`) and on a settings key that `check` accepted and `compile` refused (`opy-rs#411`).

## Requirements

### Conditions

- REQ-001: A run is defined by (Wright level, knowledge level, tool network). Wright level is `none`, `bin`
  (on PATH through the tracing shim), or `bin+skill` (plus the guide from a pinned `wrightkit/skills` commit,
  installed through the agent's own skill mechanism). Knowledge level is `none`, `wiki` (a read-only local
  snapshot of the Workshop wiki Markdown mirror, pinned by hash, with no added index or tool), `wiki-skill`
  (a separately generated local `workshop-wiki` skill with category indexes and article files, pinned by
  snapshot and skill content hashes, usable without Wright), or `web`
  (standard fetch and search tools). Tool network is `off` or `on`; `web` implies `on`.
- REQ-002: Tier 1 conditions are `none/none`, `bin/none`, `bin+skill/none`, `none/web`, `none/wiki`,
  `none/wiki-skill`. The local pilot uses the five non-web cells first. Tier 2 is
  `bin+skill/web`, `bin+skill/wiki`, `bin/wiki`. The task and prompt are identical across conditions.
- REQ-003: Each run passes a canary before the agent starts, and a failed canary invalidates the run: `none`
  Wright means `command -v wright` fails and no install path is reachable; `off` means an attempted fetch from
  the agent's tool environment fails while the model channel still works.
- REQ-004: Every environment is fresh per run: new `HOME` and config directory, no host instruction files, no
  other skills, no MCP servers, an allowlisted PATH. The adapter reports its loaded context, and a run whose
  context contains anything unexpected is invalid.

### Scenarios

- REQ-005: The initial suite covers the four #414 families in OverPy and raw Workshop, and adds a diagnosis
  scenario whose seed passes `check` but has a waitless loop, a cross-file semantic rename, and an
  impossible-request scenario whose correct outcome is a refusal. Greenfield scenarios derive from the
  gamemode ideas in `Zezombye/overpy#439` (cited as a code span, not linked). Prompts state requirements in
  user terms and never name Wright, its commands, or Workshop APIs, and contain no public mode codes or titles.
- REQ-006: Each scenario provides `reference/` (passes every check) and `negative/` overlays (each fails
  exactly its target checks). A scenario is valid only when its reference passes, its seed fails at least one
  check, and every negative fails as designed.

### Grading

- REQ-007: Graders are frozen before agent runs and their hash is stored in every result. A change to any
  grader re-grades all runs and is listed in the report.
- REQ-008: Validity is recorded per authority and never collapsed: L1a the pinned upstream OverPy oracle
  (ADR-0007 pins) compiles the source; L1b `wright check` and `wright compile` succeed; L1c the emitted
  Workshop text passes `wright check` through `workshop-rs`. The strict metric uses L1a. Wright's verdict
  alone never decides it.
- REQ-009: Every run records whether L1a and L1b agree. A disagreement is emitted with an automatically
  captured minimal reproducer and the candidate owner, for creation of owner Issues.
- REQ-010: Requirement checks assert on the compiled Workshop program through Wright's structured surface
  (`inspect` JSON or `serve` operations). A regex over emitted text is allowed only where no structured field
  exists and ships with a positive and a negative fixture.
- REQ-011: Reported outcome metrics are: `usable` (L1a passes, all required requirement checks pass, no
  error-severity lint finding), requirement coverage over all runs and over L1a-passing runs, time to first
  valid snapshot, valid-to-invalid regressions, wall time, and the context and token metrics of REQ-020 to REQ-024. Runtime-only claims are listed as
  unverified and never counted as passed. A rubric-based judgment, if used, is reported separately from pass/fail.

### Tool-call analysis

- REQ-012: For every run, record each `wright` invocation (argv, cwd, start, duration, exit, output sizes,
  stdin use, and, when the output is a `wright-result/v1` envelope, its `command`, `ok`, `exit`, diagnostic
  codes, `input_identity`, and `selection`), each `wright serve` request and response, the normalized agent
  transcript, and a snapshot of the entry file after every write. Snapshots are compiled after the run in the
  grader environment.
- REQ-013: Expectation detectors over the trace produce a per-run vector and per-condition rates. Each detector
  names the Wright document statement it tests and the scenario families it applies to.

| id | expectation | detector |
| --- | --- | --- |
| E01 | discover before use (help, version, or `capabilities`) | class of the first Wright call |
| E02 | decisions use structured output (`--format json` or `serve`) | share of decision-driving calls |
| E03 | `check` is only the correctness gate; stability needs `lint` or `analyze` | on stability-risk tasks, `lint` or `analyze` precedes the final answer |
| E04 | the final state is validated | a validation follows the last edit and its `input_identity` equals the final file hash |
| E05 | symbol questions use name-addressed `inspect` | `inspect refs/symbols/cfg` versus text search for identifiers, on understanding tasks |
| E06 | finding selection is used on large outputs and the withheld count is noticed | flag use; transcript acknowledgement |
| E07 | semantic refusals are respected | after an `edit-*` or `rename-*` refusal, no textual replace of the same symbol |
| E08 | exit codes drive behavior | exit 1 is followed by an edit removing that diagnostic code; exit 3 or 4 is not retried past a bound |
| E09 | rename uses the semantic operation where supported | rename scenarios: semantic operation versus `sed`, and result correctness |
| E10 | `convert` output is not written over hand-written source | file diff after `convert` |
| E11 | `serve` sessions negotiate capabilities and avoid malformed requests | first operation; malformed-request rate |
| E12 | the edit-and-recheck loop is tight | edits between validations; identical repeated calls with no edit between |

- REQ-014: Friction is counted per command and flag: usage errors (exit 2), unknown subcommand or flag
  attempts including commands the installed version lacks, help lookups per task, retries after exit 3 or 4,
  malformed `serve` requests, identical repeated calls, and turns to first successful use.
- REQ-015: Each failing run is attributed to a layer (`agent`, `wright`, `opy-rs`, `workshop-rs`, `upstream`)
  from deterministic evidence, and aggregated into an owner-issue table with reproducers.

### Context and token consumption

- REQ-020: The adapter reports usage per model turn: input, output, cached-read, cached-write, and reasoning
  tokens as the provider states them, plus the context size at that turn and the model's context limit. The
  result stores the per-turn series and the totals. Any compaction, truncation, or context-limit event is
  recorded and marks the run.
- REQ-021: Context is attributed by source with one common tokenizer estimate so models are comparable: Wright
  output (per command), file reads, file writes and edits, shell output other than Wright, web fetch and search
  results, wiki reads, skill and system content, and model text. The attribution states its estimation method
  and reports the provider-stated total beside it, with the residual.
- REQ-022: Per run, report total tokens, peak context and its share of the limit, turns, tokens and turns to
  first valid snapshot (REQ-011), and tokens per usable result across a condition (total tokens of all runs
  divided by the number of usable runs, so failures count against the condition). Cost is reported separately
  from tokens because cache pricing differs by provider.
- REQ-023: Efficiency is compared paired by trial index. For H1, compare tokens and peak context between
  conditions on scenarios where both reach `usable`, and separately over all runs. Report the direction and size
  with intervals, per scenario and per model, and whether H1 held. A condition that spends fewer tokens by
  failing more often is not more efficient.
- REQ-024: Per Wright command, report the mean output size in tokens, the share of a run's context it
  occupies, and how often the agent re-reads or repeats it. Commands whose output is large relative to the
  decisions it drives are candidates for output-shape or selection-default Issues in `wright`. The guide's own
  token size is reported and counted in the `bin+skill` context.

### Eval quality and guide tuning

- REQ-025: The report flags problems with the eval itself: baseline headroom (usable rate at or above 95%,
  where no gain can show), run-to-run variance, infrastructure failures and timeouts, invalid runs, and a
  grader that gives different verdicts on the same stored workspace.
- REQ-026: Scenarios may carry a `split` of `train` or `test`, and reports break results out by split. Hard
  scenarios are chosen by human judgment of difficulty, not because a current model fails them.
- REQ-027: A guide-tuning loop over the optional guide changes one surface per round (description or body),
  scores train and held-out test scenarios under `bin+skill`, keeps the change only when both improve, reverts
  it otherwise, and analyzes the cause after two or three stalled rounds. Failure transcripts are never pasted
  into the guide, and reference solutions and answer keys stay outside the agent workspace. Attributable
  metrics apply: description changes are judged by trigger rate, body changes by the E01-E12 rates and
  outcome. The noise floor is measured before tuning starts.

### Models and statistics

- REQ-016: Agents run through an adapter that takes model, workspace, prompt, scrubbed environment, tool list,
  and skill directory, and returns transcript, per-turn usage (REQ-020), and exit status. The model list is data. The adapter,
  Wright release and checksum, OPY provider version, guide commit, wiki snapshot hash, and oracle version are
  recorded in a manifest. The contract does not depend on one vendor.
- REQ-017: Each cell runs at least 5 trials (3 for the largest scenario), in randomized order. Results give counts
  with Wilson 95% intervals, per scenario and per model before any pooling, with paired comparison across
  conditions by trial index. The primary metric and Tier 1 comparisons are fixed before the first full run.
  Provider errors are retried as infrastructure and logged, not counted.
- REQ-018: The web condition logs every fetched URL and flags runs that fetch public mode sources; flagged runs
  are reported separately, not dropped.

### Delivery

- REQ-019: Work proceeds in phases with exit criteria: calibrate graders and canaries; pilot one model at Tier 1
  with one trial to measure cost and find harness leaks; run Tier 1 across models; run Tier 2 as needed; triage
  results into owner Issues. Execution cadence stays manual or scheduled and never gates pull requests.

## Non-goals

- Ranking models, or tuning the benchmark for a vendor.
- Proving in-game runtime behavior; runtime-only claims stay unverified.
- A new verification framework. Extend `benchmarks/agent` and ordinary tests.
- Vendoring the guide or the Workshop wiki into this repository; both are referenced by pin.
- Wright-side workarounds for engine gaps that the benchmark exposes.

## Architecture constraints and references

- [Agent benchmark contract](../agent-benchmark.md): allowed agent context and the no-injection primary
  condition remain in force.
- [Agent contract](../agent-contract.md) and [CLI machine contract](../cli/machine-contract.md): traces and
  detectors use the documented envelope, exit codes, and `serve` operations.
- [Integration verification](../compatibility.md) and ADR-0007: the upstream oracle is a pinned reference;
  Wright is never the sole judge of OPY compatibility.
- ADR-0010: failures are attributed to the owning implementation and are not patched in Wright.

## Dependencies

- Pinned upstream OverPy `9.7.10` oracle available to the grader environment.
- Ability to provision the OPY provider before an offline run.
- An agent adapter for the first evaluation agent; the multi-model evaluation is run by the maintainers.
- `opy-rs#410` and `opy-rs#411` are known engine disagreements the benchmark is expected to reproduce; they do
  not block this spec.

## Unresolved questions

- Q-001 [product]: which models, reasoning settings, and total budget the first full run uses; owner PM.
- Q-002 [product]: is Tier 2 required for acceptance, or only Tier 1; owner PM.
- Q-003 [verification]: rubric-based judgment (REQ-011) in scope for the first version, or deferred; owner QA.
- Q-004 [architecture]: the harness now lives in `benchmarks/agent` as small modules beside
  `agent_bench.py` (grading, trace, report, adapters); confirm or redirect; owner Architect.
- Q-005 [verification]: wiki snapshots use the mirror's category pages and verified per-article content hashes;
  derived skills also verify their content hash. Both remain local under the source terms. Any distribution
  needs explicit source permission and an Architect ownership decision; it is outside this benchmark batch.
- Q-006 [verification]: the common tokenizer used for cross-model attribution (REQ-021) and how its error is
  reported; owner QA.
- Q-007 [verification]: skill-retrieval metrics beyond aggregate usage (unique files read, repeated reads,
  content tokens read, and how to judge which articles were needed) require transcript normalization and a
  defined estimator. Decide from the baseline pilot before adding requirements or tuning either guide.
