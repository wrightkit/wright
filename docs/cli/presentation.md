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
message render as one entry listing all locations; when `--max` withholds
findings, text output states the withheld count and the JSON envelope carries
`selection.total`/`selection.withheld` beside the filtered array.

JSON output is one `wright-result/v1` envelope on stdout with no ANSI, progress,
or workflow commands. `compile` and `convert` source artifacts remain the only
stdout payload in text mode, including when GitHub Actions presentation is
selected. GitHub Actions diagnostics and findings are emitted as escaped
workflow annotations; grouping is sent to the workflow command stream and a
concise PASS/WARN/ERROR line is appended to `GITHUB_STEP_SUMMARY` when the
runner provides that file. The summary uses the highest structured severity:
errors produce `ERROR`, warnings produce `WARN`, and info/notice-only results
produce `PASS`.

Interactive terminal mode is TUI-lite by design. For text workflows selected
as `terminal`, Wright prints immediate activity feedback and then renders
truthful session phases such as input resolution, parsing, semantic analysis,
linting, emission, or conversion. A lightweight spinner starts only after a
short anti-flicker threshold; phase output is transient and is fully cleared
before the final verdict, diagnostics, report, or source artifact is rendered.
Completed `check`, `lint`, `analyze`, `inspect`, and the `inspect` query
subcommands (`symbols`, `refs`, `cfg`, `callgraph`, `cost`) print a
command-specific PASS/WARN/ERROR verdict and compact summary before details;
diagnostics and findings include a one-line source context when the reported
provenance path is readable. The driver exposes typed progress events through
`ProgressObserver`; no terminal strings, spinner frames, ANSI sequence, or
source context enters the driver envelope or JSON. Plain output,
redirected/piped output, `TERM=dumb`, CI, GitHub Actions, and explicit JSON
rendering remain static and deterministic.

The `check` report follows a human-first hierarchy (#443) on the
terminal/plain text surface only: the verdict line leads with the blocking
error count before the non-blocking severities, diagnostics render in action
order (errors before warnings and info notes, stable within a severity), and a
mapped diagnostic carries its `-->` location plus a one-line source frame on
the diagnostic stream. Spans whose path is not a readable source file —
`<stdin>`, `<provider-artifact>`, or a dangling `<file N>` — are never dressed
up as file locations; the reported position and the pipeline stage trail each
diagnostic as secondary metadata instead. A diagnostic whose code carries an
`unknown-` kind (a rejected Workshop or OverPy name) is followed by a
one-line `hint: run 'wright lookup' ...` pointer on `check` and `compile`
(#529, ADR-0021); the hint is text presentation only and never enters the
envelope. A compact footer closes the report
with the count of affected source files and, on an interactive terminal, the
elapsed workflow time; plain output omits wall-clock values so it stays
deterministic. Severity ordering, secondary metadata, the lookup hint, and
the footer are presentation-layer concerns: the `wright-result/v1` envelope
keeps the driver's diagnostic set and production order, and the GitHub
Actions renderer is unchanged.

The `lint` report applies the same hierarchy to findings: the verdict
leads with finding counts by severity, findings render in action order, and a
finding entry leads with severity, rule id, and message before its `-->`
location and one-line source frame. Evidence class and boundedness trail
dimmed as secondary metadata; pseudo-path spans degrade to position notes.
Repeated findings keep the shared collapse — one entry naming its finding
count, capped at ten listed locations — and the closing footer carries
affected-file and skipped-evaluation counts plus interactive elapsed time.
The `wright-result/v1` envelope keeps the driver's finding set and order,
and the GitHub Actions renderer is unchanged.

The `inspect` queries follow the same human-first hierarchy (#446) on the
terminal/plain text surface. Bare `wright inspect` prints the program
inventory — file, rule, variable, subroutine, and finding counts — followed
by a bounded rule-name preview and the detail commands, so the first screen
answers "what is this" and routes deeper questions to the owning query. Each
subcommand leads with its subject and summary before its detail:

* `inspect symbols` groups entries under their kind and prints each symbol's
  name beside its primary `path:line:col` location.
* `inspect refs` prints the queried symbol's identity and usage counts in the
  verdict metadata, then the reference list where each entry carries its
  location, kind, and rule/action index.
* `inspect cfg` prints the queried rule's name, the graph shape (block, edge,
  loop-header, wait, and call-site counts; entry and exit blocks), then the
  block/edge listing.
* `inspect callgraph` prints the edge shape first, then notable fan-in and
  fan-out — subroutines with more than one caller and rules calling more
  than one subroutine — then the edge list.
* `inspect cost` prints exact generated-resource totals, then static findings
  ordered by severity with consecutive identical findings collapsed into one
  counted entry.

Detail lists bound at ten entries per section and close with
`... N more <item>(s) (--format json prints the complete result)`; JSON
remains the complete machine-readable answer and is never truncated by the
presentation bound. When a query fails — an unmatched or ambiguous name —
the verdict and diagnostics render without result metadata, so a default
result can never masquerade as an answer.

This document is the normative contract for the compiler driver and CLI.
It defines the shared driver model, the command surface, exit codes,
stdout/stderr ownership, and the `wright-result/v1` envelope that CI and
agents consume.
