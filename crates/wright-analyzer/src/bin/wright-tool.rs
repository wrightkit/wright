//! `wright-tool` — the read-only agent/tool interface over Wright's semantic
//! services.
//!
//! Loads a compiled program (Opy HIR v1 protocol JSON, as produced by the
//! adapter) through the full pipeline into Workshop IR, builds the semantic
//! service, and serves JSON requests over stdin/stdout, one request per line.
//!
//! Usage:
//! ```sh
//! wright-tool --program program.json
//! ```
//! Requests use the `Request` JSON schema in `wright_analyzer::service`; every
//! response is a single JSON line. The interface is read-only: no mutation or
//! AST-editing operation exists in v0.2.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use wright_analyzer::service::SemanticService;
use wright_core::hir;
use wright_ir::lower;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let program_path = match (args.next().as_deref(), args.next()) {
        (Some("--program"), Some(path)) => path,
        (Some(other), _) => {
            eprintln!("wright-tool: unknown argument '{other}'");
            eprintln!("usage: wright-tool --program <program.json>");
            return ExitCode::from(2);
        }
        _ => {
            eprintln!("wright-tool: --program is required");
            eprintln!("usage: wright-tool --program <program.json>");
            return ExitCode::from(2);
        }
    };

    let program_json = match std::fs::read_to_string(&program_path) {
        Ok(content) => content,
        Err(error) => {
            eprintln!("wright-tool: cannot read {program_path}: {error}");
            return ExitCode::from(2);
        }
    };
    let protocol = match hir::parse_str(&program_json) {
        Ok(protocol) => protocol,
        Err(error) => {
            eprintln!("wright-tool: cannot load program: {error}");
            return ExitCode::from(2);
        }
    };
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
    let service = match SemanticService::new(&program) {
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
