# CLI presentation and completion

[← CLI contract index](../cli.md)

## CLI presentation and completion (#164, #186)

The `wright` command model is defined once with `clap`. The same model drives
argv parsing, generated help, and shell completion:

```text
# Pure completion generation (stdout)
wright completion bash
wright completion zsh
wright completion fish
wright completion powershell

# Automatic or explicit user-local completion installation
wright completion install
wright completion install zsh
wright completion install --dir <DIR>
wright completion install --dry-run
wright completion install --all
```

### Shell completion lifecycle (#186)

Wright manages its completion lifecycle directly so installations and updates
provide working completions without duplicating shell filesystem logic or
mutating shell startup configuration:

* `wright completion <shell>`: generates pure static completion scripts for
  Bash, Zsh, Fish, or PowerShell (`pwsh`) directly to stdout for manual setup or
  package-manager packaging.
* `wright completion install [SHELL]`: detects the current shell (or takes an
  explicit shell/`--shell` override) and installs generated completion into the
  standard conventional user-local directories without modifying `.zshrc`,
  `.bashrc`, or user startup scripts.
  - `--dir <DIR>` overrides the target directory.
  - `--dry-run` prints planned installation paths without writing files.
  - `--all` installs/refreshes completions for all supported shells.
  - `--force` forces re-writing existing completion files.
* **Shell detection**: resolves `$WRIGHT_SHELL` first, then inspects `$SHELL`
  path/filename, followed by shell environment variables (`ZSH_VERSION`,
  `BASH_VERSION`, `FISH_VERSION`, `PSModulePath`), falling back to PowerShell on
  Windows.
* **Conventional installation locations**:
  - **Fish**: `~/.config/fish/completions/wright.fish` (or
    `~/.local/share/fish/vendor_completions.d/wright.fish`), automatically loaded
    by Fish.
  - **Bash**: `~/.local/share/bash-completion/completions/wright` (or
    `~/.bash_completion.d/wright`), loaded automatically when `bash-completion`
    is active.
  - **Zsh**: `$ZSH_CUSTOM/completions/_wright` (if `$ZSH_CUSTOM` set),
    `~/.oh-my-zsh/custom/completions/_wright` (if Oh My Zsh exists),
    `~/.zfunc/_wright`, `~/.zsh/completions/_wright`, or
    `~/.local/share/zsh/site-functions/_wright`.
  - **PowerShell**: `~/Documents/PowerShell/Scripts/_wright.ps1` (Windows) or
    `~/.config/powershell/completions/_wright.ps1` (Unix).
* **Install and Update integration**:
  - `install.sh` invokes `wright completion install` post-installation and
    reports non-fatal guidance if automatic completion installation cannot run.
  - `wright update` refreshes existing Wright completion files in conventional
    locations upon replacing the binaries so completions stay aligned with the
    installed command model.
* **Package-manager boundaries**: Homebrew, Scoop, and WinGet packaging
  consume `wright completion <shell>` generation using native package conventions
  rather than maintaining hand-written completion scripts.

Workflow commands accept these CLI-only presentation options:

* `--format text|json` selects human or machine output (`-f` remains an alias).
* `--renderer auto|terminal|plain|github-actions` selects the presentation
  environment. `auto` selects GitHub Actions when `GITHUB_ACTIONS` is truthy,
  plain output for generic `CI` or a non-TTY, and terminal output otherwise.
* `--color auto|always|never` controls ANSI color. Explicit options take
  precedence over environment detection; GitHub Actions keeps workflow
  command lines free of ANSI even when color is explicitly requested.

`check`, `analyze`, `lint`, and `inspect cost` also accept the
finding-selection options `--severity`, `--rule-id`, `--file`, and `--max`
(#430), applied by the driver to reported diagnostics and lint findings.
Selection is a rendering concern only: it runs after the verdict and exit
code are fixed on the
complete set, so it can never turn a failing command into `exit 0` or a
`WARN` verdict into `PASS`. Consecutive lint findings sharing a rule id and
message render as one entry listing all locations on the terminal renderer
(the plain renderer keeps one record per finding); when `--max` withholds
findings, text output states the withheld count and the JSON envelope carries
`selection.total`/`selection.withheld` beside the filtered array, plus
`selection.byCode` counting the filtered set per code before `max` truncates
it (#791).

### The plain renderer: line records (#793, ADR-0030)

The plain renderer — the default on a non-TTY or generic CI — renders every
non-artifact report as line records: one fact per line, deterministic, and
free of decoration, pluralized units, and wall-clock values. A record is

```text
<kind>: <key>=<value> [<key>=<value> …]
```

except the leading verdict line, which is `<STATUS> <command>
[key=value …]`. The status shares the renderers' tag set: `PASS`, `WARN`,
`FAIL` (a raised `--fail-on` verdict), `ERROR` (a genuine failure), `USAGE`
and `UNSUPPORTED` for exit 2/3 envelopes; a clean query command (`inspect`,
`rules`, `symbols`, `refs`, `cfg`, `callgraph`, `cost`, `lookup`) shows its
uppercase name in place of a vacuous `PASS`. Verdict fields carry the input
`root=` plus program `kind=`/`locale=` when the result reports an origin.

Records lead with the verdict, then `summary:` — the headline counts named
after their JSON fields (`errors=`, `warnings=`, `notes=`/`findings=`,
`affected=`, `emittedBytes=`, `rules=`, `symbols=`, `reads=`, `writes=`,
`calls=`, `blocks=`, `edges=`, …) — then the command's section records:
`program:`, `rule:`, `symbol:`, `ref:`, `block:`, `edge:`, `fan-in:`,
`fan-out:`, `share:`, `statement:`, `byNode:`, `hotspot:`, `tree:`,
`complexity:`, `variable:`, `objects:`, `cost:`, `output:` (written-path
plus `sha256=`), `fix:`/`patch:`/`refusal:` for `lint --fix`, and `entry:`
records for `lookup`. `diagnostic:` records render envelope diagnostics on
stderr; result records stay on stdout, and artifacts keep byte-exact
stdout ownership. A `diagnostic:` carries `<severity> <code>` then
`at=<path:line:col>`, `stage=`, `status=`, `source=`, `locale=`, and
`message=`; a `finding:` carries the same shape plus `count=`/`spanKind=`/
`expression=` when the `repeated` member is present and `evidence=` when
reported. Locations are single `at=path:line:col` tokens — never a `-->`
line or source frame.

Key names share the JSON fields they mirror, and values stay bare unless
they contain whitespace, `=`, `"`, or `\`, in which case they are
double-quoted with `\"`, `\\`, `\n`, and `\t` escapes. Unavailable
computations report `unavailable` plus the payload's own reason member
(`elementCountUnavailable`, `emittedBytesUnavailable`,
`unavailableReason`) — never a fabricated zero (ADR-0030 decision 3).
Truncation is explicit: `truncated: <list>=<shown>/<total>
[byCode=<code>:<n>,…] expand="<command>"`, and a selection closes with
`selected: <noun>=<shown>/<total> [byCode=…] [withheld=<n> expand=…]`.
Run-cache and baseline comparisons render `comparison:`/`change:` records,
and `next:` records name follow-up commands — the shared list the result
carries (#792) first, then the `wright lookup` pointer a rejected-name
diagnostic implies. Non-envelope command errors use `error:`/`unavailable:` records
on stderr: `update` refusals are the record alone, while `lookup` request
errors also print the `<STATUS> lookup` verdict line on stdout.

JSON output is one `wright-result/v1` envelope on stdout with no ANSI, progress,
or workflow commands. The `convert` source artifact and the `compile -o -`
program remain the only stdout payloads in text mode, including when GitHub
Actions presentation is selected; every other `compile` report is a verdict,
the written path, and the output SHA-256. GitHub Actions diagnostics and findings are emitted as escaped
workflow annotations; grouping is sent to the workflow command stream and a
concise PASS/WARN/FAIL/ERROR line is appended to `GITHUB_STEP_SUMMARY` when the
runner provides that file. The summary uses the highest structured severity:
errors produce `ERROR`, warnings produce `WARN`, and info/notice-only results
produce `PASS`; a result raised by `--fail-on` produces `FAIL` (exit 1).

Interactive terminal mode is TUI-lite by design. For text workflows selected
as `terminal`, Wright prints immediate activity feedback and then renders
truthful session phases such as input resolution, parsing, semantic analysis,
linting, emission, or conversion. A lightweight spinner starts only after a
short anti-flicker threshold; phase output is transient and is fully cleared
before the final verdict, diagnostics, report, or source artifact is rendered.
Completed `check`, `lint`, `analyze`, `inspect`, and the `inspect` query
subcommands (`symbols`, `refs`, `cfg`, `callgraph`, `cost`) print a
command-specific PASS/WARN/FAIL/ERROR verdict and compact summary before details;
diagnostics and findings include a one-line source context when the reported
provenance path is readable. The driver exposes typed progress events through
`ProgressObserver`; no terminal strings, spinner frames, ANSI sequence, or
source context enters the driver envelope or JSON. Plain output,
redirected/piped output, `TERM=dumb`, CI, GitHub Actions, and explicit JSON
rendering remain static and deterministic.

On the terminal renderer only, a report with independent multi-line items
is drawn on one continuous rail (#703, #718). The verdict line opens it with
`╭─`, every section heading, count, and child line continues it with `│`,
each item (envelope diagnostic, lint finding or fix entry, analyze risk
item, `inspect cfg` block, `inspect rules --content` entry) branches off with
`├─`, and the footer closes it with `╰─`, even when a trailing detail hint follows
it in the flat layout; a report without a footer ends on a bare `╰─`. A blank rail line separates independent blocks, except
between a one-line section heading and its first item. Content sits on one
column, so locations, source frames, and secondary notes align under the item
text. Rail glyphs draw dim when color is on, so severity labels and messages
keep prominence. A report with no multi-line item, such as a clean `check`
(two undecorated lines), stays flat. The rail is a terminal detail: lines keep
the stdout/stderr stream they use elsewhere, `plain`, GitHub Actions, CI, and
JSON keep their existing undecorated layout byte-for-byte, and the reported
findings, verdicts, exit codes, caps, and `--all` expansion are identical
across renderers.

The `check` report follows a human-first hierarchy (#443): the verdict line
leads with the blocking error count before the non-blocking severities, and
diagnostics render in action order (errors before warnings and info notes,
stable within a severity). On the terminal renderer a mapped diagnostic
carries its `-->` location plus a one-line source frame on the diagnostic
stream; on the plain renderer the same location is the record's `at=` field.
Spans whose path is not a readable source file, such as
`<stdin>`, `<provider-artifact>`, or a dangling `<file N>`, are never dressed
up as file locations; the reported position and the pipeline stage trail each
diagnostic as secondary metadata instead. A diagnostic whose code carries an
`unknown-` kind (a rejected Workshop or OverPy name) is followed by a
one-line `hint: run 'wright lookup' ...` pointer on `check` and `compile`
(#529, ADR-0021); an OPY-origin diagnostic names its vocabulary:
`wright lookup --language opy` (#692). The plain renderer carries the same
follow-up as a `next:` record. The hint is text presentation only
and never enters the envelope. A compact footer closes the terminal report
with the count of affected source files and, on an interactive terminal, the
elapsed workflow time; the plain renderer folds the same affected count into
its `summary:` record and omits wall-clock values so it stays
deterministic. Severity ordering, secondary metadata, the lookup hint, and
the footer are presentation-layer concerns: the `wright-result/v1` envelope
keeps the driver's diagnostic set and production order, and the GitHub
Actions renderer is unchanged.

The `lint` report applies the same hierarchy to findings: the verdict
leads with finding counts by severity, findings render in action order, and a
finding entry leads with severity, rule id, and message before its location
(the terminal renderer's `-->` line and one-line source frame; the plain
record's `at=` field). Evidence class and boundedness trail
dimmed as secondary metadata on the terminal renderer and as `evidence=`
fields on the plain renderer; pseudo-path spans degrade to position notes.
On the terminal renderer repeated findings keep the shared collapse, one
entry naming its finding count and using the selected locations — the plain
renderer emits one `finding:` record per finding — and interactive elapsed
time stays presentation-only.
The `wright-result/v1` envelope uses the same selected finding set,
and the GitHub Actions renderer is unchanged.

The `inspect` queries follow the same human-first hierarchy (#446): on the
terminal renderer as stacked sections, on the plain renderer as the same
facts in record order. Bare `wright inspect` prints the program
inventory of file, rule, variable, subroutine, and finding counts, followed
by a bounded rule-name preview and the detail commands, so the first screen
answers "what is this" and routes deeper questions to the owning query — on
the plain renderer the detail commands are `next:` records. Each
subcommand leads with its subject and summary before its detail:

* `inspect symbols` groups entries under their kind and prints each symbol's
  name beside its primary `path:line:col` location (`symbol:` records on the
  plain renderer).
* `inspect refs` prints the queried symbol's identity and usage counts in the
  verdict metadata (`summary:` fields on the plain renderer), then the
  reference list where each entry carries its location, kind, and rule/action
  index (`ref:` records).
* `inspect cfg` prints the queried rule's name, the graph shape (block, edge,
  loop-header, wait, and call-site counts; entry and exit blocks), then the
  block/edge listing (`block:`/`edge:` records).
* `inspect callgraph` prints the edge shape first, then notable fan-in and
  fan-out, that is, subroutines with more than one caller and rules calling
  more than one subroutine, then the edge list (`fan-in:`/`fan-out:`/`edge:`
  records).
* `inspect cost` prints exact generated-resource totals, the canonical
  element count against the client element budget with each rule's share
  ranked by count and located in source (`path:line:col`; `share:` records),
  then static
  findings
  ordered by severity with consecutive identical findings collapsed into one
  counted entry that lists every occurrence's location on the terminal
  renderer (#722). A failed
  computation is never a zero: a failed element count reports `unavailable`
  with its reason and the known 32768-element budget, and failed emission
  reports `emitted bytes: unavailable — <reason>` (`elements=unavailable`/
  `emittedBytes=unavailable` plus the `elementCountUnavailable`/
  `emittedBytesUnavailable` reason fields on the plain renderer)
  (ADR-0030 decision 3). Skipped rule
  evaluations are disclosed like `lint`'s (#788 — `skipped=` on the plain
  `summary:`). With
  `--rule`, `inspect cost` instead prints that rule's identity and
  location, its `Costliest statements` ranked by recursive count, and the
  `By node name` own-cost shares that reconcile to the rule's total (#724)
  (`statement:`/`byNode:` records).

Text and compact JSON present the same selected items. Detail lists default
to at most 20 items, with their total and an expansion hint when truncated —
a `truncated:` record naming the expansion command on the plain renderer;
`--all` lifts the default cap. The analyze report keeps its ranked top-five
hotspots, risks and variables and top-three condition trees, with the same
explicit expansion metadata. JSON is the structured form for programmatic
processing. Verdicts still describe the full set. When a query fails, its
verdict and diagnostics render without result metadata, so a default result
cannot masquerade as an answer.

This document is the normative contract for the compiler driver and CLI.
It defines the shared driver model, the command surface, exit codes,
stdout/stderr ownership, and the `wright-result/v1` envelope that CI and
agents consume.
