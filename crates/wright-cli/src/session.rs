use std::process::ExitCode;

use wright_driver::Diagnostic;

use crate::present::{self, Presentation};

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
