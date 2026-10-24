#![allow(clippy::result_large_err)]

pub mod cache;
pub mod config;
pub mod diag;
pub mod edit;
pub mod essential;
pub mod fix;
pub mod input;
#[cfg(not(target_arch = "wasm32"))]
pub mod lint_package;
pub(crate) mod list_entries;
pub(crate) mod lookup;
#[cfg(not(target_arch = "wasm32"))]
pub mod opy_provider;
pub mod output;
pub mod progress;
#[cfg(not(target_arch = "wasm32"))]
pub mod provider_edit;
pub(crate) mod request;
pub mod result;
pub mod select;
pub mod service;
pub mod session;
pub mod source_provider;
pub(crate) mod statement_edit;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod store;
pub mod workshop_compat;

pub use config::{
    InputSpec, LintConfig, LintPackageConfig, LintPackagePin, OutputFormat, RunOptions,
    SessionConfig, SourceKind,
};
pub use diag::{Diagnostic, Origin, Position, Severity, SourceSpan, Stage, Status};
pub use input::{InputTarget, ResolvedInput, sha256_hex};
#[cfg(not(target_arch = "wasm32"))]
pub use lint_package::{
    InstalledPackage, LintPackageError, LintPackageSource, LintPackageStore, PackageSearch,
    PackageSearchHit, RemoteCheck, RemovedPackage, StoreEntry,
};
#[cfg(not(target_arch = "wasm32"))]
pub use opy_provider::{
    OpyProviderConfig, OpyProviderError, OpyProviderResolver, ResolvedOpyProvider,
};
pub use output::{CompileWrite, OutputDest, Overwrite};
pub use progress::{ProgressEvent, ProgressObserver, ProgressPhase, ProgressUnit};
pub use result::{
    AnalyzeResult, CheckResult, CompileResult, CompiledOutput, ConvertResult, ConvertTarget,
    Envelope, EssentialResult, InspectResult, LintPackageResult, LintPackageRow, LintResult,
    RESULT_CONTRACT, SimulateResult,
};
pub use select::{FindingSelection, SelectionOutcome};
pub use session::{CompilerSession, Loaded, Provenance, WORKSHOP_CLIENT_ELEMENT_LIMIT};
pub use source_provider::{
    SourceBackend, SourceCompilation, SourceLanguage, SourceProvenance, SourceProvider,
    SourceProviderError, SourceTarget, SourceTargetKind,
};
pub use workshop_compat::{
    WorkshopSemanticComparison, WorkshopSemanticSide, compare_workshop_texts,
};
pub use wright_transform::Profile;

/// The driver crate name reported in result metadata.
pub const DRIVER_NAME: &str = "wright-driver";

/// The stable embedding-API contract name (#56).
pub const EMBEDDING_CONTRACT: &str = "wright-embedding/v1";

/// A deterministic SHA-256 identity for an input or artifact.
pub fn input_identity(text: &str) -> String {
    sha256_hex(text.as_bytes())
}
