//! LPP v1 data types used by the client.
//!
//! These types mirror the wire shapes defined in the `language-provider-
//! protocol` repository's LPP v1 specification (sections 6-17). The wire
//! contract is normative there; this module is the client-side Rust view of
//! it and adds no protocol schema of its own. All positions and ranges use
//! LSP conventions: 0-based lines and 0-based UTF-16 code-unit characters.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The client identity reported during initialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

/// The provider identity returned during initialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// One language a provider serves, as declared during initialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageInfo {
    pub id: String,
    /// File extensions associated with the language, without a leading dot,
    /// lowercase. May be empty.
    pub extensions: Vec<String>,
}

/// A 0-based position (LSP conventions; `character` is a UTF-16 code-unit
/// offset within the line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

/// A half-open range: `start` is inclusive, `end` is exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/// A source edit replacing `range` with `new_text`, expressed in the
/// coordinates of the original document text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextEdit {
    pub range: Range,
    #[serde(rename = "newText")]
    pub new_text: String,
}

/// One unit of source text, tagged with its URI, language id, and version.
///
/// LPP is stateless: the text always travels with the request, and the
/// version lets both sides tag results and detect stale bookkeeping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub uri: String,
    #[serde(rename = "languageId")]
    pub language_id: String,
    /// A non-negative integer that MUST increase by at least 1 whenever the
    /// client changes the document text. Providers validate this.
    pub version: i64,
    pub text: String,
}

/// A set of documents keyed by URI, supplied with a request.
pub type DocumentSet = BTreeMap<String, Document>;

/// A client-selected entry for provider-owned filesystem project loading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub uri: String,
    #[serde(rename = "languageId")]
    pub language_id: String,
    pub version: i64,
}

/// The severity of a provider diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

/// A provider diagnostic. Diagnostics within one document are sorted by
/// `range.start` (line, then character) by the provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: DiagnosticSeverity,
    /// Provider-defined diagnostic code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    /// String naming the diagnostic origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// A location in a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

/// A declared symbol. `selection_range` defaults to `range` when absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    /// Provider-defined kind (LPP defines no fixed kind vocabulary).
    pub kind: String,
    pub range: Range,
    #[serde(
        rename = "selectionRange",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub selection_range: Option<Range>,
}

/// The opaque compile envelope: a format id plus a string payload the
/// protocol never interprets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkshopArtifact {
    pub format: String,
    pub content: String,
}

/// The `lpp/initialize` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    #[serde(rename = "serverInfo")]
    pub server_info: ServerInfo,
    pub languages: Vec<LanguageInfo>,
    pub capabilities: Capabilities,
}

/// Diagnostics for one document, tagged with the version they were computed
/// for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentDiagnostics {
    pub uri: String,
    pub version: i64,
    pub diagnostics: Vec<Diagnostic>,
}

/// The `lpp/check` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    pub documents: Vec<DocumentDiagnostics>,
}

/// The `lpp/compile` result: per-document diagnostics plus the opaque
/// artifact (null whenever an error-severity diagnostic was reported).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileResult {
    pub diagnostics: Vec<DocumentDiagnostics>,
    pub artifact: Option<WorkshopArtifact>,
}

/// The `lpp/reconstruct` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconstructResult {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

/// Symbols for one document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentSymbols {
    pub uri: String,
    pub version: i64,
    pub symbols: Vec<Symbol>,
}

/// The `lpp/symbols` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolsResult {
    pub documents: Vec<DocumentSymbols>,
}

/// The `lpp/definition` / `lpp/references` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationsResult {
    pub locations: Vec<Location>,
}

/// The edits produced for one document by `lpp/rename`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentEdits {
    #[serde(rename = "documentUri")]
    pub document_uri: String,
    pub version: i64,
    #[serde(rename = "textEdits")]
    pub text_edits: Vec<TextEdit>,
}

/// The `lpp/rename` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameResult {
    pub edits: Vec<DocumentEdits>,
}

/// The `lpp/validateEdits` result. `reason` is present iff `valid` is false;
/// `failing_edit_index` is present for `overlappingEdits` and
/// `rangeOutOfBounds` only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidateEditsResult {
    pub valid: bool,
    pub version: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(
        rename = "failingEditIndex",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub failing_edit_index: Option<u32>,
}

/// An LPP v1 capability id. The capability ids are a closed set in LPP v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Check,
    Compile,
    ProjectLoading,
    Reconstruct,
    Symbols,
    Definition,
    References,
    Rename,
    EditValidation,
}

impl Capability {
    /// Every LPP v1 capability.
    pub const ALL: [Capability; 9] = [
        Capability::Check,
        Capability::Compile,
        Capability::ProjectLoading,
        Capability::Reconstruct,
        Capability::Symbols,
        Capability::Definition,
        Capability::References,
        Capability::Rename,
        Capability::EditValidation,
    ];

    /// The wire capability id.
    pub fn as_str(self) -> &'static str {
        match self {
            Capability::Check => "check",
            Capability::Compile => "compile",
            Capability::ProjectLoading => "projectLoading",
            Capability::Reconstruct => "reconstruct",
            Capability::Symbols => "symbols",
            Capability::Definition => "definition",
            Capability::References => "references",
            Capability::Rename => "rename",
            Capability::EditValidation => "editValidation",
        }
    }

    /// The LPP v1 method governed by this capability.
    pub fn method(self) -> &'static str {
        match self {
            Capability::Check => "lpp/check",
            Capability::Compile => "lpp/compile",
            Capability::ProjectLoading => "lpp/check",
            Capability::Reconstruct => "lpp/reconstruct",
            Capability::Symbols => "lpp/symbols",
            Capability::Definition => "lpp/definition",
            Capability::References => "lpp/references",
            Capability::Rename => "lpp/rename",
            Capability::EditValidation => "lpp/validateEdits",
        }
    }

    /// Parse a wire capability id (`None` for unknown ids; unknown capability
    /// ids can only appear through a new protocol version or an additive
    /// revision).
    pub fn parse(name: &str) -> Option<Capability> {
        Some(match name {
            "check" => Capability::Check,
            "compile" => Capability::Compile,
            "projectLoading" => Capability::ProjectLoading,
            "reconstruct" => Capability::Reconstruct,
            "symbols" => Capability::Symbols,
            "definition" => Capability::Definition,
            "references" => Capability::References,
            "rename" => Capability::Rename,
            "editValidation" => Capability::EditValidation,
            _ => return None,
        })
    }
}

/// The negotiated capability set advertised by a provider during
/// initialization. The LPP 1.0 fields are required; `projectLoading` is the
/// additive LPP 1.1 capability and defaults to false for 1.0 providers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub check: bool,
    pub compile: bool,
    #[serde(rename = "projectLoading", default)]
    pub project_loading: bool,
    pub reconstruct: bool,
    pub symbols: bool,
    pub definition: bool,
    pub references: bool,
    pub rename: bool,
    #[serde(rename = "editValidation")]
    pub edit_validation: bool,
}

impl Capabilities {
    /// Whether the capability was negotiated.
    pub fn supports(&self, capability: Capability) -> bool {
        match capability {
            Capability::Check => self.check,
            Capability::Compile => self.compile,
            Capability::ProjectLoading => self.project_loading,
            Capability::Reconstruct => self.reconstruct,
            Capability::Symbols => self.symbols,
            Capability::Definition => self.definition,
            Capability::References => self.references,
            Capability::Rename => self.rename,
            Capability::EditValidation => self.edit_validation,
        }
    }

    /// Require a capability, refusing explicitly when it was not negotiated.
    ///
    /// This is the client-side half of the no-silent-fallback contract: a
    /// method whose capability is unavailable is never sent to the provider;
    /// the caller gets the same structured `capabilityUnavailable` refusal
    /// the provider would produce on the wire.
    pub fn require(&self, capability: Capability) -> Result<(), crate::error::ProviderError> {
        if self.supports(capability) {
            Ok(())
        } else {
            Err(crate::error::ProviderError::lpp(
                crate::error::LppErrorKind::CapabilityUnavailable,
                serde_json::json!({
                    "capability": capability.as_str(),
                    "method": capability.method(),
                }),
                format!(
                    "capability '{}' is not available in this session",
                    capability.as_str()
                ),
            ))
        }
    }

    /// The negotiated capabilities, in the closed LPP v1 capability order.
    pub fn supported(&self) -> Vec<Capability> {
        Capability::ALL
            .into_iter()
            .filter(|capability| self.supports(*capability))
            .collect()
    }
}
