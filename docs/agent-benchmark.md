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
| `knowledge` | `none`; `wiki`: a pinned snapshot given by `--wiki-dir` and linked as `./wiki` (never counted as an edit); `wiki-skill`: the separate community guide given by `--wiki-skill-dir`, installed through the agent's skill mechanism without requiring Wright; `web`: the adapter enables its web tools. |
| `network` | `off` or `on`; `web` requires `on`. |

`agent_bench.py wiki-snapshot [--dir DIR]` builds the `wiki` snapshot from the
mirror at `md.wrightkit.dev`: it lists category pages and fetches each distinct article once
(through `curl`, because the mirror rejects Python's HTTP client with 403), and
writes `articles/`, `NOTICE.txt`, and `SNAPSHOT.json` with per-document
hashes and a `snapshotSha256`. A snapshot is never overwritten, a run with
`knowledge` `wiki` verifies the article hashes and snapshot identity before starting,
and the result records the verified identity in `environment.wiki`.
The default categories are actions, values, events, constants, and references;
add `tutorials` through `--categories` for a separate second-tier experiment.
The mirror's manifest is incomplete and is not the crawl source.

`wiki-skill` builds a separate `workshop-wiki` skill with a short `SKILL.md`, category
indexes, and individual articles. It takes a pinned snapshot, the workshop-rs catalog,
and the opy-rs manifest; OverPy spellings are included only when found in the pinned
upstream oracle. The generated skill is community guidance, not canonical semantic
authority. Its content hash is verified before a run and recorded with the snapshot
hash in `environment.wikiSkill`. Changed or missing pinned content is refused.
The `./wiki` symlink does not enforce filesystem read-only access; content hashes
are checked again when producing the result, but this is not a filesystem sandbox.

```sh
python3 benchmarks/agent/agent_bench.py wiki-skill \
    --snapshot /abs/path/pinned-wiki --out-dir /abs/path/local/workshop-wiki \
    --catalog /abs/path/workshop-rs/crates/workshop-rs/src/catalog/data/catalog.json \
    --opy-manifest /abs/path/opy-rs/crates/opy-rs/src/manifest/data/manifest.json
```

The output directory must be named `workshop-wiki` and must not exist. Run
`setup-oracle` first. Snapshots and derived skills are local benchmark material;
do not commit or distribute them. The [Workshop.codes Terms of Service](https://workshop.codes/tos)
apply to the source content; generating a skill grants no additional permission.

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
`BENCH_SKILL_DIR` (only for `bin+skill`), `BENCH_WIKI_SKILL_DIR` (only for `wiki-skill`), `BENCH_HOST_PATH` (the unscrubbed
`PATH`, for locating the agent binary itself; do not pass it to the agent), and
`BENCH_RUN_DIR`. The adapter reports, all optional:

| Path (env var) | Content |
| --- | --- |
| `BENCH_USAGE` | JSONL, one row per model turn: `t` (epoch seconds), `input`, `output`, `cache_read`, `cache_write`, `reasoning`, `context`, `context_limit` |
| `BENCH_TRANSCRIPT` | normalized JSONL of the agent's events |
| `BENCH_CONTEXT` | `{"loaded": [...]}`; a loaded item other than the condition's expected `wright` and/or `workshop-wiki` guide invalidates the run |

An adapter that cannot observe loaded context omits `loaded` and reports its
audit limitation instead. The harness and report mark that field unreported;
installing a skill is not evidence that the agent loaded it. Built-in skills
belong to each agent's default context and are recorded separately when observable.

Exit code 75 means a provider or infrastructure failure: the harness retries
the trial (`--infra-retries`) and records the retries. Any other non-zero exit
is reported as an agent or infrastructure failure in the report diagnostics.

The scenario `prompt.md` is the only text the harness gives an agent: it adds no
system prompt, hint, or Workshop context. Each agent keeps its own default system
prompt, which is part of what is measured. The adapter, not the harness, must
keep host configuration out of the run, and it reports what loaded through
`BENCH_CONTEXT`.

| Adapter | Agent | Notes |
| --- | --- | --- |
| [`claude_code.py`](../benchmarks/agent/adapters/claude_code.py) | Claude Code | `BENCH_MODEL` selects the model. Removes web tools unless knowledge is `web`. |
| [`pi.py`](../benchmarks/agent/adapters/pi.py) | pi | `BENCH_MODEL=provider/id`. Disables context files, extensions, and skill discovery, and loads the guide explicitly. Providers registered by an extension need `BENCH_PI_EXTENSIONS`; web tools come from `BENCH_PI_WEB_EXTENSIONS`. |
| [`devin.py`](../benchmarks/agent/adapters/devin.py) | Devin CLI | `BENCH_MODEL` (for example `swe-2-max`). Runs with an isolated `HOME` holding only the Devin credentials, a config that reads no other tool's rules or skills, and MCP tools denied. Managed plugin skills are listed apart from `loaded`. |
| [`codex.py`](../benchmarks/agent/adapters/codex.py) | Codex CLI | `BENCH_MODEL` and `BENCH_THINKING`. Isolated `HOME`/`CODEX_HOME`, user config and exec rules ignored, workspace skills installed under `.agents/skills`. Session token events supply per-model-call usage and observed skill context; built-ins are listed apart. Web search, Apps, and plugin discovery are disabled; any observed MCP call invalidates the trial. |
| [`agy.py`](../benchmarks/agent/adapters/agy.py) | Antigravity CLI | `BENCH_MODEL` (for example `gemini-3.8-flash-high`) and `BENCH_THINKING`. Isolated `HOME` with only authentication files, workspace skills under `.agents/skills`, MCP/browser access denied and URL reads denied outside `web`. Observed built-in web tools under network `off` stop and invalidate the trial; URL permissions do not cover search. Streaming step usage is recorded. The CLI does not export observed loaded skills or context limits; these remain unreported. |

The pi adapter also uses an isolated `HOME` containing only its authentication
files. Explicit provider extensions remain referenced by path, not copied with
host settings. Reasoning is counted once: Codex and Antigravity include reasoning
in their output counters, so adapters split it out before aggregation. Antigravity
reports uncached input separately from cached reads; Codex reports inclusive input.

Set the model in `--agent-cmd` (the environment is scrubbed), and give the
adapter and skill paths as absolute paths, because the agent runs in the
workspace: `--agent-cmd "BENCH_MODEL=openai-codex/gpt-6-luna python3 /abs/path/adapters/pi.py"`.
Pass `--env-pass HOME` when the agent authenticates from the real home directory.

Two checks protect the context. The workspace must not sit below a directory
that holds instruction files (`AGENTS.md`, `CLAUDE.md`, and similar), because
agents discover them by walking up; the default `--out` is
`~/.cache/wright-agent-bench` for that reason, and a violation marks the run
`invalid` (`--no-ancestor-check` disables it). Network `off` is enforced only
when `--canary-cmd` is given and fails inside the agent environment; without it
the result records `networkEnforcement: declared-only`, which is what the shell
tools of pi and Devin provide today.

Every task receives a common workspace boundary instruction. Network `off` adds an explicit prohibition on web tools, URL fetches, and downloads. This instruction is not network enforcement.

On macOS, `--file-sandbox` applies `sandbox-exec` to the adapter and all descendant
processes: filesystem writes are restricted to that trial's output directory
(plus device streams), and `TMPDIR` points inside it. Unsupported hosts fail
instead of silently running without protection. This protects host files; it
does not enforce network isolation or prevent read access to host files. Model
account usage, CPU and disk consumption remain shared with the host. Provider
failures returned as exit 75 are listed separately and excluded from outcome
metrics.

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

For a local offline-declared pilot, use
[`matrix.pilot.example.json`](../benchmarks/agent/matrix.pilot.example.json): one
agent at a time, five cells including `none/wiki-skill/off`, two scenarios with
different requirement families, three trials, and a 3600-second timeout. Replace
the absolute path placeholders and choose the adapter/model before running. This
is 30 trials per agent, not the full-suite evaluation. Start with a single trial
to check cost, usage, `context.loaded`, and `networkEnforcement`, then use a fresh
output directory for the randomized matrix. Complete pi GPT before switching to
pi Gemini and then Devin; Gemini needs its provider extension.

This pilot omits `web`; it does not establish network isolation when enforcement
is `declared-only`. It measures baseline headroom and variance before guide tuning.
Skill-retrieval attribution (files and content tokens read), wiki-only scenarios,
and web URL contamination analysis remain separate follow-up work; a successful
pilot does not establish those requirements.

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
| `invalid`, `infraRetries`, `networkEnforcement` | Present when the run was excluded or retried; whether network `off` was checked by a canary or only declared |

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
