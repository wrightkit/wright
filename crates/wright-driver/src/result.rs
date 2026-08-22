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
    /// The command name (`compile`, `check`, `analyze`, `inspect`, `lint`).
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
///   stdin, when the explicit fallback is requested; reconstruction
///   rejections from the language-owned reconstructors, stage
///   `reconstruction`);
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
        if diagnostic.stage == crate::diag::Stage::Reconstruction {
            return exit::UNSUPPORTED;
        }
        if diagnostic.code == "adapter-stdin-unsupported" {
            return exit::UNSUPPORTED;
        }
        if diagnostic.code == "ostw-unsupported" {
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
#[derive(Debug, Clone, Default, Serialize)]
pub struct CheckResult {
    /// The OSTW project outcome summary, present only for `.ostw`/`.del`
    /// inputs (#117). Syntax/project infrastructure only: no semantic or
    /// emission claim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ostw: Option<OstwProjectSummary>,
}

/// The OSTW frontend/project outcome reported by `check`.
#[derive(Debug, Clone, Serialize)]
pub struct OstwProjectSummary {
    /// The `ds.toml` `entry_point` value.
    pub entry: String,
    /// The compilation graph: `ds.toml` (id 0) then the entry-point
    /// import-reachable closure.
    pub files: Vec<OstwFileSummary>,
    /// The independent workspace/source inventory (every `.ostw`/`.del`
    /// under the root) — tooling only, never compilation membership.
    pub inventory: Vec<String>,
}

/// One project file's parse outcome.
#[derive(Debug, Clone, Serialize)]
pub struct OstwFileSummary {
    /// The project-relative path.
    pub path: String,
    /// The registry id used by span provenance.
    pub id: u32,
    /// Whether this is a source file (`.ostw`/`.del`); `false` for `ds.toml`.
    pub source: bool,
    /// Whether the source file parsed cleanly.
    pub parsed: bool,
    /// Resolved in-closure import targets (project-relative paths).
    pub imports: Vec<String>,
}

/// The result of an `analyze` run: program summary and semantic facts.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AnalyzeResult {
    pub program: serde_json::Value,
    /// Deterministic semantic facts derived from the program structure,
    /// symbol index, and control-flow graphs. This is intentionally separate
    /// from the lint registry and its configurable findings.
    pub facts: serde_json::Value,
}

/// The result of an `inspect` run: the structural/semantic program model.
#[derive(Debug, Clone, Default, Serialize)]
pub struct InspectResult {
    pub program: serde_json::Value,
    pub rules: serde_json::Value,
    pub symbols: serde_json::Value,
    pub references: serde_json::Value,
}

/// The result of a `lint` run (#98): source identity, the program
/// summary, the registered lint rules with their effective configuration,
/// the active configuration, and the findings.
#[derive(Debug, Clone, Default, Serialize)]
pub struct LintResult {
    /// SHA-256 of the input bytes (source identity).
    pub input_identity: String,
    /// Program summary (from the semantic service `program` request).
    pub program: serde_json::Value,
    /// Registered lint rules with default/effective severity, enabled state,
    /// and evidence class (from the `lintRules` request).
    pub rules: serde_json::Value,
    /// The effective lint configuration (from the `lintRules` request).
    pub config: serde_json::Value,
    /// Lint findings, each carrying a stable code, severity, evidence class,
    /// message, and source span (from the `getFindings` request).
    pub findings: serde_json::Value,
}

/// The reconstruction target of a `convert` run (#126): which language-owned
/// reconstructor turns the validated Workshop program back into source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConvertTarget {
    /// Reconstruct canonical OPY source (`wright_opy::reconstruct`).
    #[default]
    Opy,
    /// Reconstruct canonical OSTW source (`wright_ostw::reconstruct`).
    Ostw,
}

impl ConvertTarget {
    /// The canonical name used in CLI arguments, docs, and diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            ConvertTarget::Opy => "opy",
            ConvertTarget::Ostw => "ostw",
        }
    }

    /// Parse a CLI spelling into a conversion target (`None` for unknown
    /// names).
    pub fn parse(name: &str) -> Option<ConvertTarget> {
        Some(match name {
            "opy" => ConvertTarget::Opy,
            "ostw" => ConvertTarget::Ostw,
            _ => return None,
        })
    }
}

/// The result of a `convert` run: the canonical reconstructed source for the
/// selected target plus its deterministic SHA-256.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConvertResult {
    /// The reconstruction target (`opy` or `ostw`).
    pub target: ConvertTarget,
    /// The canonical reconstructed source for the target. Empty when the
    /// reconstruction rejected: a rejection never carries partial source.
    pub text: String,
    /// SHA-256 of the reconstructed source.
    pub sha256: String,
}

/// Build the envelope metadata block for a command.
pub fn version_info() -> VersionInfo {
    VersionInfo {
        version: DRIVER_VERSION.to_string(),
        contract: RESULT_CONTRACT.to_string(),
    }
}
