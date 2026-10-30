use std::io::{IsTerminal, Write};
use std::process::ExitCode;

use wright_driver::result::exit;
use wright_driver::{CompileWrite, OutputDest, Overwrite};

use crate::cli::{self, OutputFormatArg};
use crate::{config_from_common, present, run_configured};

pub(crate) fn run(args: &cli::CompileArgs) -> ExitCode {
    let to_stdout = args.output.as_deref().is_some_and(|p| p.as_os_str() == "-");
    if to_stdout && args.common.format == OutputFormatArg::Json {
        present::terminal::report_error(
            present::Presentation::from_common(&args.common),
            "USAGE",
            "compile",
            "-o - prints the program to stdout and cannot be combined with --format json",
        );
        return ExitCode::from(exit::USAGE);
    }
    let dest = match &args.output {
        _ if to_stdout => OutputDest::Stdout,
        Some(path) => OutputDest::Path(path.clone()),
        None => OutputDest::Default,
    };
    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    let copy = args.copy;
    let force = args.force;
    let mut compile_config = config_from_common(&args.common);
    if let Err(message) = args.policy.apply_to(&mut compile_config) {
        present::terminal::report_error(
            present::Presentation::from_common(&args.common),
            "USAGE",
            "compile",
            &message,
        );
        return ExitCode::from(exit::USAGE);
    }
    compile_config.run = wright_driver::config::RunOptions {
        no_cache: args.no_cache,
        baseline: args.baseline.clone(),
        save_baseline: args.save_baseline.clone(),
        max_total: None,
    };
    run_configured(
        "compile",
        compile_config,
        present::Presentation::from_common(&args.common),
        None,
        move |session: &mut wright_driver::CompilerSession| {
            let mut confirm = |path: &std::path::Path| {
                confirm_overwrite(
                    path,
                    &mut std::io::stdin().lock(),
                    &mut std::io::stderr().lock(),
                )
            };
            let overwrite = if force {
                Overwrite::Force
            } else if interactive {
                Overwrite::Ask(&mut confirm)
            } else {
                Overwrite::Refuse
            };
            let to = args.to.clone();
            let mut envelope = session.compile_to(CompileWrite {
                dest,
                overwrite,
                to,
            });
            let copied = envelope
                .result
                .output
                .as_ref()
                .filter(|_| copy && envelope.ok)
                .map(|output| {
                    arboard::Clipboard::new()
                        .and_then(|mut clipboard| clipboard.set_text(&output.text))
                });
            if let Some(Err(error)) = copied {
                envelope
                    .diagnostics
                    .push(wright_driver::Diagnostic::warning(
                        "clipboard-failed",
                        wright_driver::Stage::Emission,
                        format!("cannot copy to the clipboard: {error}"),
                    ));
            }
            envelope
        },
    )
}

fn confirm_overwrite(
    path: &std::path::Path,
    input: &mut impl std::io::BufRead,
    prompt: &mut impl Write,
) -> bool {
    let _ = write!(
        prompt,
        "Overwrite {}? [y/N] ",
        wright_driver::input::display_path(path)
    );
    let _ = prompt.flush();
    let mut answer = String::new();
    input.read_line(&mut answer).is_ok()
        && matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overwrite_prompt_defaults_to_no_and_accepts_only_yes() {
        for (answer, expected) in [
            ("y\n", true),
            ("Yes\n", true),
            ("\n", false),
            ("n\n", false),
            ("", false),
            ("maybe\n", false),
        ] {
            let mut prompt = Vec::new();
            let confirmed = confirm_overwrite(
                std::path::Path::new("mode.ow"),
                &mut answer.as_bytes(),
                &mut prompt,
            );
            assert_eq!(confirmed, expected, "{answer:?}");
            assert_eq!(
                String::from_utf8(prompt).unwrap(),
                "Overwrite mode.ow? [y/N] "
            );
        }
    }
}
