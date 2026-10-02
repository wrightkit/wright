# Agent Benchmark

- Contracts: `wright-agent-bench/v3` (a run result) and `wright-agent-score/v1` (a score card)
- Harness: [`benchmarks/agent/agent_bench.py`](../benchmarks/agent/agent_bench.py)
- Run it from a shell: [agent-benchmark-howto.md](agent-benchmark-howto.md)
- Design and requirements: [`SPEC-414`](specs/SPEC-414-agent-benchmark-comparison.md)

The benchmark answers one product question: can a general coding agent, with no
Workshop-specific prompt or skill injection, use a project and Wright to
complete realistic Workshop work correctly? It measures Wright's discoverable
semantic surface; it is not a model leaderboard, and one stochastic run is not
evidence of correctness (use `--trials`).

A score is a credible reference, not a measure of an agent's ability. It holds for one
Wright, skill set, suite, agent, model, and protocol, and it is meant to show what
the prompt, skills, and tools did in that setup. Every report therefore lists, from
what the adapter observed, the CLI version, model, tools, and loaded skills each
agent actually had (`Agent setup`).

## Allowed agent context

- The scenario workspace: the seed project only.
- The scenario prompt, delivered on the agent's stdin. It states the
  requirement in user terms and never names Wright commands or Workshop APIs.
- In the `wright` tool condition, the released `wright` CLI and `wright serve`
  session on `PATH`; in the `overpy` tool condition, the `overpy` compiler.

Not allowed in the primary condition: a Workshop/OverPy/OSTW system prompt or
skill pack, a generated API reference, or task-specific hints. The agent,
model, and version are recorded (`--agent-id`); the contract does not depend
on a vendor. Other conditions below are experiments and are labeled as such.

## Conditions

A cell is `tool[+skill...]/knowledge/network`, for example `wright+wright-skill/none/off`;
the baseline is `none/none/off`. The task and prompt are identical in every cell.

| Factor | Levels |
| --- | --- |
| `tool` | `none`: neither tool is reachable. `wright`: `wright` on `PATH` through a tracing shim. `overpy`: the pinned `overpy` compiler through a tracing shim (OverPy scenarios only). A tool that is not part of the condition is hidden from `PATH`, and a canary fails the run if it is reachable. |
| `skills` | Any of `wright-skill` (how to use Wright), `workshop-skill` (the progressive wiki knowledge skill, see below), `opy-skill` (how to write OverPy and use `overpy`; OverPy scenarios only), and `workshop-format-skill` (the raw Workshop source format; Workshop scenarios only, a local benchmark control). Each is given by `--skill-dir NAME=DIR` and installed through the agent's skill mechanism. |
| `knowledge` | `none`; `wiki`: a pinned snapshot given by `--wiki-dir`, copied into the workspace as `./wiki` (a real copy, because tools such as `rg` do not follow symlinks; never counted as an edit); `web`: the adapter enables its web tools. |
| `network` | `off` or `on`; `web` requires `on`. |

Language decides which cells apply: the `overpy` tool, `opy-skill`, and
`workshop-format-skill` apply only to scenarios of their language, and `matrix`
skips the other pairs and prints them as not applicable. They are not failures
and are not counted.

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

The `workshop-skill` is built by `agent_bench.py wiki-skill`. It is a separate skill (generated name `workshop-wiki`) with a short `SKILL.md`, category
indexes, and individual articles. It takes a pinned snapshot, the workshop-rs catalog,
and the opy-rs manifest; OverPy spellings are included only when found in the pinned
upstream oracle. The generated skill is community guidance, not canonical semantic
authority. Its content hash is recorded with its build record in `environment.skills`; content that
differs from its `BUILD.json` is refused. The `./wiki` copy is not read-only, and `./wiki`
is not counted as an edit, so edits there are not detected.

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
from results: a tool outside the condition must not be reachable, and
`--canary-cmd` (a command that must fail when the network is `off`) must fail
in the agent environment. A determined agent can still find a tool binary
elsewhere on disk, so run `none` in a clean environment when that matters.

## Adapters

`--agent-cmd` is a shell command run in the workspace with the prompt on stdin.
The harness describes the cell through environment variables, and the adapter
enforces it: `BENCH_TOOL`, `BENCH_SKILLS` (names), `BENCH_SKILL_DIRS` (one
directory per installed skill, separated by the path separator), `BENCH_KNOWLEDGE`,
`BENCH_NETWORK`, `BENCH_HOST_PATH` (the unscrubbed
`PATH`, for locating the agent binary itself; do not pass it to the agent), and
`BENCH_RUN_DIR`. The adapter reports, all optional:

| Path (env var) | Content |
| --- | --- |
| `BENCH_USAGE` | JSONL, one row per model turn: `t` (epoch seconds), `input`, `output`, `cache_read`, `cache_write`, `reasoning`, `context`, `context_limit` |
| `BENCH_TRANSCRIPT` | normalized JSONL of the agent's events |
| `BENCH_CONTEXT` | `{"loaded": [...]}`; a loaded item other than the condition's skills (by the name in each `SKILL.md`) invalidates the run |
| `BENCH_AGENT_INFO` | `{"agent", "model", "effort", "tools", ...}`: the agent system actually used and the tools it exposed, copied into the result as `agentInfo`, so harness differences can be disclosed |

An adapter that cannot observe loaded context omits `loaded` and reports its
audit limitation instead. The harness and report mark that field unreported;
installing a skill is not evidence that the agent loaded it. Built-in skills
belong to each agent's default context and are recorded separately when observable.

Exit code 75 means a provider or infrastructure failure: the harness retries
the trial (`--infra-retries`) and records the retries. Every result has a
`status`: `completed`, `provider-interrupted` (exit 75 after the retries),
`timeout`, `agent-error` (any other non-zero exit), or `invalid` (a canary, an
unexpected loaded context, or an unavailable required grader).

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
| [`opencode.py`](../benchmarks/agent/adapters/opencode.py) | opencode | `BENCH_MODEL=provider/model` (as `opencode models` lists it) and `BENCH_THINKING` as the model variant. Isolated `HOME` and XDG directories holding only the credentials; `--pure`; Claude Code instructions and the real home's external skills are disabled by environment variable because opencode reads them regardless of `HOME`; skills installed under `.opencode/skills`; web tools denied outside `web`. Per-step usage comes from `step_finish` events; the tool list is what the agent used. |
| [`grok.py`](../benchmarks/agent/adapters/grok.py) | Grok CLI | `BENCH_MODEL` (a `grok models` id) and `BENCH_THINKING` as reasoning effort. Isolated `GROK_HOME` holding only the login and the skills; prompt sent `--verbatim`; subagents disabled; web search disabled outside `web`. Tools, skills, and the context window come from the stream's init and result lines. |
| [`direct.py`](../benchmarks/agent/adapters/direct.py) | none (built-in loop) | See below. |

The pi adapter also uses an isolated `HOME` containing only its authentication
files. Explicit provider extensions remain referenced by path, not copied with
host settings. Reasoning is counted once: pi, Codex, and Antigravity include reasoning
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

On macOS, `--file-sandbox` applies `sandbox-exec` to the adapter and all descendant
processes: filesystem writes are restricted to that trial's output directory
(plus device streams), and `TMPDIR` points inside it. Unsupported hosts fail
instead of silently running without protection. This protects host files; it
blocks reads of `AGENTS.md`, `CLAUDE.md`, and `GEMINI.md` outside the trial workspace
to prevent tool-path rule discovery from contaminating context. It also hides from
the agent the scenarios (their reference solutions), every other run under `--out`,
the wiki snapshot, skills outside the condition, and any `--deny-read PATH`, such as
the checkouts of the repositories under test. The adapter, harness code, the `wright`
binary directory, and the condition's skills stay readable. The result lists the
denied paths in `fileReadEnforcement`. Without this, agents find the answer keys and the
owner repositories on the host (seen in practice), so `evaluate` turns the sandbox on by
default (`--no-file-sandbox` disables it). Other host reads remain possible, and
network isolation is not enforced. Model
account usage, CPU and disk consumption remain shared with the host. Provider
failures returned as exit 75 are listed separately and excluded from outcome
metrics. The harness never edits the task prompt: network `off` and the workspace
boundary are enforced or disclosed, not stated to the agent.

## Scenarios

`benchmarks/agent/scenarios/<id>/` contains `scenario.json`, `prompt.md`,
`seed/` (the initial workspace), `reference/` (files overlaid on the seed
to form a passing solution), and optional `negative/<name>/` overlays.
`scenario.json` fields:

| Field | Meaning |
| --- | --- |
| `id`, `family`, `language` | Identity; `language` is `workshop`, `opy`, or `ostw`; a scenario may use a language only once its owner declares the needed capability supported, and `language` selects its score track and which language tools and skills apply |
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
    --tool wright --skills wright-skill --skill-dir wright-skill=DIR \
    --knowledge none --network off --trials 5
python3 benchmarks/agent/agent_bench.py matrix matrix.json   # agents x cells x scenarios x trials
python3 benchmarks/agent/agent_bench.py report target/agent-bench [--regrade] [--reference none/none/off]
python3 benchmarks/agent/agent_bench.py score target/agent-bench   # Wright Agent Score cards
```

[`matrix.example.json`](../benchmarks/agent/matrix.example.json) is the Tier 1 matrix. `matrix.json` lists `agents` (`{id, cmd}`), `cells`, optional `scenarios`,
`trials`, `parallel`, `seed` (run order is shuffled by it), and `options`
(`skill_dirs` as `{name: dir}`, `wiki_dir`, `env_pass`, ...). Cells not applicable to a scenario's language are skipped; the matrix stops after two consecutive provider interruptions and exits 3 when any occurred. Finished runs are skipped, so an
interrupted matrix resumes.

For a local offline-declared pilot, use
[`matrix.pilot.example.json`](../benchmarks/agent/matrix.pilot.example.json): one
agent at a time, five cells including `workshop-skill`, two scenarios with
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
| `condition`, `agent`, `agentInfo`, `protocol`, `status` | Cell with its `label`; agent label/command/exit/seconds; the system and tools the adapter reports; timeout and infra retries; `completed`, `provider-interrupted`, `timeout`, `agent-error`, or `invalid` |
| `environment`, `grader` | OS/Python/Wright version and sha256, harness commit, suite version and hash, skill identities (`skills`), wiki identity, grader hash and oracle version |
| `checks`, `passed`, `usable`, `failedLayers` | Per-check outcome; `usable` is `passed` with no error-severity lint finding and no unsafe edits; `usableReason` names the blocking cause |
| `authorities`, `disagreement` | Validity per authority and any Wright/oracle disagreement |
| `diagnostics`, `lintFindings`, `unsafeEdits` | Remaining `wright check` diagnostics, lint rule codes, files changed outside `writable` |
| `unverifiedRuntimeClaims` | The scenario's `runtimeOnly` claims |
| `toolUse` | Per tool (`wright`, `overpy`): invocations by subcommand, failures, exits of 3 or 4 (candidate owner or environment gaps), and estimated output tokens per command |
| `friction`, `expectations` | Usage errors, unknown subcommands, help lookups, retries, malformed `serve` requests, unparsed `serve` responses, identical repeats; expectation E01-E12 verdicts |
| `snapshots` | Strict validity of each snapshot of the entry, first valid index, and valid-to-invalid regressions |
| `usage`, `context` | Turns, tokens by kind, peak context (and its share of the limit), tokens to first valid; loaded context |
| `invalid`, `infraRetries`, `networkEnforcement` | Present when the run was excluded or retried; whether network `off` was checked by a canary or only declared |

`toolUse` is recorded per CLI invocation through the shim; `wright serve` sessions are teed
line by line into the trace. Comparing cells for the same scenario shows what
Wright adds. Exit 3 or 4 entries, disagreements, and failed `layer` values are
the input for owner Issues.

Expectations E05, E07, E09, and E10 need the normalized agent transcript and
report `unavailable` until an adapter provides it. Estimated tokens for Wright
output use four bytes per token; provider-reported usage is authoritative.

## Report

`agent_bench.py report` writes `report.md` and `summary.json`: usable and passed
counts with scenario-clustered 95% intervals (resampling scenarios, then trials), tokens per run and per usable result, peak
context, paired comparison against `--reference` (default `none/none/off`) (same scenario, agent, and
trial; token comparison only where both are usable), per-scenario and per-split
tables, expectation rates, friction, output size per command, and diagnostics.
Diagnostics flag headroom (baseline usable rate of at least 95%), infrastructure
failures, invalid runs, trial variance, and, with `--regrade`, a grader that
gives different verdicts on the same stored workspace.

## Score

`agent_bench.py score` writes `score.json` and `score.txt`: one card per language
track (Wright Workshop Agent Score, Wright OPY Agent Score; contract
`wright-agent-score/v1`). A card uses only the canonical cell
`wright+wright-skill/none/off` on `test` scenarios, macro-averages the usable rate
over scenarios, and gives a two-stage bootstrap 95% interval (10,000 draws, seed
467) and Pass^k as a secondary figure. Provider-interrupted, invalid, and
agent-error runs are published as exclusions; timeouts count. The score is
refused if the runs differ in Wright binary, skill hashes, suite hash, agent,
model, effort, or protocol, and it is marked provisional with fewer than eight
held-out scenarios, missing scenarios, or unequal trials. The card discloses
`networkEnforcement` (`declared-only` or `canary-checked`).

## Evaluating without an agent harness

Quick start. Put your paths in `~/.config/wright-agent-bench/config.json` once
(`WRIGHT_BENCH_CONFIG` overrides the location):

```json
{"skill_dirs": {"wright-skill": "~/skills/skills/wright", "opy-skill": "~/skills/skills/overpy"},
 "deny_read": ["~/Repos", "~/.agents", "~/.claude"],
 "wiki_dir": "~/.local/share/wright-agent-bench/wiki"}
```

then run one command per agent. `--dry-run` checks the setup (wright binary, the
agent CLI, credentials, skill directories, the oracle) and prints the plan without
running anything; `wright` is taken from `PATH` unless `--wright` or the config says
otherwise, and the credentials each adapter needs are passed through automatically.

```sh
python3 benchmarks/agent/agent_bench.py evaluate --adapter devin --model swe-2-max --dry-run
python3 benchmarks/agent/agent_bench.py evaluate --adapter devin --model swe-2-max
```

`agent_bench.py evaluate --adapter ADAPTER --model MODEL --skill-dir wright-skill=DIR`
is the one-command entry: it writes `matrix.json`, runs the cells, then writes
`report.md`, `summary.json`, `score.json`, `score.txt`, and `RESULTS.md` into
`<out>/<name>`. `--cells score` runs the canonical cell only; `--cells controls`
adds the baseline, `wright` without the skill, and, for OverPy scenarios, the
`overpy` controls (cells whose skill has no `--skill-dir` are skipped). It runs the
same from a terminal or from inside another agent's shell, because isolation comes
from the harness's scrubbed environment, not from its parent. It runs locally; CI
does not run it. Adapters for CLIs that keep credentials in the home directory (devin,
codex, opencode, grok, agy) need `--env-pass HOME` so they can copy them into their
isolated home.

`--adapter direct` is the built-in loop (`adapters/direct.py`) that needs no agent
harness: it calls a model API with one `bash` tool (and `fetch` only for
knowledge `web`), lists the installed skills by name and description, and records
exact usage and a full transcript. `BENCH_MODEL` is `anthropic/<model>`
(`ANTHROPIC_API_KEY`, optional `ANTHROPIC_BASE_URL`) or `openai/<model>`
(`OPENAI_API_KEY`, optional `OPENAI_BASE_URL`, so OpenAI-compatible endpoints work).
Pass the key variables with `--env-pass`. Its turn, time, and output limits are
recorded in `agentInfo.protocol`. It does not sandbox the network, so pair it with
`--canary-cmd`. Scores from `direct` and from product harnesses measure different
things and are not mixed.

### Running different models at different times

Each `evaluate` writes its own directory and scores only its own runs, so models
and agents can be run whenever quota allows, in any order. Put the runs side by side
with

```sh
python3 benchmarks/agent/agent_bench.py compare ~/.cache/wright-agent-bench/{devin-swe2-stage1,codex-luna-xhigh,pi-luna-xhigh}
```

which prints one table of scores, intervals, trials, and exclusions, and warns when
the runs differ in the Wright binary, skill contents, or suite (scenarios, grader,
oracle lock). Scores are comparable when it prints no warning. Keep the Wright
binary, skills, and scenarios unchanged between runs; changes to the rest of the
harness are disclosed in each card's `Harness` line and do not block comparison.
An interrupted run resumes by repeating the same command with the same `--name`.

## Cadence

The benchmark does not gate pull requests. Run it manually or on a schedule once
its cost and stability are understood.
