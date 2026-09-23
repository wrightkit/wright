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
Completed `check`, `lint`, `analyze`, and `inspect` commands print a
command-specific PASS/WARN/ERROR verdict and compact summary before details;
diagnostics and findings include a one-line source context when the reported
provenance path is readable. The driver exposes typed progress events through
`ProgressObserver`; no terminal strings, spinner frames, ANSI sequence, or
source context enters the driver envelope or JSON. Plain output,
redirected/piped output, `TERM=dumb`, CI, GitHub Actions, and explicit JSON
rendering remain static and deterministic.

This document is the normative contract for the compiler driver and CLI.
It defines the shared driver model, the command surface, exit codes,
stdout/stderr ownership, and the `wright-result/v1` envelope that CI and
agents consume.
