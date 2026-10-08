use std::path::PathBuf;

use crate::serve::ServeArgs;
use crate::update::UpdateArgs;
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

Commands check correctness, summarize semantic hotspots, lint, compile,
rename Workshop symbols, resolve owner names and signatures (`lookup`), or
reconstruct source through the typed wright-driver result envelope.
`inspect`
prints the semantic summary, and its query subcommands (symbols, refs, cfg,
callgraph, cost) expose each detail area. `compile` and `convert`
keep their source artifact stdout contracts; JSON mode prints only one
wright-result/v1 envelope to stdout. `serve` exposes the versioned
wright-agent/v1 session contract over stdio, JSON-RPC 2.0, or MCP.
`agent install` writes the canonical Wright agent guide into a project's
agent skills directory.

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
    --opy-provider <PATH> Explicit local first-party OPY provider executable
    --profile <PROFILE>  WIR transformation policy: off|compat|aggressive
    -o, --output <PATH>  Write compiled output to PATH (compile only)
    -f, --format <FMT>   Output format: text|json
    --renderer <MODE>    Presentation: auto|terminal|plain|github-actions
    --color <POLICY>     ANSI color: auto|always|never

LINT OPTIONS:
    --lint-config <PATH>       Read project lint configuration YAML
    --rule <PATH>              Load a local YAML rule file or directory (repeatable)
    --disable-rule <ID>         Disable a lint rule (repeatable)
    --rule-severity <ID>:<SEV>  Override a lint rule severity (repeatable)

FINDING SELECTION (check, analyze, lint, inspect cost):
    --severity <LEVEL>  Report findings at or above a severity: error|warning|info
    --rule-id <ID>      Report findings from one lint rule id only
    --file <PATH>       Report findings in one source file (any spelling that resolves to it)
    --max <N>           Report at most N findings (withheld counts are shown)

UPDATE OPTIONS:
    --check              Resolve update targets and report availability without modifying anything
    --version <VERSION>  Install an exact version (`update self`, `update provider <NAME>`)";

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Parse, lower, validate, and emit Workshop text.
    Compile(CompileArgs),
    /// Reconstruct validated Workshop input as canonical OPY source; OSTW is
    /// recognized but unavailable because DEL/OSTW provider support is not
    /// currently shipped.
    Convert(ConvertArgs),
    /// Check frontend, project, semantic, and validation correctness.
    Check(ReportArgs),
    /// Summarize Workshop cost, complexity hotspots, risk indicators, and
    /// cross-cutting state.
    Analyze(AnalyzeArgs),
    /// Parse, lower, and report lint findings.
    Lint(LintArgs),
    /// Parse, lower, and inspect semantic facts: the bare command prints the
    /// summary, and its query subcommands expose each detail area (#429).
    #[command(
        args_conflicts_with_subcommands = true,
        subcommand_precedence_over_arg = true
    )]
    Inspect(InspectArgs),
    /// Rename a Workshop variable or subroutine semantically (#434): previews
    /// the validated source diff by default; `--write` applies it atomically.
    Rename(RenameArgs),
    /// Resolve a display name, near spelling, or guess to the language
    /// owner's accepted spelling and signature (#529). Use it before writing
    /// in an unfamiliar language and when a name is rejected.
    Lookup(LookupArgs),
    /// Generate static shell completion from the command model.
    Completion(CompletionArgs),
    /// Update Wright-managed components: a standalone installation and
    /// installed first-party providers.
    Update(UpdateArgs),
    /// Install the canonical Wright agent guide into a project so coding
    /// agents discover and prefer Wright's semantic surfaces (#415).
    Agent(AgentArgs),
    /// Serve the versioned agent contract over stdio, JSON-RPC 2.0, or MCP.
    Serve(ServeArgs),
    /// Compare two Workshop texts using canonical WIR semantics (internal gate command).
    #[command(name = "semantic-compare", hide = true)]
    SemanticCompare(SemanticCompareArgs),
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
pub(crate) struct SemanticCompareArgs {
    /// Expected Workshop text path.
    pub(crate) expected: PathBuf,
    /// Actual Workshop text path, or `-` to read stdin.
    pub(crate) actual: PathBuf,
}

/// Arguments of commands that report findings: shared workflow options plus
/// the finding-selection options (#430).
#[derive(Debug, Args)]
pub(crate) struct ReportArgs {
    #[command(flatten)]
    pub(crate) common: CommonArgs,
    #[command(flatten)]
    pub(crate) select: SelectArgs,
}

/// Arguments of `analyze`: shared workflow options, finding selection, and
/// `--brief` — the summary form of the report (#532).
#[derive(Debug, Args)]
pub(crate) struct AnalyzeArgs {
    #[command(flatten)]
    pub(crate) common: CommonArgs,
    #[command(flatten)]
    pub(crate) select: SelectArgs,
    /// Print the brief form: counts, the costliest rules, and how to
    /// expand — the full report stays one option away.
    #[arg(long)]
    pub(crate) brief: bool,
}

/// Finding-selection options shared by `check`, `analyze`, `lint`, and
/// `inspect cost` (#429). Selection narrows reported output only — verdicts
/// and exit codes always reflect the complete set.
#[derive(Debug, Args, Default)]
pub(crate) struct SelectArgs {
    /// Report findings at or above this severity only.
    #[arg(long, value_enum, value_name = "LEVEL")]
    pub(crate) severity: Option<SeverityArg>,
    /// Report findings produced by this lint rule id only; an unknown id is
    /// a usage error.
    #[arg(long, value_name = "ID")]
    pub(crate) rule_id: Option<String>,
    /// Report findings located in this source file only; any spelling that
    /// resolves to the same file (as passed, root-relative, or absolute)
    /// selects it.
    #[arg(long, value_name = "PATH")]
    pub(crate) file: Option<String>,
    /// Report at most N findings; withheld findings are reported, never
    /// silently dropped.
    #[arg(long, value_name = "N")]
    pub(crate) max: Option<usize>,
}

#[derive(Debug, Args)]
pub(crate) struct CommonArgs {
    /// Input path; `-` reads standard input and an omitted path uses the current directory.
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
    /// Override the first-party OPY provider executable.
    #[arg(long, value_name = "PATH")]
    pub(crate) opy_provider: Option<PathBuf>,
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

/// Arguments of `rename` (#434): the declared symbol name and the new
/// identifier as positionals, then `[INPUT]` through the shared workflow
/// options. `wright rename` covers raw Workshop input only; source languages
/// are rename surfaces of their providers.
#[derive(Debug, Args)]
pub(crate) struct RenameArgs {
    /// The declared name of the variable or subroutine to rename.
    #[arg(value_name = "NAME")]
    pub(crate) name: String,
    /// The new identifier.
    #[arg(value_name = "NEW_NAME")]
    pub(crate) to: String,
    /// Apply the validated rename to the input file atomically instead of
    /// previewing the diff.
    #[arg(long)]
    pub(crate) write: bool,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

/// Arguments of `lookup` (#529): the free-text query, the language selector,
/// and the optional narrowing fields of the `lookup` operation. It answers
/// from language vocabulary alone, so it takes no input path.
#[derive(Debug, Args)]
pub(crate) struct LookupArgs {
    /// Free text to resolve: a display name, a near spelling, or a guess.
    #[arg(value_name = "QUERY")]
    pub(crate) query: Option<String>,
    /// The language vocabulary to query.
    #[arg(long, value_enum, value_name = "LANG", default_value_t = LookupLanguageArg::Workshop)]
    pub(crate) language: LookupLanguageArg,
    /// Restrict entries to one kind.
    #[arg(long, value_enum, value_name = "KIND")]
    pub(crate) kind: Option<LookupKindArg>,
    /// List the members, parameters, or settings children of this identity:
    /// an enum domain, a callable, or a settings path prefix.
    #[arg(long, value_name = "IDENTITY")]
    pub(crate) within: Option<String>,
    /// Workshop display-name locale.
    #[arg(long, value_name = "LOCALE")]
    pub(crate) locale: Option<String>,
    /// Maximum entries; the default is 3 and the maximum is 10.
    #[arg(long, value_name = "N")]
    pub(crate) limit: Option<usize>,
    /// Override the first-party OPY provider executable.
    #[arg(long, value_name = "PATH")]
    pub(crate) opy_provider: Option<PathBuf>,
    /// Output format.
    #[arg(short = 'f', long, value_enum, default_value_t = OutputFormatArg::Text)]
    pub(crate) format: OutputFormatArg,
}

/// The language a `lookup` request queries (`capabilities.languages`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum LookupLanguageArg {
    Workshop,
    Opy,
}

impl LookupLanguageArg {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Workshop => "workshop",
            Self::Opy => "opy",
        }
    }
}

/// Entry kinds the `lookup` operation filters on (ADR-0021).
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum LookupKindArg {
    Action,
    Value,
    Event,
    EnumMember,
    Setting,
}

impl LookupKindArg {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Action => "action",
            Self::Value => "value",
            Self::Event => "event",
            Self::EnumMember => "enumMember",
            Self::Setting => "setting",
        }
    }
}

/// Arguments of `inspect`: an optional query subcommand naming one detail
/// area, plus the shared workflow options used by the bare summary.
#[derive(Debug, Args)]
pub(crate) struct InspectArgs {
    #[command(flatten)]
    pub(crate) common: CommonArgs,
    /// Print the brief form of the bare summary: counts, the leading rules,
    /// and how to expand (#532). The query subcommands keep their own
    /// selection options.
    #[arg(long)]
    pub(crate) brief: bool,
    #[command(subcommand)]
    pub(crate) query: Option<InspectQuery>,
}

/// The `inspect` query subcommands (#429): each is the CLI entry for the
/// agent operation of the same name, so the top-level command surface stays
/// small (#439). Agent requests keep their flat operation names.
#[derive(Debug, Subcommand)]
pub(crate) enum InspectQuery {
    /// List semantic symbols; `--only` narrows to one symbol kind.
    Symbols(SymbolsArgs),
    /// Show the references and usage counts of one symbol, addressed by name.
    Refs(RefsArgs),
    /// Show the control-flow graph of one rule, addressed by name.
    Cfg(CfgArgs),
    /// Show the subroutine call graph.
    Callgraph(CallgraphArgs),
    /// Report generated-resource counts and static findings.
    Cost(ReportArgs),
}

/// Arguments of `inspect symbols`: shared workflow options plus the
/// symbol-kind filter. The filter is `--only` rather than `--kind` because
/// `CommonArgs` already assigns `--kind` to input-frontend selection.
#[derive(Debug, Args)]
pub(crate) struct SymbolsArgs {
    #[command(flatten)]
    pub(crate) common: CommonArgs,
    /// Report only symbols of this kind.
    #[arg(long, value_enum, value_name = "KIND")]
    pub(crate) only: Option<SymbolKindArg>,
    /// Report only symbols located in this source file; any spelling that
    /// resolves to the same file selects it.
    #[arg(long, value_name = "PATH")]
    pub(crate) file: Option<String>,
    /// Report at most N symbols; withheld symbols are reported, never
    /// silently dropped.
    #[arg(long, value_name = "N")]
    pub(crate) max: Option<usize>,
}

/// Arguments of `inspect refs`: the symbol name, then the shared workflow
/// options plus the reference selection fields (#531).
#[derive(Debug, Args)]
pub(crate) struct RefsArgs {
    /// The declared name of the symbol to look up.
    #[arg(value_name = "NAME")]
    pub(crate) name: String,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
    /// Report only references of this kind.
    #[arg(long, value_enum, value_name = "KIND")]
    pub(crate) only: Option<ReferenceKindArg>,
    /// Report only references located inside this rule.
    #[arg(long, value_name = "RULE")]
    pub(crate) rule: Option<String>,
    /// Report only references located in this source file; any spelling
    /// that resolves to the same file selects it.
    #[arg(long, value_name = "PATH")]
    pub(crate) file: Option<String>,
    /// Report at most N references; withheld references are reported, never
    /// silently dropped.
    #[arg(long, value_name = "N")]
    pub(crate) max: Option<usize>,
}

/// Arguments of `inspect cfg`: the rule name, then the shared workflow
/// options plus the block selection fields (#531).
#[derive(Debug, Args)]
pub(crate) struct CfgArgs {
    /// The declared name of the rule to look up.
    #[arg(value_name = "RULE")]
    pub(crate) rule: String,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
    /// Report only blocks of this kind.
    #[arg(long, value_enum, value_name = "KIND")]
    pub(crate) only: Option<CfgBlockKindArg>,
    /// Report at most N blocks; withheld blocks are reported, never
    /// silently dropped.
    #[arg(long, value_name = "N")]
    pub(crate) max: Option<usize>,
}

/// Arguments of `inspect callgraph`: shared workflow options plus the
/// edge selection fields (#531).
#[derive(Debug, Args)]
pub(crate) struct CallgraphArgs {
    #[command(flatten)]
    pub(crate) common: CommonArgs,
    /// Report only edges out of this rule.
    #[arg(long, value_name = "RULE")]
    pub(crate) caller: Option<String>,
    /// Report only edges into this subroutine.
    #[arg(long, value_name = "SUBROUTINE")]
    pub(crate) callee: Option<String>,
    /// Report at most N edges; withheld edges are reported, never silently
    /// dropped.
    #[arg(long, value_name = "N")]
    pub(crate) max: Option<usize>,
}

#[derive(Debug, Args)]
pub(crate) struct LintArgs {
    #[command(flatten)]
    pub(crate) common: CommonArgs,
    #[command(flatten)]
    pub(crate) select: SelectArgs,
    /// Print the brief form: finding counts by severity, the
    /// highest-severity findings, and how to expand (#532).
    #[arg(long)]
    pub(crate) brief: bool,
    /// Show the automated fixes lint offers for fixable findings as a
    /// validated diff, without writing (#556).
    #[arg(long, conflicts_with = "brief")]
    pub(crate) fix: bool,
    /// Apply the fixes `--fix` previews: each fix is re-validated against
    /// the current source before writing; a stale source refuses and
    /// writes nothing (#556).
    #[arg(long, requires = "fix")]
    pub(crate) write: bool,
    /// Read project lint configuration YAML.
    #[arg(long = "lint-config", value_name = "PATH")]
    pub(crate) lint_config: Option<PathBuf>,
    /// Load a local YAML rule file or directory (repeatable).
    #[arg(long = "rule", value_name = "PATH")]
    pub(crate) rule: Vec<PathBuf>,
    /// Disable a lint rule (repeatable).
    #[arg(long = "disable-rule", value_name = "ID")]
    pub(crate) disable_rule: Vec<String>,
    /// Override a lint rule policy as ID:off, ID:warn, or ID:error (repeatable).
    #[arg(long = "rule-severity", value_name = "ID:SEVERITY")]
    pub(crate) rule_severity: Vec<String>,
}

#[derive(Debug, Args)]
pub(crate) struct CompletionArgs {
    #[command(subcommand)]
    pub(crate) subcommand: Option<CompletionSubcommand>,

    /// Shell to generate completion for: bash|zsh|fish|powershell.
    #[arg(value_enum, value_name = "SHELL")]
    pub(crate) shell: Option<ShellArg>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum CompletionSubcommand {
    /// Install generated shell completions into standard user-local directories.
    Install(CompletionInstallArgs),
}

#[derive(Debug, Args)]
pub(crate) struct AgentArgs {
    #[command(subcommand)]
    pub(crate) subcommand: Option<AgentSubcommand>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum AgentSubcommand {
    /// Install the canonical Wright agent guide as an agent skill in the
    /// project; re-running refreshes it in place.
    Install(AgentInstallArgs),
    /// Emit the advertised agent operations as client tool definitions for
    /// code-executing agents, or as plain JSON Schema for other harnesses (#535).
    Tools(AgentToolsArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ToolsFormat {
    /// Anthropic Messages API client tool definitions for programmatic
    /// (code-execution) tool calling.
    Messages,
    /// One plain JSON Schema per operation for other harnesses.
    JsonSchema,
}

#[derive(Debug, Args)]
pub(crate) struct AgentToolsArgs {
    /// The emitted definition form.
    #[arg(long, value_enum, default_value_t = ToolsFormat::Messages)]
    pub(crate) format: ToolsFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum McpTarget {
    /// Claude Code: `.mcp.json`.
    Claude,
    /// Cursor: `.cursor/mcp.json`.
    Cursor,
    /// VS Code: `.vscode/mcp.json`.
    Vscode,
}

#[derive(Debug, Args)]
pub(crate) struct AgentInstallArgs {
    /// Destination skills directory; the guide installs under DIR/wright
    /// (default: .agents/skills in the current project).
    #[arg(long, value_name = "DIR")]
    pub(crate) dest: Option<PathBuf>,

    /// Replace whatever already occupies the guide path when it was not
    /// installed by `wright agent install`, or a `wright` MCP server entry
    /// that is not the one this command generates.
    #[arg(long)]
    pub(crate) force: bool,

    /// Report the destination without writing files.
    #[arg(long)]
    pub(crate) dry_run: bool,

    /// Also register Wright's MCP server (`wright serve --transport mcp`) in
    /// this harness's project-local config (#509).
    #[arg(long, value_enum, value_name = "TARGET")]
    pub(crate) mcp: Option<McpTarget>,

    /// Skip the agent guide and only register the MCP server.
    #[arg(long, requires = "mcp")]
    pub(crate) no_guide: bool,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct CompletionInstallArgs {
    /// Shell to install completion for (defaults to detecting the current shell).
    #[arg(value_enum, value_name = "SHELL")]
    pub(crate) shell: Option<ShellArg>,

    /// Shell override (alias for positional shell argument).
    #[arg(short = 's', long = "shell", value_enum)]
    pub(crate) shell_flag: Option<ShellArg>,

    /// Explicit destination directory for the completion script.
    #[arg(long, value_name = "DIR")]
    pub(crate) dir: Option<PathBuf>,

    /// Check or print what would be installed without writing files.
    #[arg(long)]
    pub(crate) dry_run: bool,

    /// Install/refresh completions for all supported shells.
    #[arg(long)]
    pub(crate) all: bool,

    /// Force overwrite of existing completion files.
    #[arg(long)]
    pub(crate) force: bool,
}

impl CompletionInstallArgs {
    pub(crate) fn effective_shell(&self) -> Option<ShellArg> {
        self.shell.or(self.shell_flag)
    }
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

/// Symbol kinds as spelled by the semantic index; `--only` accepts the
/// canonical names plus kebab-case aliases.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum SymbolKindArg {
    /// `variables.global` symbols.
    #[value(name = "globalVariable", alias = "global-variable")]
    GlobalVariable,
    /// `variables.player` symbols.
    #[value(name = "playerVariable", alias = "player-variable")]
    PlayerVariable,
    /// Subroutines.
    #[value(name = "subroutine")]
    Subroutine,
    /// Rules.
    #[value(name = "rule")]
    Rule,
}

impl SymbolKindArg {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::GlobalVariable => "globalVariable",
            Self::PlayerVariable => "playerVariable",
            Self::Subroutine => "subroutine",
            Self::Rule => "rule",
        }
    }
}

/// Reference kinds as spelled by the semantic index (#531).
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ReferenceKindArg {
    /// `declaration` references.
    #[value(name = "declaration")]
    Declaration,
    /// `definition` references.
    #[value(name = "definition")]
    Definition,
    /// `read` references.
    #[value(name = "read")]
    Read,
    /// `write` references.
    #[value(name = "write")]
    Write,
    /// `call` references.
    #[value(name = "call")]
    Call,
}

impl ReferenceKindArg {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Declaration => "declaration",
            Self::Definition => "definition",
            Self::Read => "read",
            Self::Write => "write",
            Self::Call => "call",
        }
    }
}

/// Control-flow block kinds as spelled by the `cfg` operation (#531).
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum CfgBlockKindArg {
    /// The `entry` block.
    #[value(name = "entry")]
    Entry,
    /// The `exit` block.
    #[value(name = "exit")]
    Exit,
    /// `block` basic blocks.
    #[value(name = "block")]
    Block,
    /// `if` branch headers.
    #[value(name = "if")]
    If,
    /// `while` loop headers.
    #[value(name = "while")]
    While,
    /// `for` loop headers.
    #[value(name = "for")]
    For,
}

impl CfgBlockKindArg {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Entry => "entry",
            Self::Exit => "exit",
            Self::Block => "block",
            Self::If => "if",
            Self::While => "while",
            Self::For => "for",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum SeverityArg {
    Error,
    #[value(alias = "warn")]
    Warning,
    Info,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ShellArg {
    Bash,
    Zsh,
    Fish,
    #[value(name = "powershell", alias = "pwsh")]
    PowerShell,
}

impl ShellArg {
    pub(crate) const ALL: [Self; 4] = [Self::Bash, Self::Zsh, Self::Fish, Self::PowerShell];

    pub(crate) fn metadata(self) -> (&'static str, &'static str, clap_complete::Shell) {
        match self {
            Self::Bash => ("bash", "wright", clap_complete::Shell::Bash),
            Self::Zsh => ("zsh", "_wright", clap_complete::Shell::Zsh),
            Self::Fish => ("fish", "wright.fish", clap_complete::Shell::Fish),
            Self::PowerShell => (
                "powershell",
                "_wright.ps1",
                clap_complete::Shell::PowerShell,
            ),
        }
    }

    pub(crate) fn as_str(&self) -> &'static str {
        self.metadata().0
    }

    pub(crate) fn to_clap_shell(self) -> clap_complete::Shell {
        self.metadata().2
    }
}
