//! The `lint` command's dispatch: `lint package <subcommand>` manages
//! GitHub lint rule packages over the wright-driver store (#844) — no
//! project session is involved — while bare `lint` runs the finding
//! workflow through the shared configured runner like the other workflow
//! commands.

mod package;
mod report;

use std::process::ExitCode;

use wright_driver::result::exit;

use crate::present::{self, Presentation};
use crate::{cli, essential_envelope, fail_on, run_configured, selection_from_args};
use crate::{config_from_common, present::terminal};

/// Run `wright lint`: the `package` subcommand goes to the store commands,
/// otherwise the lint finding workflow (plain or `--fix`) runs.
pub(crate) fn run(args: cli::LintArgs) -> ExitCode {
    if let Some(cli::LintSubcommand::Package(package)) = args.command {
        return package::run(&package);
    }
    let mut config = config_from_common(&args.common);
    config.selection = selection_from_args(&args.select);
    if let Err(message) = args.policy.apply_to(&mut config) {
        terminal::report_error(
            Presentation::from_common(&args.common),
            "USAGE",
            "lint",
            &message,
        );
        return ExitCode::from(exit::USAGE);
    }
    let presentation = present::Presentation::from_common(&args.common);
    if args.fix {
        return run_configured(
            "lint",
            config,
            presentation,
            fail_on(&args.common),
            None,
            move |session| session.lint_fix(args.write),
        );
    }
    run_configured(
        "lint",
        config,
        presentation.with_all(args.all),
        fail_on(&args.common),
        None,
        |session| {
            essential_envelope(
                session.lint(),
                |value| wright_driver::essential::lint(value, args.all),
                |result| result.selection.clone(),
            )
        },
    )
}
