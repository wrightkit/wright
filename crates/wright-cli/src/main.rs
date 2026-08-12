//! `wright` — the primary Wright command-line interface (M6, issue #38).
//!
//! A thin presentation/argv layer over [`wright_driver::CompilerSession`]:
//! every subcommand builds a [`SessionConfig`] and renders the driver's typed
//! result envelope. Human-readable text and machine-readable JSON derive from
//! the same envelope, and exit codes follow the documented contract
//! (`0` success, `1` source error, `2` usage, `3` unsupported, `4` internal).

mod present;

use std::path::PathBuf;
use std::process::ExitCode;

use wright_driver::config::{InputSpec, OutputFormat, SessionConfig, SourceKind};
use wright_driver::result::exit;

/// The CLI name and version banner.
pub const CLI_NAME: &str = "wright";
pub const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The command-line help text (also the `--help` output).
pub const HELP: &str = "\
wright — the Wright compiler command-line interface

USAGE:
    wright <COMMAND> [OPTIONS] [INPUT]

COMMANDS:
    compile    Parse, lower, and emit Workshop text for the input
    check      Parse, validate, and analyze the input; report diagnostics
    analyze    Parse, lower, and run semantic analysis; report findings
    inspect    Parse, lower, and show the structural/semantic program model
    help       Show this help
    version    Show version and result-contract metadata

INPUT:
    A file path, or `-`/omitted to read from standard input.
    Input kind is detected from the extension (.opy, .json, .txt/.ws) or
    stdin content; pass --kind to override.

OPTIONS:
    --kind <KIND>       Input frontend: auto|opy|workshop|protocol
    --locale <LOCALE>   Workshop client locale override (e.g. en-US)
    --root <DIR>        Include root for .opy input (default: input directory)
    --profile <PROFILE> WIR transformation policy: off|compat|aggressive (default: off)
    -o, --output <PATH> Write compiled output to PATH (compile only)
    -f, --format <FMT>  Output format: text|json (default: text)
    -h, --help          Show this help

EXIT CODES:
    0  success
    1  source/user error (parse, validation, ambiguous input)
    2  usage error (unknown command, flag, or value)
    3  recognized but unsupported input or operation
    4  internal/environment failure

OUTPUT CONTRACT:
    Text mode writes the command result to stdout and diagnostics to stderr.
    JSON mode writes one `wright-result/v1` envelope to stdout and keeps
    stderr empty on success. Exit codes and the envelope shape are stable;
    human-readable wording is not part of the machine contract.
";

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(usage) => {
            eprintln!("{usage}");
            ExitCode::from(exit::USAGE)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Compile,
    Check,
    Analyze,
    Inspect,
}

impl Command {
    fn as_str(self) -> &'static str {
        match self {
            Command::Compile => "compile",
            Command::Check => "check",
            Command::Analyze => "analyze",
            Command::Inspect => "inspect",
        }
    }

    fn parse(name: &str) -> Option<Command> {
        Some(match name {
            "compile" => Command::Compile,
            "check" => Command::Check,
            "analyze" => Command::Analyze,
            "inspect" => Command::Inspect,
            _ => return None,
        })
    }
}

fn run() -> Result<u8, String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    // Global help/version without a subcommand.
    if args.is_empty() || args[0] == "help" {
        print!("{HELP}");
        return Ok(0);
    }
    if args[0] == "--help" || args[0] == "-h" {
        print!("{HELP}");
        return Ok(0);
    }
    if args[0] == "version" || args[0] == "--version" {
        println!(
            "{CLI_NAME} {CLI_VERSION} (wright-driver {})",
            wright_driver::result::DRIVER_VERSION
        );
        return Ok(0);
    }

    let command = Command::parse(&args[0])
        .ok_or_else(|| format!("wright: unknown command '{}'\n\n{HELP}", args[0]))?;
    args.remove(0);

    let mut config = SessionConfig::default();
    let mut help = false;
    let mut positional: Option<PathBuf> = None;

    while let Some(arg) = args.first().cloned() {
        match arg.as_str() {
            "-h" | "--help" => {
                help = true;
                args.remove(0);
            }
            "-f" | "--format" => {
                let value = take_value(&mut args, &arg)?;
                config.format = OutputFormat::parse(&value).ok_or_else(|| {
                    format!("wright: unknown format '{value}' (expected text|json)")
                })?;
            }
            "--kind" => {
                let value = take_value(&mut args, &arg)?;
                config.kind = SourceKind::parse(&value).ok_or_else(|| {
                    format!(
                        "wright: unknown input kind '{value}' (expected auto|opy|workshop|protocol)"
                    )
                })?;
            }
            "--locale" => {
                let value = take_value(&mut args, &arg)?;
                config.locale = Some(value);
            }
            "--root" => {
                let value = take_value(&mut args, &arg)?;
                config.root = Some(PathBuf::from(value));
            }
            "--profile" => {
                let value = take_value(&mut args, &arg)?;
                config.profile = wright_transform::Profile::parse(&value).ok_or_else(|| {
                    format!("wright: unknown profile '{value}' (expected off|compat|aggressive)")
                })?;
            }
            "-o" | "--output" => {
                if command != Command::Compile {
                    return Err(format!(
                        "wright: --output is only valid for `compile` (not `{}`)",
                        command.as_str()
                    ));
                }
                let value = take_value(&mut args, &arg)?;
                config.output = Some(PathBuf::from(value));
            }
            "-" => {
                config.input = InputSpec::Stdin;
                args.remove(0);
            }
            other => {
                if other.starts_with('-') {
                    return Err(format!(
                        "wright: unknown option '{other}' for `{}`\n\n{HELP}",
                        command.as_str()
                    ));
                }
                if positional.is_some() {
                    return Err(format!(
                        "wright: unexpected extra argument '{other}' for `{}`\n\n{HELP}",
                        command.as_str()
                    ));
                }
                positional = Some(PathBuf::from(other));
                args.remove(0);
            }
        }
    }

    if help {
        print!("{HELP}");
        return Ok(0);
    }

    config.input = match positional {
        Some(path) => InputSpec::Path(path),
        None => InputSpec::Stdin,
    };

    let mut session = wright_driver::CompilerSession::new(config)
        .map_err(|diagnostic| format!("wright: {}", diagnostic.message))?;

    let code = match command {
        Command::Compile => run_command(&mut session, wright_driver::CompilerSession::compile),
        Command::Check => run_command(&mut session, wright_driver::CompilerSession::check),
        Command::Analyze => run_command(&mut session, wright_driver::CompilerSession::analyze),
        Command::Inspect => run_command(&mut session, wright_driver::CompilerSession::inspect),
    };
    Ok(code)
}

/// Run one driver workflow and render its envelope in the session's format.
fn run_command<T: serde::Serialize>(
    session: &mut wright_driver::CompilerSession,
    run: fn(&mut wright_driver::CompilerSession) -> wright_driver::Envelope<T>,
) -> u8 {
    let envelope = run(session);
    let code = envelope.exit;
    present::render(&envelope, session.config.format);
    code
}

/// Take the value of an option that requires one, failing on a missing value.
fn take_value(args: &mut Vec<String>, option: &str) -> Result<String, String> {
    if args.len() < 2 {
        return Err(format!("wright: missing value for {option}"));
    }
    let value = args.remove(1);
    args.remove(0);
    Ok(value)
}
