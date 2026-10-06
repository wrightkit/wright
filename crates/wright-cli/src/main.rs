mod agent;
mod agenttools;
mod cli;
mod completion;
mod mcp;
mod present;
mod provider;
mod serve;
mod tooldefs;
mod update;

use std::io::Read;
use std::io::Write;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{CommandFactory, Parser};
use wright_driver::config::{InputSpec, LintConfig, OutputFormat, SessionConfig, SourceKind};
use wright_driver::result::exit;
use wright_driver::source_provider::SourceBackend;

use crate::cli::{Cli, Command, CommonArgs, ConvertTargetArg, OutputFormatArg};

/// The CLI name and version banner.
pub const CLI_NAME: &str = "wright";
pub const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

fn version_banner() -> String {
    format!(
        "{CLI_NAME} {CLI_VERSION} (wright-driver {})",
        wright_driver::result::DRIVER_VERSION
    )
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() == 2 && args[1] == "--version" {
        println!("{}", version_banner());
        return ExitCode::SUCCESS;
    }

    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            let code = error.exit_code();
            let _ = error.print();
            return ExitCode::from(code as u8);
        }
    };

    if cli.version {
        println!("{}", version_banner());
        return ExitCode::SUCCESS;
    }

    match cli.command {
        None => {
            let mut command = Cli::command();
            let _ = command.print_help();
            println!();
            ExitCode::SUCCESS
        }
        Some(Command::SemanticCompare(args)) => run_semantic_compare(args),
        Some(Command::Serve(args)) => serve::run(args),
        Some(Command::Completion(args)) => match args.subcommand {
            Some(cli::CompletionSubcommand::Install(install_args)) => {
                match completion::run_install(&install_args) {
                    Ok(code) => ExitCode::from(code),
                    Err(error) => {
                        eprintln!("wright: {}", error.message());
                        ExitCode::from(error.exit_code())
                    }
                }
            }
            None => match args.shell {
                Some(shell) => {
                    let bytes = completion::generate_script(shell);
                    let _ = std::io::stdout().write_all(&bytes);
                    ExitCode::SUCCESS
                }
                None => {
                    eprintln!(
                        "wright: specify a shell (bash, zsh, fish, powershell) or 'install' (run 'wright completion --help' for details)"
                    );
                    ExitCode::from(exit::USAGE)
                }
            },
        },
        Some(Command::Update(args)) => match update::run(&args) {
            Ok(code) => ExitCode::from(code),
            Err(error) => {
                eprintln!("wright: {}", error.message());
                ExitCode::from(error.exit_code())
            }
        },
        Some(Command::Agent(args)) => match agent::run(&args) {
            Ok(code) => ExitCode::from(code),
            Err(error) => {
                eprintln!("wright: {}", error.message());
                ExitCode::from(error.exit_code())
            }
        },
        Some(command) => run_workflow(command),
    }
}

fn run_workflow(command: Command) -> ExitCode {
    match command {
        Command::Compile(args) => {
            let mut config = config_from_common(&args.common, true);
            config.output = args.output;
            run_configured(
                config,
                present::Presentation::from_common(&args.common),
                None,
                wright_driver::CompilerSession::compile,
            )
        }
        Command::Convert(args) => {
            let target = match args.target {
                ConvertTargetArg::Opy => wright_driver::ConvertTarget::Opy,
                ConvertTargetArg::Ostw => wright_driver::ConvertTarget::Ostw,
            };
            run_configured(
                config_from_common(&args.common, false),
                present::Presentation::from_common(&args.common),
                None,
                move |session: &mut wright_driver::CompilerSession| session.convert(target),
            )
        }
        Command::Check(args) => {
            let mut config = config_from_common(&args.common, true);
            config.selection = selection_from_args(&args.select);
            run_configured(
                config,
                present::Presentation::from_common(&args.common),
                None,
                wright_driver::CompilerSession::check,
            )
        }
        Command::Analyze(args) => {
            let mut config = config_from_common(&args.common, true);
            config.selection = selection_from_args(&args.select);
            let presentation = present::Presentation::from_common(&args.common);
            if args.brief {
                run_configured(config, presentation, None, |session| {
                    brief_envelope(session.analyze(), wright_driver::brief::analyze)
                })
            } else {
                run_configured(
                    config,
                    presentation,
                    None,
                    wright_driver::CompilerSession::analyze,
                )
            }
        }
        Command::Rename(args) => run_configured(
            config_from_common(&args.common, false),
            present::Presentation::from_common(&args.common),
            None,
            move |session| session.rename(&args.name, &args.to, args.write),
        ),
        Command::Lint(args) => {
            let mut config = config_from_common(&args.common, true);
            config.selection = selection_from_args(&args.select);
            if let Some(path) = &args.lint_config {
                config.lint = match LintConfig::from_yaml_path(path) {
                    Ok(config) => config,
                    Err(error) => {
                        eprintln!(
                            "wright: cannot read lint config {}: {error}",
                            path.display()
                        );
                        return ExitCode::from(exit::USAGE);
                    }
                };
            }
            config.lint_rule_paths = args.rule.clone();
            for rule in &args.disable_rule {
                config.lint.disable(rule);
            }
            for value in &args.rule_severity {
                let (rule_id, severity) = match value.split_once(':') {
                    Some(parts) => parts,
                    None => {
                        eprintln!(
                            "wright: --rule-severity expects <ID>:<SEVERITY> (got '{value}')"
                        );
                        return ExitCode::from(exit::USAGE);
                    }
                };
                if !config.lint.set_severity_by_name(rule_id, severity) {
                    eprintln!("wright: unknown severity '{severity}' (expected off|warn|error)");
                    return ExitCode::from(exit::USAGE);
                }
            }
            let presentation = present::Presentation::from_common(&args.common);
            if args.brief {
                return run_configured(config, presentation, None, |session| {
                    brief_envelope(session.lint(), wright_driver::brief::lint)
                });
            }
            run_configured(
                config,
                presentation,
                None,
                wright_driver::CompilerSession::lint,
            )
        }
        Command::Inspect(args) => match args.query {
            None if args.brief => run_configured(
                config_from_common(&args.common, false),
                present::Presentation::from_common(&args.common),
                None,
                |session| brief_envelope(session.inspect(), wright_driver::brief::inspect),
            ),
            None => run_configured(
                config_from_common(&args.common, false),
                present::Presentation::from_common(&args.common),
                None,
                wright_driver::CompilerSession::inspect,
            ),
            Some(cli::InspectQuery::Symbols(query)) => {
                let kind = query.only.map(|kind| kind.as_str().to_string());
                run_configured(
                    config_from_common(&query.common, false),
                    present::Presentation::from_common(&query.common),
                    None,
                    move |session| session.symbols(kind, query.file, query.max),
                )
            }
            Some(cli::InspectQuery::Refs(query)) => {
                let kind = query.only.map(|kind| kind.as_str().to_string());
                run_configured(
                    config_from_common(&query.common, false),
                    present::Presentation::from_common(&query.common),
                    None,
                    move |session| {
                        session.refs(&query.name, kind, query.rule, query.file, query.max)
                    },
                )
            }
            Some(cli::InspectQuery::Cfg(query)) => {
                // The cfg payload carries blocks only; the addressed rule
                // name is the result's identity in text output (#446).
                let subject = query.rule.clone();
                let kind = query.only.map(|kind| kind.as_str().to_string());
                run_configured(
                    config_from_common(&query.common, false),
                    present::Presentation::from_common(&query.common),
                    Some(subject.as_str()),
                    move |session| session.cfg(&query.rule, kind, query.max),
                )
            }
            Some(cli::InspectQuery::Callgraph(query)) => run_configured(
                config_from_common(&query.common, false),
                present::Presentation::from_common(&query.common),
                None,
                move |session| session.callgraph(query.caller, query.callee, query.max),
            ),
            Some(cli::InspectQuery::Cost(query)) => {
                let mut config = config_from_common(&query.common, true);
                config.selection = selection_from_args(&query.select);
                run_configured(
                    config,
                    present::Presentation::from_common(&query.common),
                    None,
                    wright_driver::CompilerSession::cost,
                )
            }
        },
        Command::Completion(_)
        | Command::Update(_)
        | Command::Agent(_)
        | Command::Serve(_)
        | Command::SemanticCompare(_) => {
            unreachable!("non-workflow command handled before run_workflow")
        }
    }
}

/// The `--brief` result form of a workflow envelope (#532): the driver's
/// brief transform builds the same payload the `serve`/`MCP` `brief`
/// request fields return, so every surface reports the same summary.
fn brief_envelope<T: serde::Serialize>(
    envelope: wright_driver::Envelope<T>,
    form: impl FnOnce(&serde_json::Value) -> serde_json::Value,
) -> wright_driver::Envelope<wright_driver::BriefResult> {
    envelope.map_result(|result| {
        wright_driver::BriefResult(form(
            &serde_json::to_value(result).expect("result serializes"),
        ))
    })
}

fn run_configured<T: serde::Serialize + present::ResultPresentation>(
    config: SessionConfig,
    presentation: present::Presentation,
    subject: Option<&str>,
    run: impl FnOnce(&mut wright_driver::CompilerSession) -> wright_driver::Envelope<T>,
) -> ExitCode {
    let mut session = match wright_driver::CompilerSession::new(config) {
        Ok(session) => session,
        Err(diagnostic) => {
            eprintln!("wright: {}", diagnostic.message);
            return ExitCode::from(exit::USAGE);
        }
    };

    ExitCode::from(run_command(&mut session, run, presentation, subject))
}

fn run_semantic_compare(args: cli::SemanticCompareArgs) -> ExitCode {
    let expected = match std::fs::read_to_string(&args.expected) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("wright: cannot read expected Workshop text: {error}");
            return ExitCode::from(4);
        }
    };
    let actual = if args.actual.as_os_str() == "-" {
        let mut text = String::new();
        if let Err(error) = std::io::stdin().read_to_string(&mut text) {
            eprintln!("wright: cannot read actual Workshop text from stdin: {error}");
            return ExitCode::from(4);
        }
        text
    } else {
        match std::fs::read_to_string(&args.actual) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("wright: cannot read actual Workshop text: {error}");
                return ExitCode::from(4);
            }
        }
    };
    let comparison = wright_driver::compare_workshop_texts(&expected, &actual);
    println!(
        "{}",
        serde_json::to_string_pretty(&comparison).expect("semantic comparison serializes")
    );
    if comparison.equivalent {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn config_from_common(common: &CommonArgs, provider_workflow: bool) -> SessionConfig {
    let input = match &common.input {
        Some(path) if path.as_os_str() != "-" => InputSpec::Path(path.clone()),
        Some(_) => InputSpec::Stdin,
        None => InputSpec::Path(".".into()),
    };
    let directory_target = is_directory_input(common);
    let provider_requested = provider_workflow || common.opy_provider.is_some() || directory_target;
    SessionConfig {
        input,
        source_backend: if is_opy_input(common) {
            SourceBackend::Provider
        } else if provider_requested && common.kind == cli::SourceKindArg::Auto && directory_target
        {
            SourceBackend::Auto
        } else {
            SourceBackend::Native
        },
        kind: match common.kind {
            cli::SourceKindArg::Auto => SourceKind::Auto,
            cli::SourceKindArg::Opy => SourceKind::Opy,
            cli::SourceKindArg::Ostw => SourceKind::Ostw,
            cli::SourceKindArg::Workshop => SourceKind::Workshop,
            cli::SourceKindArg::Protocol => SourceKind::Protocol,
        },
        locale: common.locale.clone(),
        root: common.root.clone(),
        opy_provider: wright_driver::OpyProviderConfig {
            executable: common.opy_provider.clone(),
            ..wright_driver::OpyProviderConfig::default()
        },
        format: match common.format {
            OutputFormatArg::Text => OutputFormat::Text,
            OutputFormatArg::Json => OutputFormat::Json,
        },
        profile: match common.profile {
            cli::ProfileArg::Off => wright_driver::Profile::Off,
            cli::ProfileArg::Compat => wright_driver::Profile::Compat,
            cli::ProfileArg::Aggressive => wright_driver::Profile::Aggressive,
        },
        ..SessionConfig::default()
    }
}

/// Map the CLI finding-selection flags onto the shared driver selection
/// model; the driver applies it to diagnostics and lint findings alike (#430).
fn selection_from_args(select: &cli::SelectArgs) -> wright_driver::FindingSelection {
    wright_driver::FindingSelection {
        severity: select.severity.map(|severity| match severity {
            cli::SeverityArg::Error => wright_driver::Severity::Error,
            cli::SeverityArg::Warning => wright_driver::Severity::Warning,
            cli::SeverityArg::Info => wright_driver::Severity::Info,
        }),
        rule: select.rule_id.clone(),
        file: select.file.clone(),
        max: select.max,
    }
}

fn is_opy_input(common: &CommonArgs) -> bool {
    match common.kind {
        cli::SourceKindArg::Opy => true,
        cli::SourceKindArg::Auto => common
            .input
            .as_deref()
            .and_then(|path| path.extension())
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("opy")),
        _ => false,
    }
}

fn is_directory_input(common: &CommonArgs) -> bool {
    match common.input.as_deref() {
        None => true,
        Some(path) if path.as_os_str() == "." => true,
        Some(path) if path.as_os_str() == "-" => false,
        Some(path) => std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir()),
    }
}

/// Run one driver workflow and render its envelope in the CLI presentation.
fn run_command<T: serde::Serialize + present::ResultPresentation>(
    session: &mut wright_driver::CompilerSession,
    run: impl FnOnce(&mut wright_driver::CompilerSession) -> wright_driver::Envelope<T>,
    presentation: present::Presentation,
    subject: Option<&str>,
) -> u8 {
    let activity = Arc::new(presentation.activity());
    session.set_progress_observer(activity.clone());
    let started = std::time::Instant::now();
    let envelope = run(session);
    let elapsed = started.elapsed();
    session.clear_progress_observer();
    // The transient activity line is fully cleared before the final result
    // renders, so progress text never competes with the verdict (#443).
    drop(activity);
    let code = envelope.exit;
    let source_base = session.input_root();
    present::render(
        &envelope,
        presentation,
        elapsed,
        source_base.as_deref(),
        subject,
    );
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    fn common(input: Option<&str>, kind: cli::SourceKindArg) -> CommonArgs {
        CommonArgs {
            input: input.map(Into::into),
            kind,
            locale: None,
            root: None,
            opy_provider: None,
            profile: cli::ProfileArg::Off,
            format: cli::OutputFormatArg::Json,
            renderer: cli::RendererArg::Plain,
            color: cli::ColorArg::Never,
        }
    }

    #[test]
    fn every_opy_workflow_selects_the_provider_backend() {
        let opy = common(Some("main.opy"), cli::SourceKindArg::Auto);
        assert_eq!(
            config_from_common(&opy, true).source_backend,
            SourceBackend::Provider
        );

        let workshop = common(Some("main.txt"), cli::SourceKindArg::Auto);
        assert_eq!(
            config_from_common(&workshop, true).source_backend,
            SourceBackend::Native
        );

        let explicit_opy = common(None, cli::SourceKindArg::Opy);
        assert_eq!(
            config_from_common(&explicit_opy, true).source_backend,
            SourceBackend::Provider
        );
        assert_eq!(
            config_from_common(&opy, false).source_backend,
            SourceBackend::Provider
        );

        let mut explicit_provider = common(Some("main.opy"), cli::SourceKindArg::Auto);
        explicit_provider.opy_provider = Some("opy-provider".into());
        assert_eq!(
            config_from_common(&explicit_provider, false).source_backend,
            SourceBackend::Provider
        );
    }

    #[test]
    fn omitted_input_targets_the_current_directory_and_dash_stays_stdin() {
        let omitted = common(None, cli::SourceKindArg::Auto);
        assert_eq!(
            config_from_common(&omitted, true).input,
            InputSpec::Path(".".into())
        );
        assert_eq!(
            config_from_common(&omitted, true).source_backend,
            SourceBackend::Auto
        );

        let stdin = common(Some("-"), cli::SourceKindArg::Auto);
        assert_eq!(config_from_common(&stdin, true).input, InputSpec::Stdin);
        assert_eq!(
            config_from_common(&stdin, true).source_backend,
            SourceBackend::Native
        );
    }
}
