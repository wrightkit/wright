# Agent Benchmark

- Contract: `wright-agent-bench/v2`
- Harness: [`benchmarks/agent/agent_bench.py`](../benchmarks/agent/agent_bench.py)
- Design and requirements: [`SPEC-414`](specs/SPEC-414-agent-benchmark-comparison.md)

The benchmark answers one product question: can a general coding agent, with no
Workshop-specific prompt or skill injection, use a project and Wright to
complete realistic Workshop work correctly? It measures Wright's discoverable
semantic surface; it is not a model leaderboard, and one stochastic run is not
evidence of correctness (use `--trials`).

## Allowed agent context

- The scenario workspace: the seed project only.
- The scenario prompt, delivered on the agent's stdin. It states the
  requirement in user terms and never names Wright commands or Workshop APIs.
- In the `bin` condition, the released `wright` CLI and `wright serve`
  session on `PATH`.

Not allowed in the primary condition: a Workshop/OverPy/OSTW system prompt or
skill pack, a generated API reference, or task-specific hints. The agent,
model, and version are recorded (`--agent-id`); the contract does not depend
on a vendor. Other conditions below are experiments and are labeled as such.

## Conditions

A cell is `<wright>/<knowledge>/<network>`. The task and prompt are identical
in every cell.

| Factor | Levels |
| --- | --- |
| `wright` | `none`: no directory providing `wright` is on `PATH`. `bin`: `wright` on `PATH` through a tracing shim. `bin+skill`: `bin`, plus the guide directory given by `--skill-dir`, which the adapter installs. |
| `knowledge` | `none`; `wiki`: `--wiki-dir` is linked read-only as `./wiki` (never counted as an edit); `web`: the adapter enables its web tools. |
| `network` | `off` or `on`; `web` requires `on`. |

Each run is scrubbed: a fresh `HOME`, an allowlisted environment (`--env-pass`
names host variables to keep), and no host instruction files. Two canaries run
before the agent; a failed canary marks the run `invalid` and it is excluded
from results: `wright` must not be reachable under level `none`, and
`--canary-cmd` (a command that must fail when the network is `off`) must fail
in the agent environment. A determined agent can still find a Wright binary
elsewhere on disk, so run `none` in a clean environment when that matters.

## Adapters

`--agent-cmd` is a shell command run in the workspace with the prompt on stdin.
The harness describes the cell through environment variables, and the adapter
enforces it: `BENCH_WRIGHT`, `BENCH_KNOWLEDGE`, `BENCH_NETWORK`,
`BENCH_SKILL_DIR` (only for `bin+skill`), `BENCH_HOST_PATH` (the unscrubbed
`PATH`, for locating the agent binary itself; do not pass it to the agent), and
`BENCH_RUN_DIR`. The adapter reports, all optional:

| Path (env var) | Content |
| --- | --- |
| `BENCH_USAGE` | JSONL, one row per model turn: `t` (epoch seconds), `input`, `output`, `cache_read`, `cache_write`, `reasoning`, `context`, `context_limit` |
| `BENCH_TRANSCRIPT` | normalized JSONL of the agent's events |
| `BENCH_CONTEXT` | `{"loaded": [...]}`; a loaded item other than the expected guide invalidates the run |

Exit code 75 means a provider or infrastructure failure: the harness retries
the trial (`--infra-retries`) and records the retries. Any other non-zero exit
is reported as an agent or infrastructure failure in the report diagnostics.
[`adapters/claude_code.py`](../benchmarks/agent/adapters/claude_code.py) is the
reference adapter.

## Scenarios

`benchmarks/agent/scenarios/<id>/` contains `scenario.json`, `prompt.md`,
`seed/` (the initial workspace), `reference/` (files overlaid on the seed
to form a passing solution), and optional `negative/<name>/` overlays.
`scenario.json` fields:

| Field | Meaning |
| --- | --- |
| `id`, `family`, `language` | Identity; `language` is `workshop`, `opy`, or `ostw`, and a scenario may use a language only once its owner declares the needed capability supported |
| `entry` | Source file that Wright checks and that is snapshotted after each write (`watch` overrides the file list) |
| `writable` | Files the agent may change; any other change is reported as an unsafe edit |
| `runtimeOnly` | Claims that only the Overwatch runtime can verify; reported as unverified, never as passed |
| `stabilityRisk` | `true` when finishing safely needs `lint` or `analyze`, not only `check` (expectation E03) |
| `split` | Optional `train` or `test`, for reports and guide tuning. Scenarios that share a requirement or family across languages take the same split, so a held-out scenario is never a translation of a training one |
| `source`, `referenceNote` | Optional provenance of the requirement and of the reference solution; the reference only calibrates the checks and is never shown to agents |
| `negatives` | `{name: {"fails": [check ids]}}`; the overlay must fail exactly those checks |
| `checks` | Deterministic checks, each with `id`, `kind`, and `layer` |

Check kinds: `check` (`wright check` reports no errors), `lint` (at most `max`
findings with lint `code`), `symbols` (at least `min` symbols of `symbolKind`
via `wright serve`), `contains` / `absent` (source text, `text` may be a list
of alternatives, `min`/`max` occurrences), `answer` (`answer.json` key equals
`expected`), `oracle` (the pinned upstream OverPy compiler accepts the entry),
`wright-compile` (`wright compile` succeeds), and `compiled-contains` /
`compiled-absent` (regexes `all` and `any` over the compiled Workshop text of
`source` `oracle` or `wright`). Prefer structural checks; a regex over compiled
text needs a `reference` that matches and a `negative` that does not. `layer`
names what a failure implicates: `agent` for a requirement the produced work
does not meet, or `workshop-rs` / `opy-rs` / `deltin-rs` / `wright` for
validity or analysis results owned by that layer.

## Grading authorities

Every result records validity per authority, never collapsed into one verdict:
`wrightCheck`, `wrightCompile`, `workshopCheck` (the emitted Workshop text
through `workshop-rs`), and, for `.opy` entries, `oracle`. The oracle is the
pinned upstream compiler under `benchmarks/agent/oracle/` (`node` plus
`agent_bench.py setup-oracle`); when it is not installed the status is
`unavailable` and an `oracle` check fails explicitly instead of passing.
When Wright and the oracle disagree, `disagreement` names the direction and both
diagnostics, which is the reproducer for an owner Issue. Results carry the
grader hash (`grader.hash`); a grader change means regrading every run.

## Scenario validity

`agent_bench.py validate` requires every scenario's reference to pass all checks,
its untouched seed to fail at least one, and every negative to fail exactly the
checks it names. A reference that fails a check is a product or engine gap named
by that check's `layer`, not an agent failure. This runs in the CI benchmark
job; running agents does not.

## Running

```sh
python3 benchmarks/agent/agent_bench.py run <scenario> --agent-id LABEL --agent-cmd CMD \
    --wright-level bin --knowledge none --network off --trials 5
python3 benchmarks/agent/agent_bench.py matrix matrix.json   # agents x cells x scenarios x trials
python3 benchmarks/agent/agent_bench.py report target/agent-bench [--regrade]
```

[`matrix.example.json`](../benchmarks/agent/matrix.example.json) is the Tier 1 matrix. `matrix.json` lists `agents` (`{id, cmd}`), `cells`, optional `scenarios`,
`trials`, `parallel`, `seed` (run order is shuffled by it), and `options`
(`skill_dir`, `wiki_dir`, `env_pass`, ...). Finished runs are skipped, so an
interrupted matrix resumes.

## Result

`agent_bench.py run` writes `<out>/<scenario>/<agent-id>/<cell>-<trial>/result.json`,
with the workspace, `agent.log`, snapshots, and the Wright trace beside it.

| Field | Meaning |
| --- | --- |
| `condition`, `agent`, `environment`, `grader` | Cell, agent label/command/exit/seconds, OS/Python/Wright version, grader hash and oracle version |
| `checks`, `passed`, `usable`, `failedLayers` | Per-check outcome; `usable` is `passed` with no error-severity lint finding |
| `authorities`, `disagreement` | Validity per authority and any Wright/oracle disagreement |
| `diagnostics`, `lintFindings`, `unsafeEdits` | Remaining `wright check` diagnostics, lint rule codes, files changed outside `writable` |
| `unverifiedRuntimeClaims` | The scenario's `runtimeOnly` claims |
| `wrightUse` | Invocations by subcommand, failures, exits of 3 or 4 (candidate owner or environment gaps), and estimated output tokens per command |
| `friction`, `expectations` | Usage errors, unknown subcommands, help lookups, retries, malformed `serve` requests, identical repeats; expectation E01-E12 verdicts |
| `snapshots` | Strict validity of each snapshot of the entry, first valid index, and valid-to-invalid regressions |
| `usage`, `context` | Turns, tokens by kind, peak context (and its share of the limit), tokens to first valid; loaded context |
| `invalid`, `infraRetries` | Present when the run was excluded or retried |

`wrightUse` is recorded per CLI invocation; `wright serve` sessions are teed
line by line into the trace. Comparing cells for the same scenario shows what
Wright adds. Exit 3 or 4 entries, disagreements, and failed `layer` values are
the input for owner Issues.

Expectations E05, E07, E09, and E10 need the normalized agent transcript and
report `unavailable` until an adapter provides it. Estimated tokens for Wright
output use four bytes per token; provider-reported usage is authoritative.

## Report

`agent_bench.py report` writes `report.md` and `summary.json`: usable and passed
counts with Wilson 95% intervals, tokens per run and per usable result, peak
context, paired comparison against `none/none/off` (same scenario, agent, and
trial; token comparison only where both are usable), per-scenario and per-split
tables, expectation rates, friction, output size per command, and diagnostics.
Diagnostics flag headroom (baseline usable rate of at least 95%), infrastructure
failures, invalid runs, trial variance, and, with `--regrade`, a grader that
gives different verdicts on the same stored workspace.

## Cadence

The benchmark does not gate pull requests. Run it manually or on a schedule once
its cost and stability are understood.
