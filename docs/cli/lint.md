# CLI lint contract

[← CLI contract index](../cli.md)

## `wright lint` and the lint configuration

`wright lint` runs through the same compiler/session pipeline as the other
commands and reports structured findings with stable rule IDs, configured
severity, an evidence class, and original source identity/spans where
available. It reuses the lint registry, so rule enable/disable/severity
configuration is deterministic and identical across CLI and programmatic
(`CompilerSession::lint`, tool/agent `lint`) use.

The following lint-only flags configure the registry and are repeatable:

* `--disable-rule <ID>`: disable a rule by stable ID (`min-wait-loop`,
  `duplicate-condition`, `expensive-loop-check`, `repeated-value`,
  `ongoing-condition-hot-path`, `while-without-wait`).
* `--rule-severity <ID>:<off|warn|error>`: override a rule's project policy.

These flags are usage errors on every other command (exit 2).

`ongoing-condition-hot-path` is a heuristic about the per-tick evaluation of
an `Ongoing - Global` or `Ongoing - Each Player` rule's conditions. Each tick
evaluates conditions in source order until one short-circuits the rule, so a
predicate in a later condition is reached only after every preceding condition
passes. It does not claim that the rule's action block executes every tick
while conditions remain true, measure server cost, or infer the selectivity of
any condition.

The `lint` result envelope carries `input_identity` (the SHA-256 source
identity; the tool/agent API exposes the same value as `inputIdentity`),
`program`, `rules`, `config`, and `findings`:

```json
{
  "wright": { "version": "0.1.0", "contract": "wright-result/v1" },
  "command": "lint",
  "ok": true,
  "exit": 0,
  "diagnostics": [],
  "result": {
    "input_identity": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
    "program": { "origin": { "kind": "workshop", "locale": "en-us" }, "rules": 2, "findings": 1 },
    "rules": [
      {
        "id": "min-wait-loop",
        "defaultSeverity": "warning",
        "effectiveSeverity": "warning",
        "enabled": true,
        "summary": "loop body waits at the workshop minimum rate",
        "evidence": "static-indicator",
        "tags": ["performance", "stability"],
        "knownLimits": "Wait durations that are not statically known ..."
      },
      {
        "id": "duplicate-condition",
        "defaultSeverity": "warning",
        "effectiveSeverity": "warning",
        "enabled": true,
        "summary": "condition is evaluated more than once within one rule",
        "evidence": "exact",
        "tags": ["correctness"],
        "knownLimits": "Detection is structural (not value-flow) and rule-local ..."
      },
      {
        "id": "expensive-loop-check",
        "defaultSeverity": "info",
        "effectiveSeverity": "info",
        "enabled": true,
        "summary": "geometry predicate evaluated inside a loop body",
        "evidence": "heuristic",
        "tags": ["performance"],
        "knownLimits": "The expensive-call list is a fixed heuristic ..."
      },
      {
        "id": "ongoing-condition-hot-path",
        "defaultSeverity": "info",
        "effectiveSeverity": "info",
        "enabled": true,
        "summary": "geometry predicate evaluated in an ongoing-rule condition",
        "evidence": "heuristic",
        "tags": ["performance", "stability"],
        "knownLimits": "The geometry-predicate list is a fixed heuristic; the analysis does not measure runtime cost or infer selectivity ..."
      },
      {
        "id": "repeated-value",
        "defaultSeverity": "warning",
        "effectiveSeverity": "warning",
        "enabled": true,
        "summary": "identical value expression evaluated more than once in one loop scope",
        "evidence": "exact",
        "tags": ["performance", "stability"],
        "knownLimits": "Detection is rule-local and structural ..."
      },
      {
        "id": "while-without-wait",
        "defaultSeverity": "warning",
        "effectiveSeverity": "warning",
        "enabled": true,
        "summary": "while loop body contains no wait call",
        "evidence": "static-indicator",
        "tags": ["stability"],
        "knownLimits": "Counter-pattern detection is conservative and structural: only literal-bound comparisons (<, <=, >, >=) are recognized. A statically-bounded claim additionally requires every direct child ..."
      }
    ],
    "config": { "rules": { "min-wait-loop": { "enabled": true, "severity": "warning" } } },
    "findings": [
      {
        "code": "min-wait-loop",
        "severity": "warning",
        "evidence": "static-indicator",
        "message": "loop body waits at the workshop minimum rate; ...",
        "span": { "file": 0, "path": "program.txt", "start": { "line": 28, "col": 9 }, "end": { "line": 31, "col": 13 } }
      }
    ]
  }
}
```

The core workflows have separate contracts:

* `check` is the correctness gate. It reports discovery, source implementation, project,
  semantic, lowering, and validation diagnostics. Ordinary configurable lint
  findings such as `duplicate-condition` and `min-wait-loop` are not emitted
  by default.
* `lint` executes the configurable `LintRegistry` and returns stable rule IDs,
  severity, evidence class, boundedness where applicable, source spans, rule
  metadata, and effective configuration.
* `analyze` returns semantic facts rather than lint findings. Human text output
  is a bounded report with a program overview, aggregate CFG measurements,
  ranked rule hotspots, and ranked cross-cutting variables. The displayed
  facts are static; rankings are heuristics based on CFG size or usage
  coupling. `analyze --format json` retains the complete `result.facts`
  payload for agents and embedding, while `inspect` is the human-facing
  exhaustive structural/semantic view. These facts can inform future lint
  rules without making analysis a view of the registry.

Analysis findings (`lint` and the tool/agent `getFindings`/`lint` responses)
carry an `evidence` field classifying how strongly the finding is supported
(`exact`, `static-indicator`, `heuristic`, `runtime-validated`).

Finding spans carry a machine-readable `path` resolved root-relative to the
input include root (`--root`, defaulting to the input's directory): file 0 is
the main input, and additional files in a multi-file program resolve from the
program file registry. The same source location therefore reports the same
`path` across `lint` and the tool/agent `Findings`/`Lint` surfaces regardless
of how the input was spelled (absolute, relative, or cwd-relative); stdin
inputs report `<stdin>`.

`while-without-wait` findings additionally carry a machine-readable
`boundedness` field (`obviously-unbounded` | `statically-bounded` | `unknown`)
classifying the no-yield loop's repetition evidence, and their severity is
derived from that evidence class: `warning` for an obviously unbounded or
unknown loop, `info` for a statically bounded no-yield loop. A statically
bounded no-yield loop is never treated as equivalent to an unbounded one. For
example, the agent-lab repro `loop-waitless.opy` (wrightkit/agent-lab#68) is a
finite 10-iteration counter loop and reports as a statically bounded `info`
finding:

```json
{ "code": "while-without-wait", "severity": "info", "boundedness": "statically-bounded", "message": "loop body contains no wait call; the loop is statically bounded by a counter against a literal bound, ..." }
```

Non-`while-without-wait` findings carry `"boundedness": null`.
