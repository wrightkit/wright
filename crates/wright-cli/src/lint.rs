mod package;
mod report;

use std::process::ExitCode;

use wright_driver::result::exit;

use crate::present;
use crate::{cli, fail_on, run_configured, selection_from_args};
use crate::{config_from_common, present::terminal};

pub(crate) fn run(args: cli::LintArgs) -> ExitCode {
    if let Some(cli::LintSubcommand::Package(package)) = args.command {
        return package::run(&package);
    }
    let mut config = config_from_common(&args.common);
    config.selection = selection_from_args(&args.select);
    let presentation = present::Presentation::from_common(&args.common);
    if let Err(message) = args.policy.apply_to(&mut config) {
        terminal::report_error(presentation, "USAGE", "lint", &message);
        return ExitCode::from(exit::USAGE);
    }
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
        |session| crate::essential::lint(session.lint(), args.all),
    )
}
