//! The `inspect` command's argument model: the query subcommands each
//! share the agent operation's name (ADR-0035), plus their narrowing
//! flags — the same flags the usage errors and help examples name
//! (#888).

use clap::{Args, Subcommand};

use super::{CfgBlockKindArg, CommonArgs, ReferenceKindArg, SelectArgs, SymbolKindArg};

/// Arguments of `inspect`: an optional query subcommand naming one detail
/// area, plus the shared workflow options used by the bare summary.
#[derive(Debug, Args)]
#[command(
    after_help = "EXAMPLES:\n    wright inspect rules --rule \"my rule\"\n    wright inspect cost --rule \"my rule\""
)]
pub(crate) struct InspectArgs {
    #[command(flatten)]
    pub(crate) common: CommonArgs,
    /// Return every rule name in the overview.
    #[arg(long)]
    pub(crate) all: bool,
    #[command(subcommand)]
    pub(crate) query: Option<InspectQuery>,
}

/// The `inspect` query subcommands (#429): each is the CLI entry for the
/// agent operation of the same name, so the top-level command surface stays
/// small (#439). Agent requests keep their flat operation names.
#[derive(Debug, Subcommand)]
pub(crate) enum InspectQuery {
    /// List rules; `--content` adds each rule's canonical conditions and actions.
    Rules(RulesArgs),
    /// List semantic symbols; `--only` narrows to one symbol kind.
    Symbols(SymbolsArgs),
    /// Show the references and usage counts of one symbol, addressed by name.
    Refs(RefsArgs),
    /// Show the control-flow graph of one rule, addressed by name.
    Cfg(CfgArgs),
    /// Show the subroutine call graph.
    Callgraph(CallgraphArgs),
    /// Report generated-resource counts and static findings.
    Cost(CostArgs),
}

/// Arguments of `inspect rules`: shared workflow options plus the rule
/// selection fields and `--content` (ADR-0024).
#[derive(Debug, Args)]
pub(crate) struct RulesArgs {
    /// Return every selected item instead of the default bounded report.
    #[arg(long)]
    pub(crate) all: bool,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
    /// Add each selected rule's conditions and actions as canonical call
    /// trees; without `--max`, at most 20 rules are returned.
    #[arg(long)]
    pub(crate) content: bool,
    /// Report only the rule with this declared name.
    #[arg(long, value_name = "RULE")]
    pub(crate) rule: Option<String>,
    /// Report only rules located in this source file; any spelling that
    /// resolves to the same file selects it.
    #[arg(long, value_name = "PATH")]
    pub(crate) file: Option<String>,
    /// Report at most N rules; withheld rules are reported, never silently
    /// dropped.
    #[arg(long, value_name = "N")]
    pub(crate) max: Option<usize>,
}

/// Arguments of `inspect symbols`: shared workflow options plus the
/// symbol-kind filter. The filter is `--only` rather than `--kind` because
/// `CommonArgs` already assigns `--kind` to input-frontend selection.
#[derive(Debug, Args)]
pub(crate) struct SymbolsArgs {
    /// Return every selected item instead of the default bounded report.
    #[arg(long)]
    pub(crate) all: bool,
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
    /// Return every selected item instead of the default bounded report.
    #[arg(long)]
    pub(crate) all: bool,
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
    /// Return every selected item instead of the default bounded report.
    #[arg(long)]
    pub(crate) all: bool,
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
    /// Return every selected item instead of the default bounded report.
    #[arg(long)]
    pub(crate) all: bool,
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

/// Arguments of `inspect cost`: the shared report options plus the
/// per-rule drill-down selector (#724) and the run-cache / element-total
/// gate options (ADR-0027, #723).
#[derive(Debug, Args)]
#[command(
    after_help = "EXAMPLES:\n    wright inspect cost --rule \"my rule\"\n    wright inspect cost --file src/main.ws"
)]
pub(crate) struct CostArgs {
    /// Return every selected item instead of the default bounded report.
    #[arg(long)]
    pub(crate) all: bool,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
    #[command(flatten)]
    pub(crate) select: SelectArgs,
    /// Show where one declared rule's element cost goes — its statements
    /// ranked by recursive count and the per-name shares that reconcile
    /// to its total.
    #[arg(long, value_name = "RULE")]
    pub(crate) rule: Option<String>,
    /// Neither read nor write the `.wright/` run cache.
    #[arg(long)]
    pub(crate) no_cache: bool,
    /// Compare this run's element cost against the named baseline.
    #[arg(long, value_name = "NAME")]
    pub(crate) baseline: Option<String>,
    /// Record this run's element cost under the named baseline.
    #[arg(long, value_name = "NAME")]
    pub(crate) save_baseline: Option<String>,
    /// Fail with exit 1 when the element total exceeds N; reads no cache.
    #[arg(long, value_name = "N")]
    pub(crate) max_total: Option<usize>,
}
