//! Typed result envelopes for every driver workflow.
//!
//! Each workflow returns a command-specific [`Envelope`] carrying the same
//! deterministic shape: version/capability metadata, the command name, a
//! boolean `ok`, the process exit code the CLI must use, the diagnostics, and
//! the typed command result. The CLI's JSON mode serializes this exact model,
//! so CI/agents distinguish success, source errors, unsupported input, and
//! internal failures without scraping human text.

use serde::Serialize;

use crate::diag::{Diagnostic, Severity};

/// Driver identity and result-contract version.
#[derive(Debug, Clone, Serialize)]
pub struct VersionInfo {
    /// The driver/CLI version.
    pub version: String,
    /// The machine-readable result contract name.
    pub contract: String,
}

/// The stable machine-readable result contract name.
pub const RESULT_CONTRACT: &str = "wright-result/v1";
/// The driver crate version.
pub const DRIVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The exit-code contract shared by every command.
pub mod exit {
    /// Success.
    pub const SUCCESS: u8 = 0;
    /// Source/user errors (parse, validation, ambiguous input).
    pub const SOURCE_ERROR: u8 = 1;
    /// CLI usage errors (unknown flags/subcommands).
    pub const USAGE: u8 = 2;
    /// Recognized but unsupported input or operation.
    pub const UNSUPPORTED: u8 = 3;
    /// Internal/environment failures (I/O, missing bridge, bugs).
    pub const INTERNAL: u8 = 4;
}

/// One command result envelope.
#[derive(Debug, Clone, Serialize)]
pub struct Envelope<T: Serialize> {
    pub wright: VersionInfo,
    /// The command name (`compile`, `check`, `analyze`, `inspect`).
    pub command: String,
    /// Whether the workflow produced a usable result.
    pub ok: bool,
    /// The process exit code the CLI must use for this result.
    pub exit: u8,
    pub diagnostics: Vec<Diagnostic>,
    pub result: T,
}

/// Derive the exit code from the diagnostics of a failed run.
///
/// * `4` — internal/environment failures (stage `internal`);
/// * `3` — recognized but unsupported input/operation (adapter-fallback
///   stdin, when the explicit fallback is requested);
/// * `1` — everything else that blocks the workflow (source errors).
pub fn exit_code_from(diagnostics: &[Diagnostic]) -> u8 {
    let mut has_source_error = false;
    for diagnostic in diagnostics {
        if diagnostic.severity != Severity::Error {
            continue;
        }
        if diagnostic.stage == crate::diag::Stage::Internal {
            return exit::INTERNAL;
        }
        if diagnostic.code == "adapter-stdin-unsupported" {
            return exit::UNSUPPORTED;
        }
        has_source_error = true;
    }
    if has_source_error {
        exit::SOURCE_ERROR
    } else {
        exit::SUCCESS
    }
}

/// The compiled artifact of a `compile` run.
#[derive(Debug, Clone, Serialize)]
pub struct CompiledOutput {
    /// The emitted Workshop text.
    pub text: String,
    /// SHA-256 of the emitted text.
    pub sha256: String,
    /// The emission locale.
    pub locale: String,
    /// Where the output was written: a path or `stdout`.
    pub written_to: String,
    /// SHA-256 of the input bytes.
    pub input_identity: String,
}

/// The result of a `compile` run.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CompileResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<CompiledOutput>,
}

/// The result of a `check` run (the envelope's `ok` and `diagnostics` carry
/// the verdict).
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct CheckResult {}

/// The result of an `analyze` run: program summary and semantic findings.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AnalyzeResult {
    pub program: serde_json::Value,
    pub findings: serde_json::Value,
}

/// The result of an `inspect` run: the structural/semantic program model.
#[derive(Debug, Clone, Default, Serialize)]
pub struct InspectResult {
    pub program: serde_json::Value,
    pub rules: serde_json::Value,
    pub symbols: serde_json::Value,
    pub references: serde_json::Value,
}

/// Build the envelope metadata block for a command.
pub fn version_info() -> VersionInfo {
    VersionInfo {
        version: DRIVER_VERSION.to_string(),
        contract: RESULT_CONTRACT.to_string(),
    }
}
