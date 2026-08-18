//! The authoritative structured command model for `wright`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// The top-level command model used by parsing, help, and completion.
#[derive(Debug, Parser)]
#[command(
    name = "wright",
    disable_version_flag = true,
    disable_help_subcommand = true,
    subcommand_precedence_over_arg = true,
    about = "Wright compiler and Workshop tooling CLI",
    long_about = LONG_ABOUT
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Command>,
    /// Print the implementation and driver versions.
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub(crate) version: bool,
}

pub(crate) const LONG_ABOUT: &str = "Wright compiler and Workshop tooling CLI.

Commands parse, validate, analyze, lint, inspect, compile, or reconstruct
source through the typed wright-driver result envelope. `compile` and `convert`
keep their source artifact stdout contracts; JSON mode prints only one
wright-result/v1 envelope to stdout.

EXIT CODES:
    0  success
    1  source/user error
    2  usage error
    3  recognized but unsupported input or operation
    4  internal/environment failure

WORKFLOW OPTIONS:
    --kind <KIND>        Input frontend: auto|opy|ostw|workshop|protocol
    --target <TARGET>    Reconstruction target for convert: opy|ostw
    --locale <LOCALE>    Workshop client locale override
    --root <DIR>         Include/project root for source inputs
    --profile <PROFILE>  WIR transformation policy: off|compat|aggressive
    -o, --output <PATH>  Write compiled output to PATH (compile only)
    -f, --format <FMT>   Output format: text|json
    --renderer <MODE>    Presentation: auto|terminal|plain|github-actions
    --color <POLICY>     ANSI color: auto|always|never

LINT OPTIONS:
    --disable-rule <ID>         Disable a lint rule (repeatable)
    --rule-severity <ID>:<SEV>  Override a lint rule severity (repeatable)

UPDATE OPTIONS:
    --check              Check for an update without modifying the installation
    --version <VERSION>  Install an exact version instead of the latest stable release";

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Parse, lower, validate, and emit Workshop text.
    Compile(CompileArgs),
    /// Reconstruct validated Workshop input as canonical OPY or OSTW source.
    Convert(ConvertArgs),
    /// Parse, validate, and analyze the input.
    Check(CommonArgs),
    /// Parse, lower, and report semantic findings.
    Analyze(CommonArgs),
    /// Parse, lower, and report lint findings.
    Lint(LintArgs),
    /// Parse, lower, and show the structural/semantic program model.
    Inspect(CommonArgs),
    /// Generate static shell completion from the command model.
    Completion(CompletionArgs),
    /// Update a standalone installation.
    Update(UpdateArgs),
    /// Show the top-level help.
    Help,
    /// Show version and result-contract metadata.
    Version,
}

#[derive(Debug, Args)]
pub(crate) struct CompileArgs {
    #[command(flatten)]
    pub(crate) common: CommonArgs,
    /// Write compiled output to PATH instead of stdout.
    #[arg(short = 'o', long, value_name = "PATH")]
    pub(crate) output: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct ConvertArgs {
    #[command(flatten)]
    pub(crate) common: CommonArgs,
    /// Reconstruction target.
    #[arg(long, value_name = "TARGET")]
    pub(crate) target: ConvertTargetArg,
}

#[derive(Debug, Args)]
pub(crate) struct CommonArgs {
    /// Input path, or `-`/omitted for standard input.
    #[arg(value_name = "INPUT")]
    pub(crate) input: Option<PathBuf>,
    /// Input frontend.
    #[arg(long, value_enum, default_value_t = SourceKindArg::Auto)]
    pub(crate) kind: SourceKindArg,
    /// Workshop client locale override.
    #[arg(long, value_name = "LOCALE")]
    pub(crate) locale: Option<String>,
    /// Include/project root for source inputs.
    #[arg(long, value_name = "DIR")]
    pub(crate) root: Option<PathBuf>,
    /// WIR transformation policy.
    #[arg(long, value_enum, default_value_t = ProfileArg::Off)]
    pub(crate) profile: ProfileArg,
    /// Output format.
    #[arg(short = 'f', long, value_enum, default_value_t = OutputFormatArg::Text)]
    pub(crate) format: OutputFormatArg,
    /// Renderer environment; `auto` detects terminal, CI, and GitHub Actions.
    #[arg(long, value_enum, default_value_t = RendererArg::Auto)]
    pub(crate) renderer: RendererArg,
    /// ANSI color policy.
    #[arg(long, value_enum, default_value_t = ColorArg::Auto)]
    pub(crate) color: ColorArg,
}

#[derive(Debug, Args)]
pub(crate) struct LintArgs {
    #[command(flatten)]
    pub(crate) common: CommonArgs,
    /// Disable a lint rule (repeatable).
    #[arg(long = "disable-rule", value_name = "ID")]
    pub(crate) disable_rule: Vec<String>,
    /// Override a lint rule severity as ID:warning or ID:info (repeatable).
    #[arg(long = "rule-severity", value_name = "ID:SEVERITY")]
    pub(crate) rule_severity: Vec<String>,
}

#[derive(Debug, Args)]
pub(crate) struct CompletionArgs {
    /// Shell to generate completion for.
    #[arg(value_enum, value_name = "SHELL")]
    pub(crate) shell: ShellArg,
}

#[derive(Debug, Args)]
pub(crate) struct UpdateArgs {
    /// Check for an update without modifying the installation.
    #[arg(long)]
    pub(crate) check: bool,
    /// Install an exact version instead of the latest stable release.
    #[arg(long, value_name = "VERSION")]
    pub(crate) version: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum SourceKindArg {
    Auto,
    Opy,
    Ostw,
    #[value(alias = "ws")]
    Workshop,
    #[value(alias = "hir", alias = "json")]
    Protocol,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum OutputFormatArg {
    #[value(alias = "human")]
    Text,
    #[value(alias = "machine")]
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ProfileArg {
    Off,
    Compat,
    Aggressive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ConvertTargetArg {
    Opy,
    Ostw,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum RendererArg {
    Auto,
    Terminal,
    Plain,
    #[value(name = "github-actions", alias = "github")]
    GithubActions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ColorArg {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ShellArg {
    Bash,
    Zsh,
    Fish,
    #[value(name = "powershell", alias = "pwsh")]
    PowerShell,
}
