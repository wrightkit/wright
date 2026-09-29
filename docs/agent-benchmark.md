# Agent Benchmark

- Contract: `wright-agent-bench/v1`
- Harness: [`benchmarks/agent/agent_bench.py`](../benchmarks/agent/agent_bench.py)

The benchmark answers one product question: can a general coding agent, with no
Workshop-specific prompt or skill injection, use a project and Wright to
complete realistic Workshop work correctly? It measures Wright's discoverable
semantic surface; it is not a model leaderboard, and one stochastic run is not
evidence of correctness (use `--trials`).

## Allowed agent context

- The scenario workspace: the seed project only.
- The scenario prompt, delivered on the agent's stdin. It states the
  requirement in user terms and never names Wright commands or Workshop APIs.
- In the `wright` condition, the released `wright` CLI and `wright serve`
  session on `PATH`.

Not allowed: a Workshop/OverPy/OSTW system prompt or skill pack, a generated
API reference, or task-specific hints. The agent, model, and version are
recorded (`--agent-id`); the contract does not depend on a vendor.

## Conditions

Both conditions use the same workspace and prompt. They differ only in `PATH`:
`baseline` removes every directory that provides a `wright` executable;
`wright` prepends a logging shim for the binary under test. A determined agent
can still find a Wright binary elsewhere on disk, so run baselines in a clean
environment when that matters.

## Scenarios

`benchmarks/agent/scenarios/<id>/` contains `scenario.json`, `prompt.md`,
`seed/` (the initial workspace), and `reference/` (files overlaid on the seed
to form a passing solution). Families: `greenfield`, `understanding`,
`modification`, `diagnosis`. `scenario.json` fields:

| Field | Meaning |
| --- | --- |
| `id`, `family`, `language` | Identity; `language` is `workshop`, `opy`, or `ostw`, and a scenario may use a language only once its owner declares the needed capability supported |
| `entry` | Source file that Wright checks |
| `writable` | Files the agent may change; any other change is reported as an unsafe edit |
| `runtimeOnly` | Claims that only the Overwatch runtime can verify; reported as unverified, never as passed |
| `checks` | Deterministic checks, each with `id`, `kind`, and `layer` |

Check kinds: `check` (`wright check` reports no errors), `lint` (at most `max`
findings with lint `code`), `symbols` (at least `min` symbols of `symbolKind`
via `wright serve`), `contains` / `absent` (source text, `text` may be a list
of alternatives, `min`/`max` occurrences), and `answer` (`answer.json` key equals
`expected`). `layer` names what a failure implicates: `agent` for a requirement
the produced work does not meet, or `workshop-rs` / `opy-rs` / `deltin-rs` /
`wright` for validity or analysis results owned by that layer.

## Scenario validity

`agent_bench.py validate` requires every scenario's reference to pass all checks
and its untouched seed to fail at least one. A reference that fails a check is a
product or engine gap named by that check's `layer`, not an agent failure. This
runs in the CI benchmark job; running agents does not.

## Result

`agent_bench.py run <scenario> --agent-cmd CMD --agent-id LABEL` runs each
condition and writes `<out>/<scenario>/<condition>-<trial>/result.json`, with
the workspace, `agent.log`, and Wright trace beside it.

| Field | Meaning |
| --- | --- |
| `checks`, `passed`, `failedLayers` | Per-check outcome and the layers implicated by failures |
| `diagnostics` | Remaining `wright check` diagnostics with their owner origin |
| `unsafeEdits` | Files changed outside `writable` |
| `wrightUse` | Wright invocations by subcommand, failed invocations (correction rounds), and exits of 3 or 4 (unsupported or internal failures: candidate owner or environment gaps) |
| `unverifiedRuntimeClaims` | The scenario's `runtimeOnly` claims |
| `agent`, `environment` | Agent label, command, exit, duration; OS, Python, Wright version, timestamp |

`wrightUse` is recorded per CLI invocation; requests inside one `wright serve`
session are not itemized. Comparing `baseline` and `wright` results for the same
scenario shows what Wright adds. Exit 3 or 4 entries and failed `layer` values
are the input for owner Issues.

## Cadence

The benchmark does not gate pull requests. Run it manually or on a schedule once
its cost and stability are understood.
