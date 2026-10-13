# ADR-0025: Essential default output and the agent-facing token standard

- Status: Accepted
- Date: 2026-10-13
- Related: [Issue #665](https://github.com/wrightkit/wright/issues/665), [Issue #672](https://github.com/wrightkit/wright/issues/672), [Issue #673](https://github.com/wrightkit/wright/issues/673), [Issue #671](https://github.com/wrightkit/wright/issues/671), [Issue #662](https://github.com/wrightkit/wright/issues/662), [ADR-0020](0020-native-agent-tool-adapter.md), [ADR-0022](0022-shared-surface-for-semantic-queries.md), [Agent contract](../agent-contract.md), [Agent-facing metrics](../agent-metrics.md)

## Context

Wright's text output already reports a verdict and what the reader must act on, and points to narrower commands for detail. Its JSON output and the `wright-agent/v1` operations return the complete underlying data instead. The agent guide directs agents to `--format json`, so agents pay for the dump.

Measured with 0.18.0 on bastion (307 rules), in estimated tokens:

| Command | Text | CLI JSON | Agent operation |
| --- | --- | --- | --- |
| `inspect` | ~270 | ~289k | ~128k |
| `analyze` | ~760 | ~192k | ~82k |
| `lint` | ~1.2k | ~3.4k | ~2k |
| `compile` | the program | ~110k | ~110k |

The JSON results carry every rule, symbol and reference with full span objects, lint configuration and rule lists, and the compiled program even when `-o` wrote it to a file. CLI JSON is also pretty-printed, which alone more than doubles its size. The metrics gate bounded only the opt-in `--brief` forms and compared everything else with a previous baseline, so the ~289k-token `inspect` result was recorded as the new baseline (#662).

ADR-0022 already requires that any agent-facing default is defined once for all surfaces. This ADR decides what that default is and how its cost is held.

## Decision

### Default output

1. **Every surface defaults to the verdict plus what the caller must act on.** This covers CLI text, CLI JSON, `wright serve` (stdio, JSON-RPC, MCP), and the agent operations. More detail is an explicit request.
2. **JSON reports the same content as text.** JSON is the structured machine interface for programs such as CI, scripts and editors; it is not a fuller mode. CLI JSON is emitted compact and stays byte-deterministic.
3. **Agents read text by default.** JSON is for results a program processes. Structured access (goal principle 4) remains available through JSON and the agent operations.
4. **Long lists are capped and say so.** A capped list reports its total and how to see the rest; an explicit flag (`--all`, agent field `all: true`) lifts the cap. Verdicts and exit codes are computed on the complete set.
5. **Full fact dumps leave the CLI and agent surfaces.** The detail is reached through the narrower queries (`inspect symbols`, `refs`, `cfg`, `callgraph`, `cost` and their agent operations). The in-process Rust API (`CompilerSession`) keeps its complete typed results; it has no token cost.
6. **`compile` writes the program to a file and reports only status and problems.** The default file is `<entry stem>.ow` in the current directory. An existing file is overwritten only after an interactive `y/N` confirmation, or with `--force` when there is no terminal; the input file is never overwritten. `--copy` also places the program on the clipboard. Neither text nor JSON contains the program text.

The opt-in `brief` form becomes the default and the `brief` flag and request field are removed. Per-command content and caps are specified in #672 and #673.

### Token standard

1. **The standard is the cost of a task, not of one call.** For each benchmark scenario family the recommended call sequence has a deterministic cumulative cost; that number is the standard. Per-operation sizes remain diagnostic.
2. **Cost must not grow linearly with project size.** The ratio between a scenario's cost on the largest corpus project and on its seed is reported; where it grows with size, the step that pulls everything is bounded.
3. **Scenario cost blocks only where it is deterministic.** CI measures against the latest language provider, by owner decision. Scenario costs on Workshop-kind projects block. On provider-backed projects they block only while the run's provider identity matches the baseline's, and are reported as drift otherwise.
4. **Raising a blocking number is a reviewed decision.** It requires a baseline change in the same pull request, approved by a code owner of the baseline. A stated reason alone does not pass the gate.

## Consequences

- Agents and humans see the same small default; detail costs a deliberate call.
- The shape of `wright-result/v1` and `wright-agent/v1` results changes. Consumers that read full results from CLI JSON (the CI Workshop dogfood, the metrics harness, benchmark adapters) move to the narrower queries or the explicit flags.
- The agent guide changes in its owning repository, `wrightkit/skills`, and is re-vendored.
- The metrics brief budget is replaced by a budget on the default forms, and scenario cost is added (#671).
- Code-owner review of baselines depends on the repository governance change in #668.

## Compatibility impact

This is a pre-1.0 shape change of `wright-result/v1` and `wright-agent/v1`, recorded here and released as a breaking change. The contract identifiers are not bumped: the contracts are not frozen until 1.0 (#134), a pre-freeze shape change already exists (#431), and ADR-0020 records no consumer outside this repository. No source-language syntax, Workshop semantics, compiler output, diagnostic, or exit-code behavior changes; `compile` emits the same program to a file instead of stdout.

## Scope boundaries

- Field selection in the style of `gh --json <fields>` is not adopted; explicit flags suffice until a consumer needs more.
- Static catalogs (`capabilities`, `targetMetadata`, `lintRules`) are outside the cap rule; their size does not grow with the project.
- The numeric cap, the default-form budget, and the scenario-cost bands are set by the implementing issues and may be tuned from measured data without a new ADR.
- Pinning external CI inputs was considered and declined (#667).
