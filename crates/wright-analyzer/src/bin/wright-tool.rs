//! `wright-tool` — the read-only agent/tool interface over Wright's semantic
//! services.
//!
//! Loads a compiled program either from Opy HIR v1 protocol JSON (as produced
//! by the adapter) or directly from localized vanilla Workshop text, builds
//! the semantic service, and serves JSON requests over stdin/stdout, one
//! request per line.
//!
//! Usage:
//! ```sh
//! wright-tool --program program.json
//! wright-tool --workshop workshop.txt [--locale en-US]
//! ```
//! Requests use the `Request` JSON schema in `wright_analyzer::service`; every
//! response is a single JSON line. The interface is read-only: no mutation or
//! AST-editing operation exists in v0.2. Without `--locale`, Workshop input
//! is auto-detected and must be unambiguous.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use wright_analyzer::service::SemanticService;
use wright_ir::lower;
use wright_workshop::catalog::{Catalog, Locale};
use wright_workshop::{detect, parser};

enum Input {
    Protocol(wright_core::hir::Program),
    Workshop(workshop_rs::wir::Program, String),
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut program_path: Option<String> = None;
    let mut workshop_path: Option<String> = None;
    let mut locale: Option<String> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--program" => program_path = args.next(),
            "--workshop" => workshop_path = args.next(),
            "--locale" => locale = args.next(),
            other => {
                eprintln!("wright-tool: unknown argument '{other}'");
                eprintln!(
                    "usage: wright-tool <--program file.json | --workshop file.txt [--locale LOC]>"
                );
                return ExitCode::from(2);
            }
        }
    }
    let input = match (program_path, workshop_path) {
        (Some(_), Some(_)) => {
            eprintln!("wright-tool: provide only one of --program or --workshop");
            return ExitCode::from(2);
        }
        (Some(path), None) => match load_protocol(&path) {
            Ok(input) => input,
            Err(message) => {
                eprintln!("wright-tool: {message}");
                return ExitCode::from(2);
            }
        },
        (None, Some(path)) => match load_workshop(&path, locale.as_deref()) {
            Ok(input) => input,
            Err(message) => {
                eprintln!("wright-tool: {message}");
                return ExitCode::from(2);
            }
        },
        (None, None) => {
            eprintln!("wright-tool: --program or --workshop is required");
            return ExitCode::from(2);
        }
    };

    let (program, origin) = match input {
        Input::Protocol(protocol) => {
            let model = match protocol.to_ir() {
                Ok(model) => model,
                Err(error) => {
                    eprintln!("wright-tool: cannot load program: {error}");
                    return ExitCode::from(2);
                }
            };
            let program = match lower::lower(&model) {
                Ok(program) => program,
                Err(error) => {
                    eprintln!("wright-tool: cannot load program: {error}");
                    return ExitCode::from(2);
                }
            };
            (
                program,
                wright_analyzer::service::Origin {
                    kind: "protocol".to_string(),
                    locale: None,
                },
            )
        }
        Input::Workshop(program, locale) => {
            let origin = wright_analyzer::service::Origin {
                kind: "workshop".to_string(),
                locale: Some(locale.clone()),
            };
            (program, origin)
        }
    };
    let service = match SemanticService::with_origin(&program, origin) {
        Ok(service) => service,
        Err(error) => {
            eprintln!("wright-tool: cannot load program: {error}");
            return ExitCode::from(2);
        }
    };

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let response = service.handle_json(&line);
        if writeln!(out, "{response}").is_err() {
            break;
        }
    }
    ExitCode::SUCCESS
}

fn load_protocol(program_path: &str) -> Result<Input, String> {
    let program_json = std::fs::read_to_string(program_path)
        .map_err(|error| format!("cannot read {program_path}: {error}"))?;
    let protocol = wright_core::hir::parse_str(&program_json)
        .map_err(|error| format!("cannot load program: {error}"))?;
    Ok(Input::Protocol(protocol))
}

fn load_workshop(workshop_path: &str, override_locale: Option<&str>) -> Result<Input, String> {
    let text = std::fs::read_to_string(workshop_path)
        .map_err(|error| format!("cannot read {workshop_path}: {error}"))?;
    let catalog = Catalog::builtin().map_err(|error| error.to_string())?;
    let override_locale = override_locale.map(Locale::new);
    let locale = detect::resolve_locale(&text, &catalog, override_locale.as_ref())
        .map_err(|error| format!("cannot detect language: {error}"))?;
    let program = parser::parse(&text, &catalog, &locale)
        .map_err(|error| format!("cannot parse Workshop text: {error}"))?;
    Ok(Input::Workshop(program, locale.to_string()))
}
