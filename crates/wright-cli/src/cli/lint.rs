use clap::{Args, Subcommand};

use super::{ColorArg, CommonArgs, OutputFormatArg, RendererArg, SelectArgs};
use crate::serve::LintPolicyArgs;

/// Arguments of `wright lint`: the shared workflow options, the
/// finding-selection and policy flags, `--fix`/`--write`, and the
/// `package` subcommand.
#[derive(Debug, Args)]
pub(crate) struct LintArgs {
    #[command(flatten)]
    pub(crate) common: CommonArgs,
    #[command(flatten)]
    pub(crate) select: SelectArgs,
    /// Return every finding.
    #[arg(long)]
    pub(crate) all: bool,
    /// Show the automated fixes lint offers for fixable findings as a
    /// validated diff, without writing (#556).
    #[arg(long, conflicts_with = "all")]
    pub(crate) fix: bool,
    /// Apply the fixes `--fix` previews: each fix is re-validated against
    /// the current source before writing; a stale source refuses and
    /// writes nothing (#556).
    #[arg(long, requires = "fix")]
    pub(crate) write: bool,
    #[command(flatten)]
    pub(crate) policy: LintPolicyArgs,
    #[command(subcommand)]
    pub(crate) command: Option<LintSubcommand>,
}

/// The `lint` subcommands; only `package` exists today.
#[derive(Debug, Subcommand)]
pub(crate) enum LintSubcommand {
    /// Manage GitHub lint rule packages: search, install, list, outdated,
    /// update, remove (#844).
    Package(PackageArgs),
}

/// Arguments of `lint package`: the package lifecycle subcommand.
#[derive(Debug, Args)]
pub(crate) struct PackageArgs {
    #[command(subcommand)]
    pub(crate) command: LintPackageCommand,
}

/// The `lint package` subcommands.
#[derive(Debug, Subcommand)]
pub(crate) enum LintPackageCommand {
    /// Search GitHub repositories carrying the `wrightkit-lint` topic.
    Search(PackageSearchArgs),
    /// Download a GitHub lint rule package into the store, keyed by commit SHA.
    Install(PackageInstallArgs),
    /// List the package entries currently in the store.
    List(PackageOutputArgs),
    /// Check installed packages against the remote default-branch head
    /// without changing the store.
    Outdated(PackageOutputArgs),
    /// Install the current default-branch head for installed packages.
    Update(PackageUpdateArgs),
    /// Remove a package's store entries; only Wright-managed content is
    /// deleted.
    Remove(PackageRemoveArgs),
}

/// The output flags every `lint package` subcommand shares.
#[derive(Debug, Args)]
pub(crate) struct PackageOutputArgs {
    /// Output format.
    #[arg(short = 'f', long, value_enum, default_value_t = OutputFormatArg::Text)]
    pub(crate) format: OutputFormatArg,
    /// Renderer environment; `auto` detects terminal, CI, and GitHub Actions.
    #[arg(long, value_enum, default_value_t = RendererArg::Auto)]
    pub(crate) renderer: RendererArg,
    /// ANSI color policy.
    #[arg(long, value_enum, default_value_t = ColorArg::Auto)]
    pub(crate) color: ColorArg,
    /// Return every row instead of the default bounded report.
    #[arg(long)]
    pub(crate) all: bool,
}

/// Arguments of `lint package search`: optional terms narrowing the
/// `topic:wrightkit-lint` repository query.
#[derive(Debug, Args)]
pub(crate) struct PackageSearchArgs {
    /// Terms narrowing the repository search (for example `wait` or
    /// `org:myteam`); the `wrightkit-lint` topic filter always applies.
    #[arg(value_name = "QUERY")]
    pub(crate) query: Option<String>,
    #[command(flatten)]
    pub(crate) output: PackageOutputArgs,
}

/// Arguments of `lint package install`: a GitHub `owner/repo` source —
/// URL spellings are accepted — plus an optional branch, tag, or commit.
#[derive(Debug, Args)]
pub(crate) struct PackageInstallArgs {
    /// The package source: `owner/repo`, a github.com URL, or
    /// `owner/repo@<ref>`; `@<ref>` and URL spellings select the revision.
    #[arg(value_name = "SOURCE")]
    pub(crate) source: String,
    /// The branch, tag, or commit SHA to install; without it the remote
    /// default-branch head is used. A full SHA installs offline.
    #[arg(long = "ref", value_name = "REF")]
    pub(crate) reference: Option<String>,
    #[command(flatten)]
    pub(crate) output: PackageOutputArgs,
}

/// Arguments of `lint package update`: an optional `owner/repo` or
/// `owner/repo@<ref>` restricting which store entries are updated.
#[derive(Debug, Args)]
pub(crate) struct PackageUpdateArgs {
    /// Restrict the update to one repository, or to the one store entry a
    /// `@<ref>` resolves to; without it every installed package is checked.
    #[arg(value_name = "SOURCE")]
    pub(crate) source: Option<String>,
    #[command(flatten)]
    pub(crate) output: PackageOutputArgs,
}

/// Arguments of `lint package remove`: the `owner/repo` or
/// `owner/repo@<ref>` store entry to remove.
#[derive(Debug, Args)]
pub(crate) struct PackageRemoveArgs {
    /// The package to remove: `owner/repo` removes every installed entry,
    /// `owner/repo@<ref>` removes only the entry the ref resolves to.
    #[arg(value_name = "SOURCE")]
    pub(crate) source: String,
    #[command(flatten)]
    pub(crate) output: PackageOutputArgs,
}
