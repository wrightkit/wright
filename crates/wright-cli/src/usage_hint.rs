//! The CLI's command spelling wherever an error must name a runnable form
//! (#888): the usage subject, the `inspect <area>` narrowing hint a
//! surplus positional earns, and the `wright inspect` command the
//! neutral listing-operation marker renders to.

use clap::CommandFactory;

use crate::CLI_NAME;
use crate::cli::Cli;

/// The command a usage error names: the first argument that is a real
/// subcommand, else the CLI name itself.
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

/// The `inspect <area>` narrowing hint a surplus positional earns: the
/// parser's `unexpected argument` error names no working form, so the
/// usage error names the flag that narrows that query.
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

/// The CLI's rendering of the `{list:OP}`/`{list-all:OP}` marker: the
/// `wright inspect` command that lists the declared names — `wright
/// serve` renders the operation name on the same result (#888).
fn list_command(operation: &str, unbounded: bool) -> String {
    format!(
        "wright inspect {operation}{}",
        if unbounded { " --all" } else { "" }
    )
}

/// Render the listing markers every envelope diagnostic carries — the
/// CLI's own `wright inspect` command spelling (#888) — then collapse
/// the `{{` escapes user content carries.
pub(crate) fn render_inspect_diagnostics<T: serde::Serialize>(
    envelope: &mut wright_driver::Envelope<T>,
) {
    for diagnostic in &mut envelope.diagnostics {
        diagnostic.message = wright_driver::service::unescape(
            &wright_driver::service::render_list_markers(&diagnostic.message, list_command),
        );
    }
}
