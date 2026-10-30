# CLI lint contract

[← CLI contract index](../cli.md)

## `wright lint` and the lint configuration

`wright lint` runs through the same compiler/session pipeline as the other
commands and reports structured findings with stable rule IDs, configured
severity, an evidence class, and original source identity/spans where
available. It reuses the lint registry, so rule enable/disable/severity
configuration is deterministic and identical across CLI and programmatic
(`CompilerSession::lint`, tool/agent `lint`) use.

The lint configuration file may also pin lint rule packages under a
`packages` entry (#843). Each entry names a `repo` (`owner/repo` or a
github.com URL) and a full commit `sha`; the configuration is the lock.
Rules from listed packages load through the same lint path under the
project's severity and enablement policy — package rule files never set
`severity` or `enabled`. A package missing from the local store is fetched
by its SHA on activation, so a pinned project reproduces in CI without a
separate install step; an unfetchable SHA fails with an explicit
diagnostic and no lint findings. Packages live in a Wright-owned store
(`WRIGHT_LINT_PACKAGE_DIR`, defaulting to the platform data directory);
`GITHUB_TOKEN` is optional and raises GitHub API limits.

The following lint-policy flags configure the registry and are repeatable:

* `--lint-config <PATH>`: read project lint configuration YAML.
* `--rule-file <PATH>`: load a local YAML rule file or directory.
* `--disable-rule <ID>`: disable a rule by stable ID (`min-wait-loop`,
  `duplicate-condition`, `expensive-loop-check`, `repeated-value`,
  `ongoing-condition-hot-path`, `while-without-wait`,
  `nested-binder-lambda`).
* `--rule-severity <ID>:<off|warn|error>`: override a rule's project policy.

`compile` and `check` accept these four flags too; their enabled lint findings
appear in normal diagnostics. These flags are usage errors on the other
workflow commands (exit 2).
`wright serve` accepts the same four flags at startup (#594): they configure
the served session's lint policy identically, so `lint`/`lintRules` over
stdio, JSON-RPC, or MCP report the same effective enablement, severity,
options, and findings as the equivalent `wright lint` invocation.

`lint`, `check`, and `analyze` also share the finding-selection
options (#430). They narrow the *reported* findings/diagnostics through the
shared `wright-driver` selection, the same model the agent `findings`,
`lint`, and `costEstimate` operations expose, and never change the verdict
or the exit code, which always reflect the complete set:

* `--severity <error|warning|info>`: report findings at or above a severity
  threshold (`error` reports errors only, `info` reports everything).
* `--rule-id <ID>`: report findings produced by one rule id. An unknown id
  is a usage error (exit 2), never a silent empty result.
* `--file <PATH>`: report findings located in one source file. The reported
  `span.path` spelling differs per surface (root-relative findings,
  cwd-relative diagnostics), so any spelling that resolves to the same file,
  whether as passed, root-relative, or absolute, selects it.
* `--max <N>`: report at most N findings.

When `max` drops findings, the result reports how many were withheld: the
JSON envelope adds a `selection` member `{"total": <findings before selection>,
"withheld": <dropped by --max>}` beside the filtered `findings`/`diagnostics`
arrays, and text output prints a `... N finding(s) withheld` line. The verdict
metadata keeps reporting the true total (for example `10 finding(s) across
6 rule(s)` — the declared program rule inventory `analyze`/`inspect`
report, not the lint-rule registry size), so a selected result is never
presented as complete.

On the terminal/plain text surface the lint report follows the human-first
hierarchy: the verdict leads with finding counts by severity (for
example `1 error(s), 9 warning(s) across 6 rule(s)`), findings render in
action order, errors before warnings before informational findings and stable
within a severity, and each entry leads with severity, rule id, and message,
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

`nested-binder-lambda` is a static indicator about Workshop element-binding
scopes. The binder values (`filteredArray`, `sortedArray`, `mappedArray`,
`isTrueForAll`, `isTrueForAny`) evaluate every argument after the first per
array element, binding `Current Array Element`/`Current Array Index`; a
binder nested inside that per-element argument shadows the outer element
inside the inner binder's own argument. The shape is semantically valid and
may be intentional, but it is the structural signature measured on
client-rejected Workshop builds and absent from measured accepted ones
(`workshop-rs` `docs/import-limits.md`; #713). It is a warning, not a
confirmed client import boundary.

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
registered rule's stable `id` and `effectiveSeverity`, enough to interpret a
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
      { "id": "while-without-wait", "effectiveSeverity": "warning" },
      { "id": "nested-binder-lambda", "effectiveSeverity": "warning" }
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

* `check` reports discovery, source implementation, project, semantic, lowering,
  and validation diagnostics together with the enabled lint findings.
  `compile` also reports enabled lint findings before writing its artifact.
  Both preserve the lint codes, severities, source spans, and evidence classes
  in their diagnostics and accept the same policy flags; see
  [command behavior](commands.md#wright-check-and-wright-compile-lint-diagnostics).
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
  (`[static]`), and heuristic rankings or indicators (`heuristic`). Nothing
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

`while-without-wait` findings also carry a machine-readable
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

## Error severity (#1010)

Any `error`-severity finding yields `ok: false` and `exit: 1` on every
verdict surface — `wright lint` and the `lint` agent operation share the
driver's lint pipeline, so both report the identical verdict for one
input and lint configuration (#1002). Lint correctness is not
guaranteed — rules can produce
false positives and the analyzer itself can have implementation errors —
so `error` is reserved for findings whose defect is confirmed, not
indicated: only a rule whose evidence class is `runtime-validated` may
emit `error`.

### Audit: rules that can emit `error` severity

| Rule | Kind | Evidence class | Verdict |
| --- | --- | --- | --- |
| *(none)* | — | — | — |

All seven built-in native rules ship `warning` or `info` defaults:
`min-wait-loop` (`static-indicator`), `duplicate-condition` (`exact`),
`repeated-value` (`exact`), `while-without-wait` (`static-indicator`),
`nested-binder-lambda` (`static-indicator`) → `warning`;
`expensive-loop-check` (`heuristic`), `ongoing-condition-hot-path`
(`heuristic`) → `info`. None is `runtime-validated`, so none justifies
`error` and none needed a downgrade. Real-project runs over
`tests/fixtures/workshop/real-world/*.ws` (overpy-cake, overpy-pixelart,
overpy-client-to-server, reproducer-bastion-prophet) and the metrics
corpus emit only `warning`/`info` findings.

The boundaries that keep this true are enforced, not advisory:

* External (`--rule-file`, community package) declarative rules cannot
  declare `severity` at all — the field is rejected at load time as a
  `lint-rule-error`, so they can never emit `error` by default.
* A first-party (`wrightkit/lint-rules`) declarative rule may declare
  `severity: error` only together with
  `evidence: runtime-validated`; any other combination is a load-time
  `lint-rule-error`, never a silent downgrade. The first-party package
  currently ships no rules.
* `severity: error` in a project lint config or
  `--rule-severity <ID>:error` is explicit project policy, not a rule
  default, and the verdict contract intentionally honors it.

The `workshop.unknown-action`, `workshop.unknown-value`, and
`workshop.opaque-action` completeness residuals are `error`-severity
envelope *diagnostics* owned by `workshop-rs` catalog resolution — exact
facts that appear in `diagnostics`, never in lint `findings`, and outside
this audit's rule scope.

## Validated automated fixes (#556)

An `exact`-evidence finding whose correction is mechanically unambiguous
carries a `fix` member on `lint` findings and on the agent `findings`/`lint`
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

`fix.transaction` is the same `EditTransaction` `validateEditTransaction`
validates:
the agent path previews it through `validateEditTransaction` and applies it
through the caller's own write path. There is no separate fix operation.
On the CLI, `wright lint --fix` renders each offered fix's validated diff,
and `wright lint --fix --write` applies fixes one at a time, reloading and
re-linting after each write so every fix is planned and validated against
the source version it edits. `--write` requires `--fix` and a path-based
input. When `--fix` runs, the result
adds a `fixes` list with each offered fix's `status` (`preview`, `applied`,
or `refused`), its `preview`, and any refusal `diagnostics`.

The current fixable rules and their corrections:

* `duplicate-condition` (`remove-dead-branch`): delete the unreachable
  `Else If` branch: the marker and its body, through the next `Else If`,
  `Else`, or `End` marker. The fix is withheld when the repeated condition
  can evaluate differently inside one synchronous pass, as with calls
  evaluated fresh per call rather than read from the tick snapshot (random
  numbers, advancing clocks, sampled server-load metrics): the branch may be
  reachable after all.
* `repeated-value` (`evaluate-once`): mark each duplicated occurrence with
  `Evaluate Once`. The engine re-evaluates some occurrences: the loop's own
  `While` condition, `Wait Until` and `Loop If` conditions, `Update Every
  Frame` subtrees, and parameters a persistent-object action's
  reevaluation mode keeps live. Those are never wrapped; the mode's coverage is
  catalog-pinned per parameter, so an argument the selected member does
  not re-evaluate still wraps. When more than one occurrence must stay
  live the family carries no fix, because a partial wrap would leave the
  finding standing. `Evaluate Once` is the ecosystem idiom for an
  acknowledged repeated evaluation: at positions the engine already
  evaluates once per action it changes nothing. The fix retires the
  finding by marking the duplication as intentional; it does not reduce
  the evaluation count.

Every offered fix edits only the reported spans, carries the input identity
as a precondition, and is validated before any write. A raw Workshop fix
reparses the edited source, while a provider-mapped fix goes through the
provider's own edit validation and project check. A stale or malformed
precondition refuses with `edit-stale-source` (or the real diagnostic) and
writes nothing. Provider-backed findings carry a `fix` when their source
map records authored positions. A mapped `duplicate-condition` edits the
authored `elif`/`else` chain directly; unmapped provider artifacts have
spans in generated text and carry no `fix`.

## Lint rule packages: `wright lint package` (#844)

`wright lint package` discovers, installs, and manages community lint rule
packages hosted on GitHub. One repository carrying the `wrightkit-lint`
topic is one package: the `rules/*.yaml|yml` files directly inside its
`rules/` directory at one commit, each loading through the same SPEC-309
declarative rule contract as a `--rule-file` file. Nested rule files, package
code, and anything else in the archive are never extracted or executed.

The Wright-owned store keeps one immutable directory per
`<owner>/<repo>/<full-sha>/` under Wright's application-data directory;
`WRIGHT_LINT_PACKAGE_DIR` relocates it. An intact store entry is never
downloaded or overwritten again — only the full commit SHA is stored, never
the ref it resolved from — and a failed fetch or validation leaves no
partial entry. `GITHUB_TOKEN`, when set, raises the GitHub API limit and is
sent to the API host only, never to the archive host.

Installing a package never activates it: project configuration pins a
package by commit SHA and opts it into the registry (#843), so a lint run
is reproducible without re-resolving refs.

```text
wright lint package search [QUERY]         repositories carrying the wrightkit-lint topic
wright lint package install SOURCE [--ref REF]
wright lint package list                   every store entry, intact and damaged
wright lint package outdated               check each entry against the remote head
wright lint package update [SOURCE]        install the current remote head
wright lint package remove SOURCE          delete the entry's managed rules
```

A `SOURCE` is `owner/repo`, a github.com URL (`https://github.com/owner/repo`,
`git@github.com:owner/repo`, including `/tree/<ref>` and
`/releases/tag/<ref>` spellings), or `owner/repo@<ref>`; `--ref` selects a
branch, tag, or commit explicitly. Ref spellings that disagree are a usage
error. A full 40-hex SHA needs no resolution and installs offline.

* `search` runs the `topic:wrightkit-lint archived:false fork:false`
  repository query paged at `per_page=100`; `QUERY` narrows it further. The
  result reports GitHub's `total` and `incomplete` flags beside the found
  rows.
* `install` resolves the ref to a full SHA — the repository's
  default-branch head when no ref is given — fetches the archive for that
  SHA, validates it, and stores it. Reinstalling an intact entry makes no
  download (`downloaded=false`).
* `list` reports every store entry in `owner/repo/sha` order. An intact
  entry reports `installed` with its rule ids; a damaged entry reports
  `damaged` with the `lint-package-install` reason it does not validate —
  local edits to managed content are damage, never silently replaced.
* `outdated` checks each entry's installed SHA against the remote
  default-branch head and reports `unchanged`, `updated`, `unavailable`
  (the repository or ref is gone), or `error` — read-only; nothing applies
  to the store and a damaged entry reports `error` without being checked.
* `update` installs the current remote head for every installed entry, or
  for the one repository `SOURCE` selects (`owner/repo@<ref>` updates only
  the entry the ref resolves to). An `unchanged` entry is not re-fetched;
  an `updated` row keeps the installed `sha` beside the new `remote` head,
  and the previous entry stays in the store. `unavailable`/`error` rows
  fail the command.
* `remove` deletes each selected entry's managed `rules/` tree
  (`owner/repo` removes every installed entry of the repository,
  `owner/repo@<ref>` removes the one the ref resolves to). The entry
  directory is deleted only when nothing else remains: unrelated files
  beside `rules/` keep it, reported `pruned` instead of `removed`, and
  empty `<owner>/<repo>` parents are pruned. A source with no matching
  entry is a `lint-package-not-found` error.

Every subcommand renders the shared `wright-result/v1` envelope —
`package-search`, `package-install`, `package-list`, `package-outdated`,
`package-update`, `package-remove` — through the same presentation options
(`--format`, `--renderer`, `--color`, `--all`) as the workflow commands.
`search`/`list`/`outdated` are read-only queries and lead with their own
command-name tag; `install`/`update`/`remove` are lifecycle operations
whose per-package failures produce a gated FAIL with one
`lint-package-*` diagnostic per failed package. Package diagnostics use
the `lint-package-*` codes: `lint-package-source` (a bad source/ref
spelling, exit 2), `lint-package-invalid` (content the SPEC-309 loader
refuses, exit 1), `lint-package-offline`, `lint-package-not-found`,
`lint-package-rate-limited`, `lint-package-remote`, and
`lint-package-install` (store damage or I/O, exit 4). Lint-policy flags,
finding-selection flags, and `INPUT` belong to `wright lint` and are
usage errors on `lint package`.
