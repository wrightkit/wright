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

`lint`, `check`, and `analyze` additionally share the finding-selection
options (#430). They narrow the *reported* findings/diagnostics through the
shared `wright-driver` selection — the same model the agent `findings`,
`lint`, and `costEstimate` operations expose — and never change the verdict
or the exit code, which always reflect the complete set:

* `--severity <error|warning|info>`: report findings at or above a severity
  threshold (`error` reports errors only, `info` reports everything).
* `--rule-id <ID>`: report findings produced by one rule id. An unknown id
  is a usage error (exit 2), never a silent empty result.
* `--file <PATH>`: report findings located in one source file. The reported
  `span.path` spelling differs per surface (root-relative findings,
  cwd-relative diagnostics), so any spelling that resolves to the same file —
  as passed, root-relative, or absolute — selects it.
* `--max <N>`: report at most N findings.

When `max` drops findings, the result reports how many were withheld: the
JSON envelope adds `selection` — `{"total": <findings before selection>,
"withheld": <dropped by --max>}` — beside the filtered `findings`/`diagnostics`
arrays, and text output prints a `... N finding(s) withheld` line. The verdict
metadata keeps reporting the true total (for example `10 finding(s) across
6 rule(s)`), so a selected result is never presented as complete.

On the terminal/plain text surface the lint report follows the human-first
hierarchy: the verdict leads with finding counts by severity (for
example `1 error(s), 9 warning(s) across 6 rule(s)`), findings render in
action order — errors before warnings before informational findings, stable
within a severity — and each entry leads with severity, rule id, and message,
then a `--> path:line:col` location, then a one-line source frame when the
reported path resolves to a readable file. Evidence class and boundedness
trail dimmed under the entry as secondary metadata. Pseudo-path spans
(`<stdin>`, `<provider-artifact>`, `<file N>`) render as
`at <path>:line:col (no source file)` notes instead of file-style locations;
a real spelling that resolves to no file keeps its reported `-->` location
without a frame. Reported `span.path` spellings are root-relative; the text
view resolves them under the input root so subdirectory inputs keep an
actionable location. A compact footer closes the
report with the count of affected files and skipped rule evaluations, plus
elapsed time on interactive terminals only.

Consecutive findings sharing a rule id and message collapse into one entry
that names its finding count and lists the locations that resolve to source
files instead of repeating the message and a source-context line per
occurrence; a group lists at most ten locations and folds the rest into a
`... N more location(s)` line, while positions on pseudo-paths fold into the
secondary notes.

`ongoing-condition-hot-path` is a heuristic about the per-tick evaluation of
an `Ongoing - Global` or `Ongoing - Each Player` rule's conditions. Each tick
evaluates conditions in source order until one short-circuits the rule, so a
predicate in a later condition is reached only after every preceding condition
passes. It does not claim that the rule's action block executes every tick
while conditions remain true, measure server cost, or infer the selectivity of
any condition.

The `lint` result envelope carries `input_identity` (the SHA-256 source
identity; the tool/agent API exposes the same value as `inputIdentity`),
`program`, `rules`, `config`, `findings`, and `skipped`. `rules` lists each
registered rule's stable `id` and `effectiveSeverity` — enough to interpret a
finding's `code` and `severity`. Full rule metadata (summary, rationale,
documentation, known limits, evidence class, tags) is served once by the
`lintRules` agent operation rather than inlined into every `lint` result
(#431):

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
      { "id": "min-wait-loop", "effectiveSeverity": "warning" },
      { "id": "duplicate-condition", "effectiveSeverity": "warning" },
      { "id": "expensive-loop-check", "effectiveSeverity": "info" },
      { "id": "ongoing-condition-hot-path", "effectiveSeverity": "info" },
      { "id": "repeated-value", "effectiveSeverity": "warning" },
      { "id": "while-without-wait", "effectiveSeverity": "warning" }
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
    ],
    "skipped": []
  }
}
```

The core workflows have separate contracts:

* `check` is the correctness gate. It reports discovery, source implementation, project,
  semantic, lowering, and validation diagnostics. Ordinary configurable lint
  findings such as `duplicate-condition` and `min-wait-loop` are not emitted
  by default.
* `lint` executes the configurable `LintRegistry` and returns stable rule IDs,
  severity, evidence class, boundedness where applicable, source spans,
  a per-rule id/effective-severity list, and effective configuration.
* `analyze` returns semantic facts rather than lint findings (#445). Human
  text output is a bounded, layered report: Workshop cost (the canonical
  element count plus structural counts), ranked rule hotspots attributed by
  element cost or control-flow size, structural complexity (aggregate CFG
  measurements and the largest condition trees), performance/stability risk
  indicators, persistent-object facts, and ranked cross-cutting variables.
  Labels distinguish exact measurements (`[exact]`), static facts
  (`[static]`), and heuristic rankings or indicators (`heuristic`) — nothing
  in the report claims measured runtime cost or behavior. The `risks` facts
  narrow registry findings to rules that declare a `performance` or
  `stability` tag, keeping each finding's evidence class; correctness
  findings stay out of the risk frame and remain available through `lint`.
  `analyze --format json` retains the complete `result.facts` payload for
  agents and embedding, while `inspect` is the human-facing exhaustive
  structural/semantic view. These facts can inform future lint rules without
  making analysis a view of the registry.

Analysis findings (`lint` and the tool/agent `findings`/`lint` responses)
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

## Validated automated fixes (#556)

An `exact`-evidence finding whose correction is mechanically unambiguous
carries a `fix` member — on `lint` findings and on the agent `findings`/`lint`
operations identically:

```json
{
  "code": "repeated-value",
  "fix": {
    "kind": "evaluate-once",
    "summary": "mark each duplicated occurrence with Evaluate Once, the idiom for an intentional repeated evaluation",
    "transaction": { "edits": [ { "kind": "fix", "source": "...", "source_identity": "...", "range": { "...": "..." }, "new_text": "Evaluate Once(...)" } ] }
  }
}
```

`fix.transaction` is the same `EditTransaction` `semanticRename` produces:
the agent path previews it through `validateEditTransaction` and applies it
through the caller's own write path — there is no separate fix operation.
On the CLI, `wright lint --fix` renders each offered fix's validated diff,
and `wright lint --fix --write` applies fixes one at a time — reloading and
re-linting after each write so every fix is planned and validated against
the source version it edits. `--write` requires `--fix` and a path-based
input, and `--fix` conflicts with `--brief`. When `--fix` runs, the result
adds a `fixes` list with each offered fix's `status` (`preview`, `applied`,
or `refused`), its `preview`, and any refusal `diagnostics`.

The current fixable rules and their corrections:

* `duplicate-condition` (`remove-dead-branch`): delete the unreachable
  `Else If` branch — the marker and its body, through the next `Else If`,
  `Else`, or `End` marker. The fix is withheld when the repeated condition
  can evaluate differently inside one synchronous pass — calls evaluated
  fresh per call rather than read from the tick snapshot (random numbers,
  advancing clocks, sampled server-load metrics): the branch may be
  reachable after all.
* `repeated-value` (`evaluate-once`): mark each duplicated occurrence with
  `Evaluate Once`. Occurrences the engine re-evaluates — the loop's own
  `While` condition, `Wait Until` and `Loop If` conditions, `Update Every
  Frame` subtrees, and persistent-object actions whose reevaluation mode is
  enabled — are never wrapped; when more than one occurrence must stay live
  the family carries no fix, because a partial wrap would leave the finding
  standing. `Evaluate Once` is the ecosystem idiom for an acknowledged
  repeated evaluation: at positions the engine already evaluates once per
  action it changes nothing — the fix retires the finding by marking the
  duplication as intentional, it does not reduce the evaluation count.

Every offered fix edits only the reported spans, carries the input identity
as a precondition, and is validated by reparsing the edited source before
any write; a stale or malformed precondition refuses with
`edit-stale-source` (or the real parse/validation diagnostic) and writes
nothing. Fixes exist only on raw Workshop input — provider-backed findings
have spans in generated text and carry no `fix`.
