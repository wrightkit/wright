//! Command result envelopes and exit code conventions.

use crate::diag::{Diagnostic, Severity};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct VersionInfo {
    pub version: String,
    pub contract: String,
}

pub const RESULT_CONTRACT: &str = "wright-result/v1";
pub const DRIVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod exit {
    pub const SUCCESS: u8 = 0;
    pub const SOURCE_ERROR: u8 = 1;
    pub const USAGE: u8 = 2;
    pub const UNSUPPORTED: u8 = 3;
    pub const INTERNAL: u8 = 4;
}

#[derive(Debug, Clone, Serialize)]
pub struct Envelope<T: Serialize> {
    pub wright: VersionInfo,
    pub command: String,
    pub ok: bool,
    pub exit: u8,
    pub diagnostics: Vec<Diagnostic>,
    pub result: T,
}

pub fn exit_code_from(diagnostics: &[Diagnostic]) -> u8 {
    let mut has_source_error = false;
    for d in diagnostics {
        if d.severity != Severity::Error {
            continue;
        }
        if d.stage == crate::diag::Stage::Internal {
            return exit::INTERNAL;
        }
        if d.stage == crate::diag::Stage::Reconstruction
            || d.code == "adapter-stdin-unsupported"
            || d.code == "source-provider-unsupported"
        {
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

#[derive(Debug, Clone, Serialize)]
pub struct CompiledOutput {
    pub text: String,
    pub sha256: String,
    pub locale: String,
    pub written_to: String,
    pub input_identity: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CompileResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<CompiledOutput>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CheckResult {}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AnalyzeResult {
    pub program: serde_json::Value,
    pub facts: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct InspectResult {
    pub program: serde_json::Value,
    pub rules: serde_json::Value,
    pub symbols: serde_json::Value,
    pub references: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LintResult {
    pub input_identity: String,
    pub program: serde_json::Value,
    pub rules: serde_json::Value,
    pub config: serde_json::Value,
    pub findings: serde_json::Value,
    pub skipped: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConvertTarget {
    #[default]
    Opy,
    Ostw,
}

impl ConvertTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Opy => "opy",
            Self::Ostw => "ostw",
        }
    }

    pub fn parse(name: &str) -> Option<ConvertTarget> {
        match name {
            "opy" => Some(Self::Opy),
            "ostw" => Some(Self::Ostw),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ConvertResult {
    pub target: ConvertTarget,
    pub text: String,
    pub sha256: String,
}

pub fn version_info() -> VersionInfo {
    VersionInfo {
        version: DRIVER_VERSION.to_string(),
        contract: RESULT_CONTRACT.to_string(),
    }
}
