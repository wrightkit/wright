use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Serialize;
use wright_analyzer::canonical::{PositionResolution, ReferenceKind, SemanticIndex, Symbol};
use wright_driver::opy_provider::OPY_LANGUAGE_ID;
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind};
use wright_lpp::{Capability, LanguageProvider};

use crate::document::{Document, DocumentStore, Position, Range, line_col_position, uri_to_path};

#[derive(Debug, Clone, Serialize)]
pub struct SourceDiagnostic {
    pub source: String,
    pub range: Range,
    pub severity: String,
    pub code: String,
    pub message: String,
    pub source_version: i32,
    pub document_version: i32,
}

/// One validated text replacement produced by a provider-owned edit, in the
/// document store's UTF-16 coordinates. `version` is the document version
/// the provider computed and Wright re-validated the edit against; the
/// range is only meaningful to a buffer at that version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTextEdit {
    pub uri: String,
    pub range: Range,
    pub new_text: String,
    pub version: i32,
}

/// The outcome of a provider-owned source mutation (#156): validated edits
/// the caller applies, or an explicit structured refusal — unsupported
/// documents, an unconfigured provider, a provider refusal, and validation
/// failures all land in the same channel rather than a textual fallback.
#[derive(Debug)]
pub enum RenameOutcome {
    Applied(Vec<SourceTextEdit>),
    Refused { code: String, message: String },
}

/// A resolved source location — a `definition` or `references` target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceLocation {
    pub uri: String,
    pub range: Range,
}

/// A program-scope symbol the provider declared via `lpp/symbols`
/// (#738): the provider's own kind vocabulary (`global`, `player`,
/// `subroutine`, `def`, `constant`, `macro`) with UTF-16 ranges over the
/// buffer text it was sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceSymbol {
    pub name: String,
    pub kind: String,
    pub range: Range,
    pub selection_range: Option<Range>,
}

/// The OPY capabilities negotiated with the provider for this session
/// (#738). The host consults it while building the `initialize` answer —
/// a capability is advertised only when the provider actually serves it —
/// and the service consults it again per request, so a capability the
/// provider never negotiated degrades to no answer rather than an error.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OpyProviderSupport {
    /// `lpp/check` — provider diagnostics for open OPY buffers.
    pub check: bool,
    /// `lpp/symbols` — document symbols.
    pub symbols: bool,
    /// `lpp/definition` — go-to-definition.
    pub definition: bool,
    /// `lpp/references` — find references.
    pub references: bool,
}

/// The hover answer for a position: the resolved symbol's identity over the
/// identifier occurrence that matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HoverInfo {
    pub contents: String,
    pub range: Range,
}

enum OpyProviderState {
    Pending,
    Ready(Box<dyn LanguageProvider>),
    // The `(code, message)` a connect failure reports — provider error
    // classified, or a session-construction diagnostic verbatim (#1004).
    Unavailable(String, String),
}

pub struct LanguageService {
    pub store: DocumentStore,
    pub root: PathBuf,
    config: SessionConfig,
    session: Option<CompilerSession>,
    opy: OpyProviderState,
}

impl LanguageService {
    pub fn new(root: PathBuf) -> LanguageService {
        Self::with_config(root, SessionConfig::default())
    }

    /// A service whose session behavior is explicitly configured — hosts
    /// register providers or point the OPY provider resolver at a concrete
    /// executable through `config`. The session itself is constructed lazily
    /// on the first provider request.
    pub fn with_config(root: PathBuf, config: SessionConfig) -> LanguageService {
        LanguageService {
            store: DocumentStore::new(root.clone()),
            root,
            config,
            session: None,
            opy: OpyProviderState::Pending,
        }
    }

    /// The OPY capabilities negotiated with the provider (#738).
    ///
    /// Calling this connects the provider session if it has not run yet —
    /// spawn plus `lpp/initialize` — so the LSP `initialize` answer can
    /// advertise exactly what the provider serves. Without a resolvable
    /// provider the answer is "supports nothing" rather than a failure:
    /// the LSP session itself must always initialize.
    pub fn opy_provider_support(&mut self) -> OpyProviderSupport {
        let Ok(provider) = self.opy_provider() else {
            return OpyProviderSupport::default();
        };
        let Ok(capabilities) = provider.capabilities() else {
            return OpyProviderSupport::default();
        };
        OpyProviderSupport {
            check: capabilities.supports(Capability::Check),
            symbols: capabilities.supports(Capability::Symbols),
            definition: capabilities.supports(Capability::Definition),
            references: capabilities.supports(Capability::References),
        }
    }

    /// Diagnostics for the open buffer at `uri`.
    ///
    /// Raw Workshop buffers run the session's check pipeline over the
    /// buffer text (#555). OPY buffers are provider-owned (#738):
    /// `lpp/check` over the complete open-document overlay — unsaved text
    /// included — never a disk read. DEL/OSTW stay at the structured
    /// provider boundary until a provider serves them.
    pub fn diagnostics(&mut self, uri: &str) -> Vec<SourceDiagnostic> {
        let Some(language_id) = source_language_id(uri) else {
            return self.workshop_diagnostics(uri);
        };
        let Some(document) = self.store.document(uri) else {
            return Vec::new();
        };
        if language_id == OPY_LANGUAGE_ID {
            return self.opy_diagnostics(uri);
        }
        vec![SourceDiagnostic {
            source: document.uri.clone(),
            range: EMPTY_RANGE,
            severity: "error".to_string(),
            code: "source-provider-unavailable".to_string(),
            message:
                "source-language analysis is provider-owned; diagnostics are not yet negotiated"
                    .to_string(),
            source_version: document.version,
            document_version: document.version,
        }]
    }

    fn workshop_diagnostics(&self, uri: &str) -> Vec<SourceDiagnostic> {
        let Some(document) = self.store.document(uri) else {
            return Vec::new();
        };
        if !is_workshop_document(document) {
            return Vec::new();
        }
        match self.workshop_session(document) {
            Ok(mut session) => session
                .check()
                .diagnostics
                .iter()
                .map(|diagnostic| source_diagnostic(document, diagnostic))
                .collect(),
            Err(diagnostic) => vec![source_diagnostic(document, &diagnostic)],
        }
    }

    fn opy_diagnostics(&mut self, uri: &str) -> Vec<SourceDiagnostic> {
        let documents = self.source_document_set(OPY_LANGUAGE_ID);
        let root = self.project_root_uri();
        let version = self
            .store
            .document(uri)
            .map(|document| document.version)
            .unwrap_or(0);
        let boundary = |code: &str, message: String| {
            vec![SourceDiagnostic {
                source: uri.to_string(),
                range: EMPTY_RANGE,
                severity: "error".to_string(),
                code: code.to_string(),
                message,
                source_version: version,
                document_version: version,
            }]
        };
        let provider = match self.opy_provider() {
            Ok(provider) => provider,
            Err((code, message)) => return boundary(&code, message),
        };
        if !provider_supports(provider, Capability::Check) {
            return boundary(
                "source-provider-unavailable",
                "the negotiated OPY provider does not serve lpp/check".to_string(),
            );
        }
        match provider.check(&documents, root.as_deref()) {
            Ok(result) => result
                .documents
                .iter()
                .flat_map(|document| {
                    let version = self
                        .store
                        .document(&document.uri)
                        .map(|document| document.version)
                        .unwrap_or_else(|| i32::try_from(document.version).unwrap_or(i32::MAX));
                    document
                        .diagnostics
                        .iter()
                        .map(move |diagnostic| SourceDiagnostic {
                            source: document.uri.clone(),
                            range: diagnostic.range,
                            severity: lpp_severity(diagnostic.severity).to_string(),
                            code: diagnostic.code.clone().unwrap_or_default(),
                            message: diagnostic.message.clone(),
                            source_version: version,
                            document_version: version,
                        })
                })
                .collect(),
            // The shared provider-failure classification (#1004): a
            // refusal reports `provider-refusal-<refusalCode>` — the
            // driver contract — never the raw refusal code.
            Err(error) => boundary(
                &wright_driver::source_provider::provider_failure_code(&error),
                error.to_string(),
            ),
        }
    }

    /// Hover over an identifier occurrence in a raw Workshop document
    /// (#555): the resolved symbol's `kind name` — the identity `inspect`
    /// reports — over the occurrence that matched. Anything that is not an
    /// identifier occurrence, or a document that fails to load, is `None`.
    pub fn hover(&self, uri: &str, position: Position) -> Option<HoverInfo> {
        let document = self.workshop_document(uri)?;
        let index = self.workshop_index(document)?;
        let (symbol, occurrence) = symbol_at(&index, document, position)?;
        Some(HoverInfo {
            contents: format!("`{} {}`", symbol.kind.as_str(), symbol.name),
            range: document.from_span(&occurrence),
        })
    }

    /// The program-scope symbols an `.opy` buffer declares (#738):
    /// `lpp/symbols` over the open-document overlay, picking this
    /// document's entries. `None` — the host's `null` answer — when the
    /// document is not OPY, no provider negotiated, the provider does not
    /// serve `lpp/symbols`, or the provider refuses.
    pub fn document_symbols(&mut self, uri: &str) -> Option<Vec<SourceSymbol>> {
        let document = self.opy_document(uri)?;
        let documents = self.source_document_set(OPY_LANGUAGE_ID);
        let root = self.project_root_uri();
        let provider = self.opy_provider().ok()?;
        if !provider_supports(provider, Capability::Symbols) {
            return None;
        }
        let result = provider.symbols(&documents, root.as_deref()).ok()?;
        Some(
            result
                .documents
                .iter()
                .find(|entry| entry.uri == document.uri)
                .map(|entry| entry.symbols.iter().map(source_symbol).collect())
                .unwrap_or_default(),
        )
    }

    /// Go-to-definition over an identifier occurrence. In a raw Workshop
    /// document this is the resolved symbol's declared span — the location
    /// `inspect` reports (#555); in an `.opy` buffer it is the provider's
    /// `lpp/definition` answer, including targets in `#!include`d files
    /// the provider resolved itself (#738). `None` — the host's `null`
    /// answer — covers every unbacked case: no provider, an unnegotiated
    /// capability, or a provider refusal such as
    /// `definition.noSymbolAtPosition`.
    pub fn definition(&mut self, uri: &str, position: Position) -> Option<Vec<SourceLocation>> {
        if let Some(document) = self.opy_document(uri) {
            let provider = self.opy_provider().ok()?;
            if !provider_supports(provider, Capability::Definition) {
                return None;
            }
            return provider
                .definition(&document, position)
                .ok()
                .map(|result| result.locations.iter().map(source_location).collect());
        }
        let document = self.workshop_document(uri)?;
        let index = self.workshop_index(document)?;
        let (symbol, _) = symbol_at(&index, document, position)?;
        let span = symbol.span.or(symbol.occurrence)?;
        Some(vec![SourceLocation {
            uri: document.uri.clone(),
            range: document.from_span(&span),
        }])
    }

    /// References to the symbol an identifier occurrence resolves to —
    /// for a raw Workshop document the occurrence spans `inspect`
    /// reports (#555), for an `.opy` buffer the provider's
    /// `lpp/references` answer across the whole project including
    /// `#!include`d files (#738). `include_declaration` maps the LSP
    /// `includeDeclaration` flag. Unbacked requests are `None` — the
    /// host's `null` answer — never an error.
    pub fn references(
        &mut self,
        uri: &str,
        position: Position,
        include_declaration: bool,
    ) -> Option<Vec<SourceLocation>> {
        if let Some(document) = self.opy_document(uri) {
            let provider = self.opy_provider().ok()?;
            if !provider_supports(provider, Capability::References) {
                return None;
            }
            return provider
                .references(&document, position, include_declaration)
                .ok()
                .map(|result| result.locations.iter().map(source_location).collect());
        }
        let document = self.workshop_document(uri)?;
        let index = self.workshop_index(document)?;
        let (symbol, _) = symbol_at(&index, document, position)?;
        let mut seen = BTreeSet::new();
        Some(
            index
                .references(symbol.id)
                .into_iter()
                .filter(|reference| {
                    include_declaration || reference.kind != ReferenceKind::Declaration
                })
                .filter_map(|reference| reference.occurrence)
                .filter(|occurrence| {
                    seen.insert((
                        occurrence.file.index(),
                        occurrence.start.line,
                        occurrence.start.col,
                        occurrence.end.line,
                        occurrence.end.col,
                    ))
                })
                .map(|occurrence| SourceLocation {
                    uri: document.uri.clone(),
                    range: document.from_span(&occurrence),
                })
                .collect(),
        )
    }

    pub fn dependent_documents(&self, uri: &str) -> Vec<String> {
        if self.store.document(uri).is_some() {
            vec![uri.to_string()]
        } else {
            Vec::new()
        }
    }

    /// Rename the symbol at `position` in an open source-language document
    /// (#156).
    ///
    /// The request runs the same provider-owned mutation path as the CLI
    /// and agent surfaces: the provider computes the edits for the open
    /// document set, Wright verifies document versions, applies source
    /// preconditions, asks the provider to validate the transaction, and
    /// checks the edited project before returning anything. Every applied
    /// edit carries the validated document version so the caller can reject
    /// it once its buffer has moved. A refusal — unopen or unsupported
    /// document, unconfigured provider, provider refusal, validation
    /// failure — is structured and final; no textual fallback exists here.
    pub fn rename(&mut self, uri: &str, position: Position, new_name: &str) -> RenameOutcome {
        if self.store.document(uri).is_none() {
            return RenameOutcome::Refused {
                code: "rename-unknown-document".to_string(),
                message: format!("{uri} is not open in this session"),
            };
        }
        let Some(language_id) = source_language_id(uri) else {
            return RenameOutcome::Refused {
                code: "rename-unsupported-document".to_string(),
                message: format!(
                    "rename is provider-owned and {uri} is not a source-language document"
                ),
            };
        };

        let documents = self.source_document_set(&language_id);
        let sources = documents
            .iter()
            .map(|(uri, doc)| (uri.clone(), doc.text.clone()))
            .collect();
        let request = wright_driver::provider_edit::ProviderRenameRequest {
            documents,
            position_document_uri: uri.to_string(),
            position,
            new_name: new_name.to_string(),
            project_root: self.project_root_uri(),
            sources,
        };

        if let Err(diagnostic) = self.session() {
            return RenameOutcome::Refused {
                code: diagnostic.code,
                message: diagnostic.message,
            };
        }
        let mutation = self
            .session
            .as_ref()
            .expect("session initialized above")
            .run_provider_flow(
                &language_id,
                &wright_lpp::ClientInfo {
                    name: "wright-language".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                },
                |provider| wright_driver::provider_edit::semantic_rename(provider, &request),
            );

        if !mutation.ok {
            let diagnostic = mutation.diagnostics.first();
            return RenameOutcome::Refused {
                code: mutation
                    .provider_code
                    .or_else(|| diagnostic.map(|d| d.code.clone()))
                    .unwrap_or_else(|| "rename-refused".to_string()),
                message: mutation
                    .provider_message
                    .or_else(|| diagnostic.map(|d| d.message.clone()))
                    .unwrap_or_else(|| "the provider refused the rename".to_string()),
            };
        }
        let edits = mutation
            .transaction
            .map(|transaction| transaction.edits)
            .unwrap_or_default();
        let mut applied = Vec::with_capacity(edits.len());
        for edit in edits {
            let Some(doc) = self.store.document(&edit.source) else {
                return RenameOutcome::Refused {
                    code: "rename-edit-outside-open-documents".to_string(),
                    message: format!(
                        "the provider validated an edit against {}, which is not an open document",
                        edit.source
                    ),
                };
            };
            applied.push(SourceTextEdit {
                uri: edit.source,
                range: edit_range(&doc.text, &edit.range),
                new_text: edit.new_text,
                version: doc.version,
            });
        }
        RenameOutcome::Applied(applied)
    }

    fn workshop_document(&self, uri: &str) -> Option<&Document> {
        let document = self.store.document(uri)?;
        if source_language_id(uri).is_some() || !is_workshop_document(document) {
            return None;
        }
        Some(document)
    }

    fn workshop_session(
        &self,
        document: &Document,
    ) -> Result<CompilerSession, wright_driver::Diagnostic> {
        CompilerSession::new(SessionConfig {
            input: InputSpec::Text {
                text: document.text.clone(),
                path: uri_to_path(&document.uri),
            },
            kind: SourceKind::Workshop,
            root: Some(document.root.clone()),
            ..self.config.clone()
        })
    }

    fn workshop_index(&self, document: &Document) -> Option<SemanticIndex> {
        let mut session = self.workshop_session(document).ok()?;
        let loaded = session.load().ok()?;
        Some(SemanticIndex::build(&loaded.program))
    }

    /// The connected OPY provider, or the classified `(code, message)`
    /// its connect failure latched.
    fn opy_provider(&mut self) -> Result<&mut dyn LanguageProvider, (String, String)> {
        if matches!(self.opy, OpyProviderState::Pending) {
            self.opy = match self.connect_opy_provider() {
                Ok(provider) => OpyProviderState::Ready(provider),
                Err(failure) => OpyProviderState::Unavailable(failure.0, failure.1),
            };
        }
        match &mut self.opy {
            OpyProviderState::Ready(provider) => Ok(provider.as_mut()),
            OpyProviderState::Unavailable(code, message) => Err((code.clone(), message.clone())),
            OpyProviderState::Pending => unreachable!("resolved above"),
        }
    }

    /// The shared session, built lazily on the first provider request so a
    /// service that only serves Workshop documents never constructs one. A
    /// construction failure is Wright's own diagnostic, not a provider
    /// refusal: it surfaces with its own code (#1004).
    fn session(&mut self) -> Result<&CompilerSession, wright_driver::Diagnostic> {
        if self.session.is_none() {
            self.session = Some(CompilerSession::new(self.config.clone())?);
        }
        Ok(self.session.as_ref().expect("session built above"))
    }

    /// The OPY provider session, connected through the driver's shared
    /// connect path (#1004). The `(code, message)` failure the surface
    /// reports is a provider error classified through
    /// `provider_failure_code`, or a session-construction diagnostic
    /// carried verbatim.
    fn connect_opy_provider(&mut self) -> Result<Box<dyn LanguageProvider>, (String, String)> {
        let client = wright_lpp::ClientInfo {
            name: wright_lpp::LPP_CLIENT_NAME.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        };
        self.session()
            .map_err(|d| (d.code.clone(), d.message.clone()))?
            .connect_provider(
                OPY_LANGUAGE_ID,
                &client,
                wright_lpp::LPP_PROTOCOL_VERSION,
                None,
            )
            .map(|connected| connected.provider)
            .map_err(|e| {
                (
                    wright_driver::source_provider::provider_failure_code(&e),
                    e.to_string(),
                )
            })
    }

    fn opy_document(&self, uri: &str) -> Option<wright_lpp::Document> {
        let document = self.store.document(uri)?;
        if source_language_id(uri).as_deref() != Some(OPY_LANGUAGE_ID) {
            return None;
        }
        Some(wright_lpp::Document {
            uri: document.uri.clone(),
            language_id: OPY_LANGUAGE_ID.to_string(),
            version: i64::from(document.version),
            text: document.text.clone(),
        })
    }

    fn source_document_set(&self, language_id: &str) -> wright_lpp::DocumentSet {
        let mut documents = wright_lpp::DocumentSet::new();
        for uri in self.store.uris() {
            if source_language_id(uri).as_deref() != Some(language_id) {
                continue;
            }
            let document = self.store.document(uri).expect("uri came from the store");
            documents.insert(
                uri.to_string(),
                wright_lpp::Document {
                    uri: document.uri.clone(),
                    language_id: language_id.to_string(),
                    version: i64::from(document.version),
                    text: document.text.clone(),
                },
            );
        }
        documents
    }

    fn project_root_uri(&self) -> Option<String> {
        url::Url::from_directory_path(&self.root)
            .ok()
            .map(|uri| uri.to_string())
    }
}

fn provider_supports(provider: &dyn LanguageProvider, capability: Capability) -> bool {
    provider
        .capabilities()
        .map(|capabilities| capabilities.supports(capability))
        .unwrap_or(false)
}

fn lpp_severity(severity: wright_lpp::DiagnosticSeverity) -> &'static str {
    match severity {
        wright_lpp::DiagnosticSeverity::Error => "error",
        wright_lpp::DiagnosticSeverity::Warning => "warning",
        wright_lpp::DiagnosticSeverity::Info => "info",
        wright_lpp::DiagnosticSeverity::Hint => "hint",
    }
}

fn source_location(location: &wright_lpp::Location) -> SourceLocation {
    SourceLocation {
        uri: location.uri.clone(),
        range: location.range,
    }
}

fn source_symbol(symbol: &wright_lpp::Symbol) -> SourceSymbol {
    SourceSymbol {
        name: symbol.name.clone(),
        kind: symbol.kind.clone(),
        range: symbol.range,
        selection_range: symbol.selection_range,
    }
}

const PENDING_PROVIDER_LANGUAGES: &[&str] = &["ostw", "del"];

fn source_language_id(uri: &str) -> Option<String> {
    let ext = crate::document::uri_to_path(uri)?
        .extension()?
        .to_string_lossy()
        .to_lowercase();
    match ext.as_str() {
        "opy" => Some(wright_driver::opy_provider::OPY_LANGUAGE_ID.to_string()),
        id if PENDING_PROVIDER_LANGUAGES.contains(&id) => Some(id.to_string()),
        _ => None,
    }
}

fn is_workshop_document(document: &Document) -> bool {
    if let Some(language_id) = document.language_id.as_deref() {
        return language_id == "workshop";
    }
    let Some(path) = uri_to_path(&document.uri) else {
        return false;
    };
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "txt" | "ow" | "ws" | "workshop"
            )
        })
}

fn symbol_at<'index>(
    index: &'index SemanticIndex,
    document: &Document,
    position: Position,
) -> Option<(&'index Symbol, workshop_rs::source::Span)> {
    let (line, col) = document.to_line_col(position);
    match index.symbol_at(line, col) {
        PositionResolution::Resolved { symbol, occurrence } => Some((symbol, occurrence)),
        PositionResolution::Miss | PositionResolution::Ambiguous(_) => None,
    }
}

fn source_diagnostic(
    document: &Document,
    diagnostic: &wright_driver::Diagnostic,
) -> SourceDiagnostic {
    SourceDiagnostic {
        source: document.uri.clone(),
        range: diagnostic.span.as_ref().map_or(EMPTY_RANGE, |span| Range {
            start: line_col_position(&document.text, span.start.line, span.start.col),
            end: line_col_position(&document.text, span.end.line, span.end.col),
        }),
        severity: diagnostic.severity.as_str().to_string(),
        code: diagnostic.code.clone(),
        message: diagnostic.message.clone(),
        source_version: document.version,
        document_version: document.version,
    }
}

fn edit_range(text: &str, range: &wright_driver::edit::EditRange) -> Range {
    Range {
        start: line_col_position(text, range.start_line, range.start_col),
        end: line_col_position(text, range.end_line, range.end_col),
    }
}

const EMPTY_RANGE: Range = Range {
    start: Position {
        line: 0,
        character: 0,
    },
    end: Position {
        line: 0,
        character: 0,
    },
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    fn service() -> LanguageService {
        LanguageService::new(PathBuf::from("/project"))
    }

    fn open(service: &mut LanguageService, name: &str, text: &str) -> String {
        let uri = format!("file:///project/{name}");
        service.store.open(Document::with_version(
            uri.clone(),
            text,
            PathBuf::from("/project"),
            1,
        ));
        uri
    }

    fn refused(outcome: RenameOutcome) -> (String, String) {
        match outcome {
            RenameOutcome::Refused { code, message } => (code, message),
            RenameOutcome::Applied(_) => panic!("expected a refusal, got applied edits"),
        }
    }

    #[test]
    fn rename_on_an_unopen_document_refuses_without_a_provider() {
        let mut service = service();
        let (code, message) = refused(service.rename(
            "file:///project/main.opy",
            Position {
                line: 0,
                character: 0,
            },
            "renamed",
        ));
        assert_eq!(code, "rename-unknown-document");
        assert!(message.contains("not open"), "{message}");
    }

    #[test]
    fn rename_on_a_non_source_document_refuses_without_a_provider() {
        let mut service = service();
        let uri = open(&mut service, "notes.txt", "hello\n");
        let (code, _) = refused(service.rename(
            &uri,
            Position {
                line: 0,
                character: 0,
            },
            "renamed",
        ));
        assert_eq!(code, "rename-unsupported-document");
    }

    #[test]
    fn rename_on_a_source_document_without_a_provider_refuses() {
        let mut service = service();
        let uri = open(&mut service, "main.del", "x\n");
        let (code, message) = refused(service.rename(
            &uri,
            Position {
                line: 0,
                character: 0,
            },
            "renamed",
        ));
        assert_eq!(code, "provider-not-configured");
        assert!(message.contains("del"), "{message}");
    }

    #[test]
    fn rename_on_opy_with_an_unresolvable_provider_refuses() {
        let config = SessionConfig {
            opy_provider: wright_driver::OpyProviderConfig::with_executable(
                "/nonexistent/opy-provider",
            ),
            ..SessionConfig::default()
        };
        let mut service = LanguageService::with_config(PathBuf::from("/project"), config);
        let uri = open(&mut service, "main.opy", "x\n");
        let (code, _) = refused(service.rename(
            &uri,
            Position {
                line: 0,
                character: 0,
            },
            "renamed",
        ));
        assert_ne!(code, "rename-refused", "a structured code must surface");
    }

    #[test]
    fn utf16_edit_ranges_convert_from_driver_columns() {
        let text = "🎯 = 1\ny = 🎯 + 2\n";
        let range = wright_driver::edit::EditRange {
            start_line: 2,
            start_col: 5,
            end_line: 2,
            end_col: 6,
        };
        let converted = edit_range(text, &range);
        assert_eq!(converted.start.line, 1);
        assert_eq!(converted.start.character, 4);
        assert_eq!(converted.end.character, 6);
    }

    #[test]
    fn edit_range_on_crlf_does_not_count_the_carriage_return() {
        let text = "a = 1\r\nb = 2\r\n";
        let range = wright_driver::edit::EditRange {
            start_line: 2,
            start_col: 1,
            end_line: 2,
            end_col: 7,
        };
        let converted = edit_range(text, &range);
        assert_eq!(converted.start.character, 0);
        assert_eq!(
            converted.end.character, 5,
            "'b = 2' is 5 chars; the \\r is not a column"
        );
    }
}
