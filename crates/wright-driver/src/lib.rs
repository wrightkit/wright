//! Wright's reusable compiler/session driver (milestone M6).
//!
//! One orchestration path for every frontend and workflow: input discovery →
//! frontend selection (`opy` bridge, native Workshop, or protocol JSON) →
//! validation → lowering → analysis → emission. The `wright` CLI is a thin
//! presentation layer over this crate, and later tool/LSP adapters reuse the
//! same [`CompilerSession`]. Every workflow returns a typed [`Envelope`] whose
//! JSON serialization is the machine-readable CLI contract.

// Diagnostics are the primary error type of this crate, so error-returning
// functions legitimately carry the full `Diagnostic` value; boxing it would
// add an allocation per error without a measured benefit.
#![allow(clippy::result_large_err)]

pub mod config;
pub mod diag;
pub mod input;
pub mod opy;
pub mod result;
pub mod session;

pub use config::{InputSpec, OutputFormat, SessionConfig, SourceKind};
pub use diag::{Diagnostic, Origin, Position, Severity, SourceSpan, Stage};
pub use input::{ResolvedInput, sha256_hex};
pub use result::{
    AnalyzeResult, CheckResult, CompileResult, CompiledOutput, Envelope, InspectResult,
    RESULT_CONTRACT,
};
pub use session::{CompilerSession, Loaded};

/// The driver crate name reported in result metadata.
pub const DRIVER_NAME: &str = "wright-driver";

/// A deterministic SHA-256 identity for an input or artifact.
pub fn input_identity(text: &str) -> String {
    sha256_hex(text.as_bytes())
}
