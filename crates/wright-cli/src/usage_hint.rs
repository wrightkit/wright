use clap::CommandFactory;
use clap::error::ContextKind;

use crate::CLI_NAME;
use crate::cli::Cli;

/// The command path clap attributes a parse failure to, read from the usage
/// line embedded in the error itself. Resolving the subject this way keeps
/// option values elsewhere in argv from being mistaken for subcommand names.
fn usage_path(error: &clap::Error) -> Option<Vec<String>> {
    let usage = error.get(ContextKind::Usage)?.to_string();
    let usage = usage.trim().strip_prefix("Usage:").unwrap_or(usage.trim());
    Some(
        usage
            .split_whitespace()
            .take_while(|word| word.chars().next().is_some_and(char::is_alphanumeric))
            .map(str::to_string)
            .collect(),
    )
}

pub(crate) fn subject(args: &[std::ffi::OsString], error: &clap::Error) -> String {
    if let Some(path) = usage_path(error) {
        return path.get(1).cloned().unwrap_or_else(|| CLI_NAME.to_string());
    }
    // Help/version and value-validation errors carry no usage context;
    // resolve the first positional token, which is where clap expects the
    // subcommand name.
    let names: Vec<String> = Cli::command()
        .get_subcommands()
        .map(|command| command.get_name().to_string())
        .collect();
    args.iter()
        .skip(1)
        .filter_map(|arg| arg.to_str())
        .find(|arg| !arg.starts_with('-'))
        .filter(|arg| names.iter().any(|name| name == arg))
        .unwrap_or(CLI_NAME)
        .to_string()
}

pub(crate) fn narrowing(error: &clap::Error) -> Option<&'static str> {
    let path = usage_path(error)?;
    if path.get(1).map(String::as_str) != Some("inspect") {
        return None;
    }
    let area = path.get(2).map(String::as_str);
    Some(match area {
        Some("cost") => {
            "`inspect cost` narrows to one rule with --rule <NAME>; the INPUT path is the last positional"
        }
        Some("rules") => {
            "`inspect rules` narrows with --rule <NAME>, --file <PATH>, or --max <N>; the INPUT path is the last positional"
        }
        Some("symbols") => {
            "`inspect symbols` narrows with --only <KIND>, --file <PATH>, or --max <N>; the INPUT path is the last positional"
        }
        Some("refs") => {
            "`inspect refs` takes the symbol as <NAME> before the INPUT path; narrow with --only, --rule, --file, or --max"
        }
        Some("cfg") => {
            "`inspect cfg` takes the rule as <RULE> before the INPUT path; narrow with --only or --max"
        }
        Some("callgraph") => {
            "`inspect callgraph` narrows with --caller <RULE> or --callee <SUBROUTINE>; the INPUT path is the last positional"
        }
        _ => {
            "an inspect query narrows with its flags, not extra paths; the INPUT path is the last positional"
        }
    })
}

fn list_command(operation: &str, unbounded: bool) -> String {
    format!(
        "wright inspect {operation}{}",
        if unbounded { " --all" } else { "" }
    )
}

pub(crate) fn render_inspect_diagnostics<T: serde::Serialize>(
    envelope: &mut wright_driver::Envelope<T>,
) {
    for diagnostic in &mut envelope.diagnostics {
        wright_driver::service::compose_fields(
            &mut diagnostic.message,
            &mut diagnostic.fields,
            None,
            list_command,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::ffi::OsString;

    fn args(argv: &[&str]) -> Vec<OsString> {
        argv.iter().map(OsString::from).collect()
    }

    fn error(argv: &[&str]) -> clap::Error {
        Cli::try_parse_from(argv).expect_err(&format!("{argv:?} should fail parsing"))
    }

    #[test]
    fn flag_value_matching_a_subcommand_name_is_not_the_subject() {
        let argv = &["wright", "--rule", "inspect", "check"];
        assert_eq!(subject(&args(argv), &error(argv)), "wright");
    }

    #[test]
    fn subject_is_the_subcommand_owning_the_error() {
        let argv = &["wright", "inspect", "refs", "--bogus"];
        assert_eq!(subject(&args(argv), &error(argv)), "inspect");
        let argv = &["wright", "update", "self", "--bogus"];
        assert_eq!(subject(&args(argv), &error(argv)), "update");
        let argv = &["wright", "frobnicate"];
        assert_eq!(subject(&args(argv), &error(argv)), "wright");
    }

    #[test]
    fn help_requests_resolve_the_subcommand_positionally() {
        let argv = &["wright", "inspect", "--help"];
        assert_eq!(subject(&args(argv), &error(argv)), "inspect");
        let argv = &["wright", "--help"];
        assert_eq!(subject(&args(argv), &error(argv)), "wright");
    }

    #[test]
    fn narrowing_only_fires_inside_inspect() {
        let argv = &["wright", "--rule", "inspect", "check"];
        assert_eq!(narrowing(&error(argv)), None);
        let argv = &["wright", "inspect", "--bogus"];
        assert_eq!(
            narrowing(&error(argv)),
            Some(
                "an inspect query narrows with its flags, not extra paths; the INPUT path is the last positional"
            )
        );
        let argv = &["wright", "inspect", "cost", "--bogus"];
        assert!(narrowing(&error(argv)).is_some_and(|hint| hint.contains("inspect cost")));
    }
}
