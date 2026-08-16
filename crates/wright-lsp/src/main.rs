//! `wright-lsp` — the Wright language server (M10, issue #67).
//!
//! A thin LSP protocol adapter over [`wright_language::LanguageService`]:
//! all semantic logic lives in the editor-neutral service crate; this
//! binary only maps LSP DTOs to and from it, handles the stdio
//! Content-Length framing, and manages document lifecycle with correct
//! version identity and per-root publication ownership (#72), so diagnostics
//! previously published for a source are retired when that source
//! disappears from a later root analysis. No duplicate semantic logic exists
//! here.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::str::FromStr;

use lsp_types::notification::{Notification, PublishDiagnostics};
use lsp_types::{
    CompletionItem as LspCompletionItem, CompletionItemKind, CompletionParams, CompletionResponse,
    Diagnostic as LspDiagnostic, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentChanges, GotoDefinitionParams,
    GotoDefinitionResponse, Hover as LspHover, HoverContents, InitializeParams, InitializeResult,
    Location, MarkupContent, MarkupKind, OptionalVersionedTextDocumentIdentifier,
    Position as LspPosition, PositionEncodingKind, PublishDiagnosticsParams, Range as LspRange,
    ReferenceParams, RenameParams, SemanticTokens, SemanticTokensFullOptions, SemanticTokensLegend,
    SemanticTokensParams, SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo,
    TextDocumentEdit, TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextEdit, Uri, WorkDoneProgressOptions, WorkspaceEdit,
};
use serde_json::Value;

use wright_language::document::{Document, Position, Range};
use wright_language::{LanguageService, SemanticToken as WtToken};

/// Per-root publication ownership: the set of source URIs each open root
/// document last published diagnostics for.
///
/// Map keys are the store document URIs (the requesting/root documents);
/// values are canonical publication URI strings — the same form
/// [`publish_to`] sends on the wire — so the retirement diff and the
/// cross-root ownership guard compare like-for-like (a source reached as an
/// include path and as an open document URI collapse to one identity).
type PublicationOwnership = BTreeMap<String, BTreeSet<String>>;

fn main() {
    // Non-interactive version probe (#101): `wright-lsp --version` prints the
    // implementation version and exits without entering the stdio loop, so
    // scripts and release smoke tests can identify the executable.
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("wright-lsp {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if let Err(message) = run() {
        eprintln!("wright-lsp: {message}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    // The workspace root defaults to the cwd until `initialize` supplies
    // rootUri/workspaceFolders.
    let mut root = std::env::current_dir().map_err(|error| error.to_string())?;
    let mut service = LanguageService::new(root.clone());
    let mut ownership: PublicationOwnership = BTreeMap::new();

    loop {
        let message = read_message(&mut reader)?;
        let value: Value = serde_json::from_str(&message).map_err(|error| error.to_string())?;
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let id = value.get("id").cloned();
        let params = value.get("params").cloned();

        match method.as_str() {
            "initialize" => {
                if let Some(params) = params {
                    if let Ok(initialize) = serde_json::from_value::<InitializeParams>(params) {
                        if let Some(resolved) = initialize_root(&initialize) {
                            root = resolved;
                            service = LanguageService::new(root.clone());
                            // The project root changed: previous publications
                            // belong to the old service state, so ownership is
                            // reset alongside it.
                            ownership.clear();
                        }
                    }
                }
                let result = initialize_result();
                write_response(&mut writer, id, serde_json::to_value(result).unwrap())?;
            }
            "initialized" => {}
            "shutdown" => {
                write_response(&mut writer, id, Value::Null)?;
            }
            "exit" => break,
            "textDocument/didOpen" => {
                let params: DidOpenTextDocumentParams =
                    serde_json::from_value(params.unwrap()).map_err(|error| error.to_string())?;
                let uri = params.text_document.uri.to_string();
                let document = Document::with_version(
                    uri.clone(),
                    params.text_document.text,
                    root.clone(),
                    params.text_document.version,
                );
                service.store.open(document);
                publish_affected_diagnostics(&mut writer, &service, &mut ownership, &uri)?;
            }
            "textDocument/didChange" => {
                let params: DidChangeTextDocumentParams =
                    serde_json::from_value(params.unwrap()).map_err(|error| error.to_string())?;
                let uri = params.text_document.uri.to_string();
                if let Some(change) = params.content_changes.last() {
                    service
                        .store
                        .change(&uri, &change.text, params.text_document.version);
                }
                publish_affected_diagnostics(&mut writer, &service, &mut ownership, &uri)?;
            }
            "textDocument/didClose" => {
                let params: DidCloseTextDocumentParams =
                    serde_json::from_value(params.unwrap()).map_err(|error| error.to_string())?;
                let uri = params.text_document.uri.to_string();
                // Retire the diagnostics this root published that no other
                // open root still owns, then refresh the roots that depend
                // on the closed document.
                let owned = ownership.remove(&uri).unwrap_or_default();
                service.store.close(&uri);
                for source in owned {
                    if !other_roots_own(&ownership, &source) {
                        publish_to(
                            &mut writer,
                            &source,
                            source_version(&service, &source),
                            Vec::new(),
                        )?;
                    }
                }
                publish_affected_diagnostics(&mut writer, &service, &mut ownership, &uri)?;
            }
            "textDocument/didSave" => {
                // Full-sync documents treat the in-memory overlay as the
                // source of truth, so saving to disk does not change the
                // analyzed text; this lifecycle point is intentionally a
                // no-op (an explicit handler, not a silent default).
                let _ = params;
            }
            "textDocument/hover" => {
                let params: TextDocumentPositionParams =
                    serde_json::from_value(params.unwrap()).map_err(|error| error.to_string())?;
                let position = convert_position(params.position);
                let result = service
                    .hover(&params.text_document.uri.to_string(), position)
                    .map(|hover| LspHover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: hover.contents,
                        }),
                        range: hover.range.map(convert_range),
                    });
                write_response(&mut writer, id, serde_json::to_value(result).unwrap())?;
            }
            "textDocument/definition" => {
                let params: GotoDefinitionParams =
                    serde_json::from_value(params.unwrap()).map_err(|error| error.to_string())?;
                let position = convert_position(params.text_document_position_params.position);
                let result = service
                    .definition(
                        &params
                            .text_document_position_params
                            .text_document
                            .uri
                            .to_string(),
                        position,
                    )
                    .map(|location| {
                        GotoDefinitionResponse::Scalar(Location {
                            uri: source_to_uri(&location.source),
                            range: convert_range(location.range),
                        })
                    });
                write_response(&mut writer, id, serde_json::to_value(result).unwrap())?;
            }
            "textDocument/references" => {
                let params: ReferenceParams =
                    serde_json::from_value(params.unwrap()).map_err(|error| error.to_string())?;
                let position = convert_position(params.text_document_position.position);
                let uri = params.text_document_position.text_document.uri;
                let result: Vec<Location> = service
                    .references(&uri.to_string(), position)
                    .into_iter()
                    .map(|location| Location {
                        uri: source_to_uri(&location.source),
                        range: convert_range(location.range),
                    })
                    .collect();
                write_response(&mut writer, id, serde_json::to_value(result).unwrap())?;
            }
            "textDocument/completion" => {
                let params: CompletionParams =
                    serde_json::from_value(params.unwrap()).map_err(|error| error.to_string())?;
                let position = convert_position(params.text_document_position.position);
                let uri = params.text_document_position.text_document.uri;
                let items: Vec<LspCompletionItem> = service
                    .completion(&uri.to_string(), position)
                    .into_iter()
                    .map(|item| LspCompletionItem {
                        label: item.label,
                        kind: Some(completion_kind(&item.kind)),
                        detail: item.detail,
                        ..Default::default()
                    })
                    .collect();
                let result = CompletionResponse::Array(items);
                write_response(&mut writer, id, serde_json::to_value(result).unwrap())?;
            }
            "textDocument/rename" => {
                let params: RenameParams =
                    serde_json::from_value(params.unwrap()).map_err(|error| error.to_string())?;
                let position = convert_position(params.text_document_position.position);
                let uri = params.text_document_position.text_document.uri;
                let rename = service.rename(&uri.to_string(), position, &params.new_name);
                if rename.ok {
                    // Version-aware document edits (#73): each open document
                    // is identified at its current version through the
                    // standard `documentChanges`/`TextDocumentEdit` form, so
                    // the client refuses to apply the rename to a newer
                    // buffer state; filesystem-backed sources carry the
                    // unversioned `null` form LSP allows. The edits are the
                    // shared M14 #129 transaction in editor conventions —
                    // one exact-occurrence `TextEdit` per semantic
                    // occurrence, grouped by document — never a
                    // protocol-layer re-resolution or textual fallback. The
                    // unversioned `changes`-only shape is never used for
                    // rename, and no Wright-specific extension is introduced.
                    let mut by_source: std::collections::BTreeMap<
                        String,
                        Vec<wright_language::service::RenameEdit>,
                    > = std::collections::BTreeMap::new();
                    for edit in rename.edits {
                        by_source.entry(edit.source.clone()).or_default().push(edit);
                    }
                    let document_changes = DocumentChanges::Edits(
                        by_source
                            .into_iter()
                            .map(|(source, edits)| TextDocumentEdit {
                                text_document: OptionalVersionedTextDocumentIdentifier {
                                    uri: source_to_uri(&source),
                                    version: source_version(&service, &source),
                                },
                                edits: edits
                                    .into_iter()
                                    .map(|edit| {
                                        lsp_types::OneOf::Left(TextEdit {
                                            range: convert_range(edit.range),
                                            new_text: edit.new_text,
                                        })
                                    })
                                    .collect(),
                            })
                            .collect(),
                    );
                    let workspace_edit = WorkspaceEdit {
                        document_changes: Some(document_changes),
                        ..Default::default()
                    };
                    write_response(
                        &mut writer,
                        id,
                        serde_json::to_value(workspace_edit).unwrap(),
                    )?;
                } else {
                    let detail = rename.diagnostics.join("; ");
                    write_error(
                        &mut writer,
                        id,
                        -32602,
                        &format!("rename refused: {detail}"),
                    )?;
                }
            }
            "textDocument/semanticTokens/full" => {
                let params: SemanticTokensParams =
                    serde_json::from_value(params.unwrap()).map_err(|error| error.to_string())?;
                let uri = params.text_document.uri;
                let tokens = service.semantic_tokens(&uri.to_string());
                let result = SemanticTokens {
                    result_id: None,
                    data: encode_semantic_tokens(&tokens),
                };
                write_response(&mut writer, id, serde_json::to_value(result).unwrap())?;
            }
            other => {
                // Unknown requests get a method-not-found error; notifications
                // are ignored.
                if id.is_some() {
                    write_response(&mut writer, id, Value::Null)?;
                }
                let _ = other;
            }
        }
    }
    Ok(())
}

/// The server capabilities: documents, hover, definition, references,
/// completion, rename, and full semantic tokens.
fn initialize_result() -> InitializeResult {
    InitializeResult {
        capabilities: ServerCapabilities {
            position_encoding: Some(PositionEncodingKind::UTF16),
            text_document_sync: Some(TextDocumentSyncCapability::Options(
                TextDocumentSyncOptions {
                    open_close: Some(true),
                    change: Some(TextDocumentSyncKind::FULL),
                    ..Default::default()
                },
            )),
            hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
            definition_provider: Some(lsp_types::OneOf::Left(true)),
            references_provider: Some(lsp_types::OneOf::Left(true)),
            completion_provider: Some(lsp_types::CompletionOptions {
                trigger_characters: None,
                resolve_provider: Some(false),
                ..Default::default()
            }),
            rename_provider: Some(lsp_types::OneOf::Left(true)),
            semantic_tokens_provider: Some(
                SemanticTokensServerCapabilities::SemanticTokensOptions(
                    lsp_types::SemanticTokensOptions {
                        work_done_progress_options: WorkDoneProgressOptions {
                            work_done_progress: None,
                        },
                        legend: SemanticTokensLegend {
                            token_types: vec![
                                "keyword".into(),
                                "variable".into(),
                                "identifier".into(),
                                "string".into(),
                                "number".into(),
                                "operator".into(),
                                "macro".into(),
                                "attribute".into(),
                            ],
                            token_modifiers: vec![],
                        },
                        range: Some(false),
                        full: Some(SemanticTokensFullOptions::Bool(true)),
                    },
                ),
            ),
            ..Default::default()
        },
        server_info: Some(ServerInfo {
            name: "wright-lsp".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
        }),
    }
}

/// Read one Content-Length framed LSP message.
fn read_message(reader: &mut impl BufRead) -> Result<String, String> {
    let mut content_length = None;
    loop {
        let mut header = String::new();
        if reader
            .read_line(&mut header)
            .map_err(|error| error.to_string())?
            == 0
        {
            return Err("stdin closed".to_string());
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some(value) = header.strip_prefix("Content-Length:") {
            content_length = value.trim().parse::<usize>().ok();
        }
    }
    let length = content_length.ok_or_else(|| "missing Content-Length header".to_string())?;
    let mut body = vec![0u8; length];
    reader
        .read_exact(&mut body)
        .map_err(|error| error.to_string())?;
    String::from_utf8(body).map_err(|error| error.to_string())
}

/// Write one LSP response message.
fn write_response(writer: &mut impl Write, id: Option<Value>, result: Value) -> Result<(), String> {
    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    let body = serde_json::to_string(&response).map_err(|error| error.to_string())?;
    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body)
        .and_then(|_| writer.flush())
        .map_err(|error| error.to_string())
}

/// Write one LSP error response message (structured refusal).
fn write_error(
    writer: &mut impl Write,
    id: Option<Value>,
    code: i64,
    message: &str,
) -> Result<(), String> {
    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    });
    let body = serde_json::to_string(&response).map_err(|error| error.to_string())?;
    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body)
        .and_then(|_| writer.flush())
        .map_err(|error| error.to_string())
}

/// Publish diagnostics for every document affected by a change to `uri`,
/// routing each affected root through the stateful publication-ownership
/// refresh.
fn publish_affected_diagnostics(
    writer: &mut impl Write,
    service: &LanguageService,
    ownership: &mut PublicationOwnership,
    changed_uri: &str,
) -> Result<(), String> {
    for affected_uri in service.dependent_documents(changed_uri) {
        refresh_root(writer, service, ownership, &affected_uri)?;
    }
    Ok(())
}

/// Refresh one root document's diagnostics with publication ownership.
///
/// Groups the root's current diagnostics by source identity, publishes each
/// group normally (the requesting/root URI is always published, possibly
/// empty, so its stale markers are cleared), then retires sources this root
/// previously published that are no longer present — unless another recorded
/// root currently owns them — with an empty publication. The root's new
/// ownership set is recorded last.
fn refresh_root(
    writer: &mut impl Write,
    service: &LanguageService,
    ownership: &mut PublicationOwnership,
    root_uri: &str,
) -> Result<(), String> {
    let diagnostics = service.diagnostics(root_uri);
    let mut by_source: BTreeMap<String, Vec<LspDiagnostic>> = BTreeMap::new();
    for diagnostic in diagnostics {
        by_source
            .entry(publication_uri(&diagnostic.source))
            .or_default()
            .push(convert_diagnostic(diagnostic));
    }
    // Always publish (possibly empty) for the requested document so stale
    // markers are cleared.
    by_source.entry(publication_uri(root_uri)).or_default();

    // The refreshed root is removed from the map before the ownership guard
    // runs, so only *other* roots' current ownership protects a source.
    let previous = ownership.remove(root_uri).unwrap_or_default();
    let current: BTreeSet<String> = by_source.keys().cloned().collect();

    for (source, diagnostics) in by_source {
        publish_to(
            writer,
            &source,
            source_version(service, &source),
            diagnostics,
        )?;
    }
    for retired in previous.difference(&current) {
        if !other_roots_own(ownership, retired) {
            publish_to(
                writer,
                retired,
                source_version(service, retired),
                Vec::new(),
            )?;
        }
    }

    ownership.insert(root_uri.to_string(), current);
    Ok(())
}

/// Whether any recorded root currently owns `source` (the refreshed/closed
/// root is removed from the map before this is consulted).
fn other_roots_own(ownership: &PublicationOwnership, source: &str) -> bool {
    ownership.values().any(|owned| owned.contains(source))
}

/// The canonical URI string under which a source identity is published and
/// tracked, matching the form [`publish_to`] sends on the wire. Both
/// directions (URI and resolved path) collapse to the same identity.
fn publication_uri(source: &str) -> String {
    wright_language::document::source_to_uri(source).unwrap_or_else(|| fallback_uri().to_string())
}

/// Convert one editor-neutral source-aware diagnostic into an LSP diagnostic.
fn convert_diagnostic(diagnostic: wright_language::SourceDiagnostic) -> LspDiagnostic {
    LspDiagnostic {
        range: convert_range(diagnostic.range),
        severity: Some(match diagnostic.severity.as_str() {
            "error" => DiagnosticSeverity::ERROR,
            "warning" => DiagnosticSeverity::WARNING,
            _ => DiagnosticSeverity::INFORMATION,
        }),
        code: Some(lsp_types::NumberOrString::String(diagnostic.code)),
        message: diagnostic.message,
        ..Default::default()
    }
}

/// Write one `textDocument/publishDiagnostics` notification for a source
/// identity (document URI or resolved filesystem path).
fn publish_to(
    writer: &mut impl Write,
    source: &str,
    version: Option<i32>,
    diagnostics: Vec<LspDiagnostic>,
) -> Result<(), String> {
    let params = PublishDiagnosticsParams {
        uri: Uri::from_str(&publication_uri(source)).unwrap_or_else(|_| fallback_uri()),
        diagnostics,
        version,
    };
    let notification = serde_json::json!({
        "jsonrpc": "2.0",
        "method": PublishDiagnostics::METHOD,
        "params": serde_json::to_value(params).unwrap(),
    });
    let body = serde_json::to_string(&notification).unwrap();
    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body)
        .and_then(|_| writer.flush())
        .map_err(|error| error.to_string())
}

/// The current version of a source identity, when it is an open document.
fn source_version(service: &LanguageService, source: &str) -> Option<i32> {
    if let Some(document) = service.store.document(source) {
        return Some(document.version);
    }
    // The source may be a canonical publication URI or a resolved filesystem
    // path; decode URIs before matching open documents by path.
    let path =
        wright_language::document::uri_to_path(source).unwrap_or_else(|| PathBuf::from(source));
    service
        .store
        .uri_for_path(&path)
        .and_then(|uri| service.store.document(&uri))
        .map(|document| document.version)
}

fn convert_position(position: LspPosition) -> Position {
    Position {
        line: position.line,
        character: position.character,
    }
}

/// Resolve the workspace root from LSP initialize parameters: `rootUri`
/// first, then the first `workspaceFolders` URI. An empty/invalid URI leaves
/// the current cwd root in place.
#[allow(deprecated)]
fn initialize_root(params: &InitializeParams) -> Option<std::path::PathBuf> {
    if let Some(uri) = &params.root_uri {
        if let Some(path) = uri_to_path(uri.as_str()) {
            return Some(path);
        }
    }
    params
        .workspace_folders
        .as_ref()
        .and_then(|folders| folders.first())
        .and_then(|folder| uri_to_path(folder.uri.as_str()))
}

/// Resolve a workspace-root `file://` URI to a filesystem path. The
/// filesystem root itself (`file:///`) and empty paths are treated as absent
/// so the process cwd stays the fallback root.
fn uri_to_path(uri: &str) -> Option<std::path::PathBuf> {
    let path = wright_language::document::uri_to_path(uri)?;
    if path.as_os_str().is_empty() || path == std::path::Path::new("/") {
        return None;
    }
    Some(path)
}

/// Map an editor-neutral source identity (a file URI or a resolved
/// filesystem path) to a valid LSP URI through the shared standard file-URI
/// conversion, so percent-encoded/spaced/Unicode paths round-trip correctly.
fn source_to_uri(source: &str) -> Uri {
    match wright_language::document::source_to_uri(source) {
        Some(uri) => Uri::from_str(&uri).unwrap_or_else(|_| fallback_uri()),
        None => fallback_uri(),
    }
}

fn fallback_uri() -> Uri {
    Uri::from_str("file:///unknown").expect("static URI parses")
}

fn convert_range(range: Range) -> LspRange {
    LspRange {
        start: LspPosition {
            line: range.start.line,
            character: range.start.character,
        },
        end: LspPosition {
            line: range.end.line,
            character: range.end.character,
        },
    }
}

fn completion_kind(kind: &str) -> CompletionItemKind {
    match kind {
        "globalVariable" | "playerVariable" | "variable" => CompletionItemKind::VARIABLE,
        "subroutine" | "function" => CompletionItemKind::FUNCTION,
        "rule" => CompletionItemKind::CLASS,
        "keyword" => CompletionItemKind::KEYWORD,
        _ => CompletionItemKind::TEXT,
    }
}

/// Encode semantic tokens in LSP's relative delta format.
fn encode_semantic_tokens(tokens: &[WtToken]) -> Vec<lsp_types::SemanticToken> {
    let mut data = Vec::new();
    let mut previous_line = 0u32;
    let mut previous_character = 0u32;
    for token in tokens {
        let line_delta = token.line.saturating_sub(previous_line);
        let character_delta = if line_delta == 0 {
            token.character.saturating_sub(previous_character)
        } else {
            token.character
        };
        let token_type = match token.token_type.as_str() {
            "keyword" => 0,
            "variable" => 1,
            "identifier" => 2,
            "string" => 3,
            "number" => 4,
            "operator" => 5,
            "macro" => 6,
            "attribute" => 7,
            _ => 2,
        };
        data.push(lsp_types::SemanticToken {
            delta_line: line_delta,
            delta_start: character_delta,
            length: token.length,
            token_type,
            token_modifiers_bitset: 0,
        });
        previous_line = token.line;
        previous_character = token.character;
    }
    data
}
