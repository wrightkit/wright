mod agent;
mod agenttools;
mod cli;
mod completion;
mod mcp;
mod present;
mod provider;
mod rail;
mod serve;
mod tooldefs;
mod update;

use std::io::Write;
use std::io::{IsTerminal, Read};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{CommandFactory, Parser};
use wright_driver::config::{InputSpec, OutputFormat, SessionConfig, SourceKind};
use wright_driver::result::exit;
use wright_driver::source_provider::SourceBackend;
use wright_driver::{CompileWrite, OutputDest, Overwrite};

use crate::cli::{Cli, Command, CommonArgs, ConvertTargetArg, OutputFormatArg};

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
        Some(Command::Lookup(args)) => run_lookup(&args),
        Some(Command::Serve(args)) => serve::run(args),
        Some(Command::Completion(args)) => match args.subcommand {
            Some(cli::CompletionSubcommand::Install(install_args)) => {
                match completion::run_install(&install_args, args.shell) {
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
        Command::Compile(args) => run_compile(&args),
        Command::Convert(args) => {
            let target = match args.target {
                ConvertTargetArg::Opy => wright_driver::ConvertTarget::Opy,
                ConvertTargetArg::Ostw => wright_driver::ConvertTarget::Ostw,
            };
            run_configured(
                config_from_common(&args.common),
                present::Presentation::from_common(&args.common),
                None,
                move |session: &mut wright_driver::CompilerSession| session.convert(target),
            )
        }
        Command::Check(args) => {
            let mut config = config_from_common(&args.common);
            config.selection = selection_from_args(&args.select);
            run_configured(
                config,
                present::Presentation::from_common(&args.common).with_all(args.all),
                None,
                wright_driver::CompilerSession::check,
            )
        }
        Command::Analyze(args) => {
            let mut config = config_from_common(&args.common);
            config.selection = selection_from_args(&args.select);
            let presentation = present::Presentation::from_common(&args.common);
            run_configured(config, presentation.with_all(args.all), None, |session| {
                essential_envelope(
                    session.analyze(),
                    |value| wright_driver::essential::analyze(value, args.all),
                    |result| result.selection.clone(),
                )
            })
        }
        Command::Rename(args) => run_configured(
            config_from_common(&args.common),
            present::Presentation::from_common(&args.common),
            None,
            move |session| session.rename(&args.name, &args.to, args.write),
        ),
        Command::Lint(args) => {
            let mut config = config_from_common(&args.common);
            config.selection = selection_from_args(&args.select);
            if let Err(message) = args.policy.apply_to(&mut config) {
                eprintln!("wright: {message}");
                return ExitCode::from(exit::USAGE);
            }
            let presentation = present::Presentation::from_common(&args.common);
            if args.fix {
                return run_configured(config, presentation, None, move |session| {
                    session.lint_fix(args.write)
                });
            }
            run_configured(config, presentation.with_all(args.all), None, |session| {
                essential_envelope(
                    session.lint(),
                    |value| wright_driver::essential::lint(value, args.all),
                    |result| result.selection.clone(),
                )
            })
        }
        Command::Inspect(args) => match args.query {
            None => run_configured(
                config_from_common(&args.common),
                present::Presentation::from_common(&args.common).with_all(args.all),
                None,
                |session| {
                    essential_envelope(
                        session.inspect(),
                        |value| wright_driver::essential::inspect(value, args.all),
                        |_| None,
                    )
                },
            ),
            Some(cli::InspectQuery::Rules(query)) => run_configured(
                config_from_common(&query.common),
                present::Presentation::from_common(&query.common).with_all(query.all),
                None,
                move |session| {
                    session
                        .rules(
                            query.name,
                            query.rule,
                            query.file,
                            query
                                .max
                                .or_else(|| (query.all && query.content).then_some(usize::MAX)),
                            query.content,
                        )
                        .map_result(|result| {
                            wright_driver::result::RulesResult(wright_driver::essential::query(
                                &result.0, query.all,
                            ))
                        })
                },
            ),
            Some(cli::InspectQuery::Symbols(query)) => {
                let kind = query.only.map(|kind| kind.as_str().to_string());
                run_configured(
                    config_from_common(&query.common),
                    present::Presentation::from_common(&query.common).with_all(query.all),
                    None,
                    move |session| {
                        session
                            .symbols(kind, query.file, query.max)
                            .map_result(|result| {
                                wright_driver::result::SymbolsResult(
                                    wright_driver::essential::query(&result.0, query.all),
                                )
                            })
                    },
                )
            }
            Some(cli::InspectQuery::Refs(query)) => {
                let kind = query.only.map(|kind| kind.as_str().to_string());
                run_configured(
                    config_from_common(&query.common),
                    present::Presentation::from_common(&query.common).with_all(query.all),
                    None,
                    move |session| {
                        session
                            .refs(&query.name, kind, query.rule, query.file, query.max)
                            .map_result(|result| {
                                wright_driver::result::RefsResult(wright_driver::essential::query(
                                    &result.0, query.all,
                                ))
                            })
                    },
                )
            }
            Some(cli::InspectQuery::Cfg(query)) => {
                // The cfg payload carries blocks only; the addressed rule
                // name is the result's identity in text output (#446).
                let subject = query.rule.clone();
                let kind = query.only.map(|kind| kind.as_str().to_string());
                run_configured(
                    config_from_common(&query.common),
                    present::Presentation::from_common(&query.common).with_all(query.all),
                    Some(subject.as_str()),
                    move |session| {
                        session
                            .cfg(&query.rule, kind, query.max)
                            .map_result(|result| {
                                wright_driver::result::CfgResult(wright_driver::essential::query(
                                    &result.0, query.all,
                                ))
                            })
                    },
                )
            }
            Some(cli::InspectQuery::Callgraph(query)) => run_configured(
                config_from_common(&query.common),
                present::Presentation::from_common(&query.common).with_all(query.all),
                None,
                move |session| {
                    session
                        .callgraph(query.caller, query.callee, query.max)
                        .map_result(|result| {
                            wright_driver::result::CallGraphResult(wright_driver::essential::query(
                                &result.0, query.all,
                            ))
                        })
                },
            ),
            Some(cli::InspectQuery::Cost(query)) => {
                // The drill-down selector conflicts with the finding
                // selection: a selected rule's statements and node shares
                // are not findings, so `--severity`/`--rule-id`/`--max` have
                // nothing to select.
                if query.rule.is_some()
                    && (query.select.severity.is_some()
                        || query.select.rule_id.is_some()
                        || query.select.max.is_some()
                        || query.select.file.is_some())
                {
                    eprintln!(
                        "wright: 'inspect cost --rule' reports one rule's element cost; finding selection (--severity/--rule-id/--max/--file) does not apply"
                    );
                    return ExitCode::from(exit::USAGE);
                }
                let mut config = config_from_common(&query.common);
                config.selection = selection_from_args(&query.select);
                config.run = wright_driver::config::RunOptions {
                    no_cache: query.no_cache,
                    baseline: query.baseline.clone(),
                    save_baseline: query.save_baseline.clone(),
                    max_total: query.max_total,
                };
                run_configured(
                    config,
                    present::Presentation::from_common(&query.common).with_all(query.all),
                    None,
                    |session| {
                        session.cost(query.rule.clone()).map_result(|mut result| {
                            result.result =
                                wright_driver::essential::cost(&result.result, query.all);
                            result
                        })
                    },
                )
            }
        },
        Command::Completion(_)
        | Command::Update(_)
        | Command::Agent(_)
        | Command::Serve(_)
        | Command::Lookup(_)
        | Command::SemanticCompare(_) => {
            unreachable!("non-workflow command handled before run_workflow")
        }
    }
}

/// Apply the same essential result projection as the agent service.
fn essential_envelope<T: serde::Serialize>(
    envelope: wright_driver::Envelope<T>,
    form: impl FnOnce(&serde_json::Value) -> serde_json::Value,
    outcome: impl FnOnce(&T) -> Option<wright_driver::SelectionOutcome>,
) -> wright_driver::Envelope<wright_driver::EssentialResult> {
    envelope.map_result(|result| wright_driver::EssentialResult {
        result: form(&serde_json::to_value(&result).expect("result serializes")),
        selection: outcome(&result),
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

/// Run `wright compile` (ADR-0025): the program goes to a file (or stdout
/// with `-o -`); the report carries status, diagnostics, path and digest.
fn run_compile(args: &cli::CompileArgs) -> ExitCode {
    let to_stdout = args.output.as_deref().is_some_and(|p| p.as_os_str() == "-");
    if to_stdout && args.common.format == OutputFormatArg::Json {
        eprintln!(
            "wright: -o - prints the program to stdout and cannot be combined with --format json"
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
    compile_config.run = wright_driver::config::RunOptions {
        no_cache: args.no_cache,
        baseline: args.baseline.clone(),
        save_baseline: args.save_baseline.clone(),
        max_total: None,
    };
    run_configured(
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
            let mut envelope = session.compile_to(CompileWrite { dest, overwrite });
            let copied = envelope
                .result
                .output
                .as_ref()
                .filter(|_| copy && envelope.ok)
                .map(|output| copy_to_clipboard(&output.text));
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

/// Ask `Overwrite <path>? [y/N]`; only `y`/`yes` replaces the file.
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

fn copy_to_clipboard(text: &str) -> Result<(), arboard::Error> {
    arboard::Clipboard::new()?.set_text(text)
}

/// Run `wright lookup` (#529): the same `ToolRequest::Lookup` the agent
/// contract serves, over a session that never loads a project. The
/// anonymous text input gives the service's warm load nothing to read —
/// the `InputSpec::Stdin` default would block on a terminal's stdin to
/// EOF before the lookup ever runs. Text mode prints one line per entry —
/// the rendered signature for a callable, else the accepted spelling —
/// with its kind and identity; JSON mode prints the result `wright serve`
/// returns for the same request.
fn run_lookup(args: &cli::LookupArgs) -> ExitCode {
    let config = SessionConfig {
        input: InputSpec::Text {
            text: String::new(),
            path: None,
        },
        opy_provider: wright_driver::OpyProviderConfig {
            executable: args.opy_provider.clone(),
            ..wright_driver::OpyProviderConfig::default()
        },
        ..SessionConfig::default()
    };
    let mut session = match wright_driver::CompilerSession::new(config) {
        Ok(session) => session,
        Err(diagnostic) => {
            eprintln!("wright: {}", diagnostic.message);
            return ExitCode::from(exit::USAGE);
        }
    };
    let mut service = match wright_driver::service::ToolService::new(&mut session) {
        Ok(service) => service,
        Err(diagnostic) => {
            eprintln!("wright: {}", diagnostic.message);
            return ExitCode::from(exit::USAGE);
        }
    };
    let request = wright_driver::service::ToolRequest::Lookup {
        language: args.language.as_str().to_string(),
        query: args.query.clone(),
        kind: args.kind.map(|kind| kind.as_str().to_string()),
        within: args.within.clone(),
        locale: args.locale.clone(),
        limit: args.limit,
    };
    match service.handle(&request) {
        wright_driver::service::ToolResponse::Ok { result } => {
            match args.format {
                OutputFormatArg::Json => println!(
                    "{}",
                    serde_json::to_string(&result).expect("result serializes")
                ),
                OutputFormatArg::Text => print_lookup_text(&result),
            }
            if result.get("unavailable").is_some() {
                return ExitCode::from(exit::UNSUPPORTED);
            }
            if result["entries"].as_array().is_none_or(Vec::is_empty) {
                eprintln!("wright: lookup returned no entries");
                return ExitCode::from(exit::SOURCE_ERROR);
            }
            ExitCode::SUCCESS
        }
        wright_driver::service::ToolResponse::Error { error } => {
            match args.format {
                OutputFormatArg::Json => println!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({ "error": error }))
                        .expect("error serializes")
                ),
                OutputFormatArg::Text => {
                    eprintln!("wright: {}: {}", error.code, error.message)
                }
            }
            if error.code.starts_with("invalid-") {
                ExitCode::from(exit::USAGE)
            } else {
                ExitCode::from(exit::SOURCE_ERROR)
            }
        }
    }
}

/// One line per lookup entry in text mode: the rendered signature for a
/// callable, else the accepted spelling, then its kind, the owner identity
/// when it differs from the spelling, and the display name when that adds
/// information. An `unavailable` result names the owner and the missing
/// capability.
fn print_lookup_text(result: &serde_json::Value) {
    if let Some(unavailable) = result.get("unavailable") {
        eprintln!(
            "unavailable: {} ({})",
            unavailable["message"].as_str().unwrap_or_default(),
            result["owner"].as_str().unwrap_or_default()
        );
        return;
    }
    let Some(entries) = result["entries"].as_array() else {
        return;
    };
    for entry in entries {
        let spelling = entry["spelling"].as_str().unwrap_or_default();
        let head = entry["signature"].as_str().unwrap_or(spelling);
        let mut line = format!("{head} [{}]", entry["kind"].as_str().unwrap_or_default());
        let identity = entry["identity"].as_str().unwrap_or_default();
        if identity != spelling {
            line.push_str(&format!("  {identity}"));
        }
        if let Some(display) = entry["displayName"].as_str() {
            if display != spelling {
                line.push_str(&format!("  — {display}"));
            }
        }
        println!("{line}");
    }
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
        serde_json::to_string(&comparison).expect("semantic comparison serializes")
    );
    if comparison.equivalent {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn config_from_common(common: &CommonArgs) -> SessionConfig {
    let input = match &common.input {
        Some(path) if path.as_os_str() != "-" => InputSpec::Path(path.clone()),
        Some(_) => InputSpec::Stdin,
        None => InputSpec::Path(".".into()),
    };
    let directory_target = is_directory_input(common);
    SessionConfig {
        input,
        source_backend: if is_opy_input(common) {
            SourceBackend::Provider
        } else if common.kind == cli::SourceKindArg::Auto && directory_target {
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

    #[test]
    fn every_opy_workflow_selects_the_provider_backend() {
        let opy = common(Some("main.opy"), cli::SourceKindArg::Auto);
        assert_eq!(
            config_from_common(&opy).source_backend,
            SourceBackend::Provider
        );

        let workshop = common(Some("main.txt"), cli::SourceKindArg::Auto);
        assert_eq!(
            config_from_common(&workshop).source_backend,
            SourceBackend::Native
        );

        let explicit_opy = common(None, cli::SourceKindArg::Opy);
        assert_eq!(
            config_from_common(&explicit_opy).source_backend,
            SourceBackend::Provider
        );

        let mut explicit_provider = common(Some("main.opy"), cli::SourceKindArg::Auto);
        explicit_provider.opy_provider = Some("opy-provider".into());
        assert_eq!(
            config_from_common(&explicit_provider).source_backend,
            SourceBackend::Provider
        );
    }

    #[test]
    fn omitted_input_targets_the_current_directory_and_dash_stays_stdin() {
        let omitted = common(None, cli::SourceKindArg::Auto);
        assert_eq!(
            config_from_common(&omitted).input,
            InputSpec::Path(".".into())
        );
        assert_eq!(
            config_from_common(&omitted).source_backend,
            SourceBackend::Auto
        );

        let stdin = common(Some("-"), cli::SourceKindArg::Auto);
        assert_eq!(config_from_common(&stdin).input, InputSpec::Stdin);
        assert_eq!(
            config_from_common(&stdin).source_backend,
            SourceBackend::Native
        );
    }
}
