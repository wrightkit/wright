//! `wright` — the primary Wright command-line interface.

mod cli;
mod completion;
mod present;
mod provider;
mod update;

use std::io::Write;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{CommandFactory, Parser};
use wright_driver::config::{InputSpec, OutputFormat, SessionConfig, SourceKind};
use wright_driver::result::exit;
use wright_driver::source_provider::SourceBackend;

use crate::cli::{
    Cli, Command, CommonArgs, ConvertTargetArg, OutputFormatArg, ProviderCommand, ProviderNameArg,
};

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
        None | Some(Command::Help) => {
            let mut command = Cli::command();
            let _ = command.print_help();
            println!();
            ExitCode::SUCCESS
        }
        Some(Command::Version) => {
            println!("{}", version_banner());
            ExitCode::SUCCESS
        }
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
        Some(Command::Update(args)) => match update::run(args.check, args.version.as_deref()) {
            Ok(code) => ExitCode::from(code),
            Err(error) => {
                eprintln!("wright: {}", error.message());
                ExitCode::from(error.exit_code())
            }
        },
        Some(Command::Provider(args)) => match args.command {
            ProviderCommand::Update(update) => match update.provider {
                ProviderNameArg::Opy => match provider::update(update.version.as_deref()) {
                    Ok(resolved) => {
                        println!(
                            "installed first-party OPY provider {} at {}",
                            resolved.version.as_deref().unwrap_or("local"),
                            resolved.executable.display()
                        );
                        ExitCode::SUCCESS
                    }
                    Err(error) => {
                        eprintln!("wright: {}: {}", error.code(), error);
                        ExitCode::from(error.exit_code())
                    }
                },
            },
        },
        Some(command) => run_workflow(command),
    }
}

fn run_workflow(command: Command) -> ExitCode {
    let (name, config, presentation, convert_target) = match command {
        Command::Compile(args) => {
            let mut config = config_from_common(&args.common, true);
            config.output = args.output;
            (
                "compile",
                config,
                present::Presentation::from_common(&args.common),
                None,
            )
        }
        Command::Convert(args) => (
            "convert",
            config_from_common(&args.common, false),
            present::Presentation::from_common(&args.common),
            Some(args.target),
        ),
        Command::Check(args) => (
            "check",
            config_from_common(&args, true),
            present::Presentation::from_common(&args),
            None,
        ),
        Command::Analyze(args) => (
            "analyze",
            config_from_common(&args, false),
            present::Presentation::from_common(&args),
            None,
        ),
        Command::Lint(args) => {
            let mut config = config_from_common(&args.common, false);
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
                    eprintln!("wright: unknown severity '{severity}' (expected warning|info)");
                    return ExitCode::from(exit::USAGE);
                }
            }
            (
                "lint",
                config,
                present::Presentation::from_common(&args.common),
                None,
            )
        }
        Command::Inspect(args) => (
            "inspect",
            config_from_common(&args, false),
            present::Presentation::from_common(&args),
            None,
        ),
        Command::Completion(_)
        | Command::Update(_)
        | Command::Provider(_)
        | Command::Help
        | Command::Version => {
            unreachable!("non-workflow command handled before run_workflow")
        }
    };

    let mut session = match wright_driver::CompilerSession::new(config) {
        Ok(session) => session,
        Err(diagnostic) => {
            eprintln!("wright: {}", diagnostic.message);
            return ExitCode::from(exit::USAGE);
        }
    };

    let code = match name {
        "compile" => run_command(
            &mut session,
            wright_driver::CompilerSession::compile,
            presentation,
        ),
        "check" => run_command(
            &mut session,
            wright_driver::CompilerSession::check,
            presentation,
        ),
        "analyze" => run_command(
            &mut session,
            wright_driver::CompilerSession::analyze,
            presentation,
        ),
        "lint" => run_command(
            &mut session,
            wright_driver::CompilerSession::lint,
            presentation,
        ),
        "inspect" => run_command(
            &mut session,
            wright_driver::CompilerSession::inspect,
            presentation,
        ),
        "convert" => {
            let target = match convert_target.expect("convert target is required") {
                ConvertTargetArg::Opy => wright_driver::ConvertTarget::Opy,
                ConvertTargetArg::Ostw => wright_driver::ConvertTarget::Ostw,
            };
            let activity = Arc::new(presentation.activity());
            session.set_progress_observer(activity.clone());
            let envelope = session.convert(target);
            session.clear_progress_observer();
            drop(activity);
            let code = envelope.exit;
            present::render(&envelope, presentation);
            code
        }
        _ => unreachable!("all workflow commands are mapped"),
    };
    ExitCode::from(code)
}

fn config_from_common(common: &CommonArgs, provider_workflow: bool) -> SessionConfig {
    let input = match &common.input {
        Some(path) if path.as_os_str() != "-" => InputSpec::Path(path.clone()),
        _ => InputSpec::Stdin,
    };
    SessionConfig {
        input,
        source_backend: if is_opy_input(common)
            && (provider_workflow || common.opy_provider.is_some())
        {
            SourceBackend::Provider
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
            cli::ProfileArg::Off => wright_transform::Profile::Off,
            cli::ProfileArg::Compat => wright_transform::Profile::Compat,
            cli::ProfileArg::Aggressive => wright_transform::Profile::Aggressive,
        },
        ..SessionConfig::default()
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

/// Run one driver workflow and render its envelope in the CLI presentation.
fn run_command<T: serde::Serialize>(
    session: &mut wright_driver::CompilerSession,
    run: fn(&mut wright_driver::CompilerSession) -> wright_driver::Envelope<T>,
    presentation: present::Presentation,
) -> u8 {
    let activity = Arc::new(presentation.activity());
    session.set_progress_observer(activity.clone());
    let envelope = run(session);
    session.clear_progress_observer();
    drop(activity);
    let code = envelope.exit;
    present::render(&envelope, presentation);
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
    fn ordinary_opy_check_and_compile_select_the_provider_backend() {
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
            SourceBackend::Native
        );

        let mut explicit_provider = common(Some("main.opy"), cli::SourceKindArg::Auto);
        explicit_provider.opy_provider = Some("opy-provider".into());
        assert_eq!(
            config_from_common(&explicit_provider, false).source_backend,
            SourceBackend::Provider
        );
    }
}
