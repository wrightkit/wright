//! Session- and service-construction failures (#1028): the diagnostic's
//! declared exit class sets the process exit, so every command reports
//! the word and code the diagnostic carries instead of a fixed usage
//! exit.

use std::process::ExitCode;

use wright_driver::Diagnostic;

use crate::present::{self, Presentation};

/// Report a `CompilerSession::new`/`ToolService::new` construction
/// diagnostic and return the exit code its declared class carries.
pub(crate) fn refuse(pres: Presentation, name: &str, diagnostic: &Diagnostic) -> ExitCode {
    let code = diagnostic.class.exit_code();
    present::terminal::report_error(
        pres,
        present::terminal::exit_word(code),
        name,
        &diagnostic.message,
    );
    ExitCode::from(code)
}
