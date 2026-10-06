use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageInfo {
    pub id: String,
    pub extensions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextEdit {
    pub range: Range,
    #[serde(rename = "newText")]
    pub new_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub uri: String,
    #[serde(rename = "languageId")]
    pub language_id: String,
    pub version: i64,
    pub text: String,
}

pub type DocumentSet = BTreeMap<String, Document>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub uri: String,
    #[serde(rename = "languageId")]
    pub language_id: String,
    pub version: i64,
    #[serde(default, skip_serializing_if = "project_target_is_file")]
    pub kind: ProjectTargetKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProjectTargetKind {
    #[default]
    File,
    Directory,
}

fn project_target_is_file(kind: &ProjectTargetKind) -> bool {
    matches!(kind, ProjectTargetKind::File)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: DiagnosticSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub range: Range,
    #[serde(
        rename = "selectionRange",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub selection_range: Option<Range>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkshopArtifact {
    pub format: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    #[serde(rename = "serverInfo")]
    pub server_info: ServerInfo,
    pub languages: Vec<LanguageInfo>,
    pub capabilities: Capabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentDiagnostics {
    pub uri: String,
    pub version: i64,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    pub documents: Vec<DocumentDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileResult {
    pub diagnostics: Vec<DocumentDiagnostics>,
    #[serde(
        rename = "sourceIdentity",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub source_identity: Option<String>,
    pub artifact: Option<WorkshopArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconstructResult {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentSymbols {
    pub uri: String,
    pub version: i64,
    pub symbols: Vec<Symbol>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolsResult {
    pub documents: Vec<DocumentSymbols>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationsResult {
    pub locations: Vec<Location>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentEdits {
    #[serde(rename = "documentUri")]
    pub document_uri: String,
    pub version: i64,
    #[serde(rename = "textEdits")]
    pub text_edits: Vec<TextEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameResult {
    pub edits: Vec<DocumentEdits>,
}

/// `lpp/lookup` params (LPP 1.5 `name-lookup.md` §21.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupParams {
    pub language_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub within: Option<LookupWithin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
}

/// One `within` selector: `{ "kind": "...", "value": "..." }` — the closed
/// kind set is `callable`, `enum`, `settings` (§21.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LookupWithin {
    pub kind: String,
    pub value: String,
}

/// `lpp/lookup` result (§21.2): the provider's ranked entries in
/// provider-defined shapes. The client stays language-neutral here; entry
/// fields are facts owned by the provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LookupResult {
    pub entries: Vec<serde_json::Value>,
}

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
    Lookup,
}

impl Capability {
    pub const ALL: [Capability; 10] = [
        Capability::Check,
        Capability::Compile,
        Capability::ProjectLoading,
        Capability::Reconstruct,
        Capability::Symbols,
        Capability::Definition,
        Capability::References,
        Capability::Rename,
        Capability::EditValidation,
        Capability::Lookup,
    ];

    fn metadata(self) -> (&'static str, &'static str) {
        match self {
            Self::Check => ("check", "lpp/check"),
            Self::Compile => ("compile", "lpp/compile"),
            Self::ProjectLoading => ("projectLoading", "lpp/check"),
            Self::Reconstruct => ("reconstruct", "lpp/reconstruct"),
            Self::Symbols => ("symbols", "lpp/symbols"),
            Self::Definition => ("definition", "lpp/definition"),
            Self::References => ("references", "lpp/references"),
            Self::Rename => ("rename", "lpp/rename"),
            Self::EditValidation => ("editValidation", "lpp/validateEdits"),
            Self::Lookup => ("lookup", "lpp/lookup"),
        }
    }

    pub fn as_str(self) -> &'static str {
        self.metadata().0
    }

    pub fn method(self) -> &'static str {
        self.metadata().1
    }

    pub fn parse(name: &str) -> Option<Capability> {
        Self::ALL
            .into_iter()
            .find(|capability| capability.as_str() == name)
    }
}

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
    /// `lpp/lookup` (LPP 1.5). Absent in pre-1.5 sessions.
    #[serde(default)]
    pub lookup: bool,
}

impl Capabilities {
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
            Capability::Lookup => self.lookup,
        }
    }

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

    pub fn supported(&self) -> Vec<Capability> {
        Capability::ALL
            .into_iter()
            .filter(|c| self.supports(*c))
            .collect()
    }
}
