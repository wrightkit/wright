//! The editor-neutral language service (#63, #65, #66).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;
use wright_driver::{CompilerSession, SessionConfig};

use crate::document::{DocumentStore, Position, Range, line_col_position};

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

pub struct LanguageService {
    pub store: DocumentStore,
    pub root: PathBuf,
    config: SessionConfig,
    session: Option<CompilerSession>,
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
        }
    }

    pub fn diagnostics(&self, uri: &str) -> Vec<SourceDiagnostic> {
        let Some(document) = self.store.document(uri) else {
            return Vec::new();
        };
        if !is_source_document(uri) {
            return Vec::new();
        }
        vec![SourceDiagnostic {
            source: document.uri.clone(),
            range: empty_range(),
            severity: "error".to_string(),
            code: "source-provider-unavailable".to_string(),
            message:
                "source-language analysis is provider-owned; diagnostics are not yet negotiated"
                    .to_string(),
            source_version: document.version,
            document_version: document.version,
        }]
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

        // The document set stays language-wide: only the provider can
        // compute project membership, and `lpp/rename` needs every open
        // document as a potential include or rename site. `semantic_rename`
        // scopes the post-edit check verdict to the documents the provider
        // actually edited (plus the position document), so an unrelated
        // broken open document cannot block a valid rename.
        let mut documents = wright_lpp::DocumentSet::new();
        let mut sources = BTreeMap::new();
        for doc_uri in self.store.uris() {
            if source_language_id(doc_uri).as_deref() != Some(language_id.as_str()) {
                continue;
            }
            let doc = self
                .store
                .document(doc_uri)
                .expect("uri came from the store");
            documents.insert(
                doc_uri.to_string(),
                wright_lpp::Document {
                    uri: doc.uri.clone(),
                    language_id: language_id.clone(),
                    version: doc.version as i64,
                    text: doc.text.clone(),
                },
            );
            sources.insert(doc_uri.to_string(), doc.text.clone());
        }
        let request = wright_driver::provider_edit::ProviderRenameRequest {
            documents,
            position_document_uri: uri.to_string(),
            position: wright_lpp::Position {
                line: position.line,
                character: position.character,
            },
            new_name: new_name.to_string(),
            project_root: url::Url::from_directory_path(&self.root)
                .ok()
                .map(|u| u.to_string()),
            sources,
        };

        if self.session.is_none() {
            self.session = match CompilerSession::new(self.config.clone()) {
                Ok(session) => Some(session),
                Err(diagnostic) => {
                    return RenameOutcome::Refused {
                        code: diagnostic.code,
                        message: diagnostic.message,
                    };
                }
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
            // `finish_transaction` refuses edits outside the document set, so
            // every source is an open document; a partial `Applied` set must
            // never escape.
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
                // `semantic_rename` verified the provider's edits against
                // this exact version; the caller uses it to reject stale
                // edits.
                version: doc.version,
            });
        }
        RenameOutcome::Applied(applied)
    }
}

/// Map a document URI to its provider language id when it is a
/// source-language document.
fn source_language_id(uri: &str) -> Option<String> {
    let ext = crate::document::uri_to_path(uri)?
        .extension()?
        .to_string_lossy()
        .to_lowercase();
    match ext.as_str() {
        "opy" => Some(wright_driver::opy_provider::OPY_LANGUAGE_ID.to_string()),
        // DEL/OSTW have no provider implementation yet; the extension is the
        // opaque id so the registry refusal stays explicit and names the
        // language once a provider ships.
        "ostw" | "del" => Some(ext),
        _ => None,
    }
}

fn is_source_document(uri: &str) -> bool {
    source_language_id(uri).is_some()
}

/// Convert a driver [`wright_driver::edit::EditRange`] (1-based line and
/// character column, half-open) back into UTF-16 document coordinates.
fn edit_range(text: &str, range: &wright_driver::edit::EditRange) -> Range {
    Range {
        start: line_col_position(text, range.start_line, range.start_col),
        end: line_col_position(text, range.end_line, range.end_col),
    }
}

fn empty_range() -> Range {
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: 0,
            character: 0,
        },
    }
}

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
        // No DEL provider exists, so `.del` deterministically reaches the
        // unconfigured-provider refusal — the explicit contract, not a
        // textual fallback.
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
        // The explicit executable fails validation before any spawn.
        assert_ne!(code, "rename-refused", "a structured code must surface");
    }

    /// `WRIGHT_OPY_PROVIDER` points at an `opy-provider` executable; without
    /// it the provider-backed rename cannot run and the test self-skips.
    #[test]
    fn an_unrelated_broken_open_document_does_not_block_a_project_rename() {
        let Ok(provider) = std::env::var("WRIGHT_OPY_PROVIDER") else {
            eprintln!("SKIPPED: WRIGHT_OPY_PROVIDER is not set");
            return;
        };
        let config = SessionConfig {
            opy_provider: wright_driver::OpyProviderConfig::with_executable(PathBuf::from(
                provider,
            )),
            ..SessionConfig::default()
        };
        let mut service = LanguageService::with_config(PathBuf::from("/project"), config);
        let uri = open(&mut service, "main.opy", "globalvar score = 0\n");
        // A broken OPY document in the same language but outside the
        // position document's project: its error must not block the rename.
        open(
            &mut service,
            "unrelated/broken.opy",
            "#!include \"missing.opy\"\n",
        );
        match service.rename(
            &uri,
            Position {
                line: 0,
                character: 12,
            },
            "vault",
        ) {
            RenameOutcome::Applied(edits) => {
                assert!(!edits.is_empty());
                assert!(
                    edits.iter().all(|edit| edit.version == 1),
                    "every applied edit carries the validated document version"
                );
            }
            RenameOutcome::Refused { code, message } => {
                panic!("expected applied edits; refused ({code}): {message}")
            }
        }
    }

    #[test]
    fn utf16_edit_ranges_convert_from_driver_columns() {
        // EditRange cols are 1-based character columns over a line whose
        // first char is 🎯 (two UTF-16 units).
        let text = "🎯 = 1\ny = 🎯 + 2\n";
        let range = wright_driver::edit::EditRange {
            start_line: 2,
            start_col: 5,
            end_line: 2,
            end_col: 6,
        };
        let converted = edit_range(text, &range);
        assert_eq!(converted.start.line, 1);
        // Char col 5 is 🎯 itself, starting at utf16 offset 4 (y, space, =,
        // space before it); the end col 6 lands after it at utf16 offset 6.
        assert_eq!(converted.start.character, 4);
        assert_eq!(converted.end.character, 6);
    }

    #[test]
    fn edit_range_on_crlf_does_not_count_the_carriage_return() {
        // str::lines drops the \r: a column past it must land on the last
        // real char, matching span_to_range's line split.
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
