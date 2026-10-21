use clap::CommandFactory;

use crate::CLI_NAME;
use crate::cli::Cli;

pub(crate) fn subject(args: &[std::ffi::OsString]) -> String {
    let names: Vec<String> = Cli::command()
        .get_subcommands()
        .map(|command| command.get_name().to_string())
        .collect();
    args.iter()
        .skip(1)
        .filter_map(|arg| arg.to_str())
        .find(|arg| names.iter().any(|name| name == arg))
        .unwrap_or(CLI_NAME)
        .to_string()
}

pub(crate) fn narrowing(args: &[std::ffi::OsString]) -> Option<&'static str> {
    let mut words = args
        .iter()
        .skip(1)
        .filter_map(|arg| arg.to_str())
        .skip_while(|arg| arg.starts_with('-'));
    if words.next() != Some("inspect") {
        return None;
    }
    let area = words.find(|arg| {
        matches!(
            *arg,
            "cost" | "rules" | "symbols" | "refs" | "cfg" | "callgraph"
        )
    });
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
