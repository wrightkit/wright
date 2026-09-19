//! A thin LSP protocol adapter over [`wright_language::LanguageService`]:
//! all semantic logic lives in the editor-neutral service crate; this
//! binary only maps LSP DTOs to and from it, handles the stdio
//! Content-Length framing, and manages document lifecycle with correct
//! version identity and per-root publication ownership (#72).

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
    TextDocumentSyncOptions, TextEdit, Uri, WorkspaceEdit,
};
use serde_json::Value;

use wright_language::document::{Document, Position, Range};
use wright_language::{LanguageService, SemanticToken as WtToken, SourceLocation};

type PublicationOwnership = BTreeMap<String, BTreeSet<String>>;

fn main() {
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

    let mut root = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut service = LanguageService::new(root.clone());
    let mut ownership: PublicationOwnership = BTreeMap::new();

    loop {
        let message = read_message(&mut reader)?;
        let value: Value = serde_json::from_str(&message).map_err(|e| e.to_string())?;
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
                    if let Ok(init) = serde_json::from_value::<InitializeParams>(params) {
                        if let Some(resolved) = initialize_root(&init) {
                            root = resolved;
                            service = LanguageService::new(root.clone());
                            ownership.clear();
                        }
                    }
                }
                write_response(&mut writer, id, initialize_result())?;
            }
            "initialized" => {}
            "shutdown" => {
                write_response(&mut writer, id, Value::Null)?;
            }
            "exit" => break,
            "textDocument/didOpen" => {
                let params: DidOpenTextDocumentParams =
                    serde_json::from_value(params.unwrap()).map_err(|e| e.to_string())?;
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
                    serde_json::from_value(params.unwrap()).map_err(|e| e.to_string())?;
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
                    serde_json::from_value(params.unwrap()).map_err(|e| e.to_string())?;
                let uri = params.text_document.uri.to_string();
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
            "textDocument/didSave" => {}
            "textDocument/hover" => {
                let p: TextDocumentPositionParams =
                    serde_json::from_value(params.unwrap()).map_err(|e| e.to_string())?;
                let result = service
                    .hover(
                        &p.text_document.uri.to_string(),
                        convert_position(p.position),
                    )
                    .map(|h| LspHover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: h.contents,
                        }),
                        range: h.range.map(convert_range),
                    });
                write_response(&mut writer, id, result)?;
            }
            "textDocument/definition" => {
                let p: GotoDefinitionParams =
                    serde_json::from_value(params.unwrap()).map_err(|e| e.to_string())?;
                let (uri, pos) = (
                    p.text_document_position_params
                        .text_document
                        .uri
                        .to_string(),
                    convert_position(p.text_document_position_params.position),
                );
                let result = service
                    .definition(&uri, pos)
                    .map(convert_location)
                    .map(GotoDefinitionResponse::Scalar);
                write_response(&mut writer, id, result)?;
            }
            "textDocument/references" => {
                let p: ReferenceParams =
                    serde_json::from_value(params.unwrap()).map_err(|e| e.to_string())?;
                let (uri, pos) = (
                    p.text_document_position.text_document.uri.to_string(),
                    convert_position(p.text_document_position.position),
                );
                let result: Vec<Location> = service
                    .references(&uri, pos)
                    .into_iter()
                    .map(convert_location)
                    .collect();
                write_response(&mut writer, id, result)?;
            }
            "textDocument/completion" => {
                let p: CompletionParams =
                    serde_json::from_value(params.unwrap()).map_err(|e| e.to_string())?;
                let items: Vec<LspCompletionItem> = service
                    .completion(
                        &p.text_document_position.text_document.uri.to_string(),
                        convert_position(p.text_document_position.position),
                    )
                    .into_iter()
                    .map(|item| LspCompletionItem {
                        label: item.label,
                        kind: Some(completion_kind(&item.kind)),
                        detail: item.detail,
                        ..Default::default()
                    })
                    .collect();
                write_response(&mut writer, id, CompletionResponse::Array(items))?;
            }
            "textDocument/rename" => {
                let params: RenameParams =
                    serde_json::from_value(params.unwrap()).map_err(|e| e.to_string())?;
                let position = convert_position(params.text_document_position.position);
                let uri = params.text_document_position.text_document.uri.to_string();
                let rename = service.rename(&uri, position, &params.new_name);
                if rename.ok {
                    let mut by_source: BTreeMap<String, Vec<TextEdit>> = BTreeMap::new();
                    for edit in rename.edits {
                        by_source.entry(edit.source).or_default().push(TextEdit {
                            range: convert_range(edit.range),
                            new_text: edit.new_text,
                        });
                    }
                    let document_changes = DocumentChanges::Edits(
                        by_source
                            .into_iter()
                            .map(|(source, edits)| TextDocumentEdit {
                                text_document: OptionalVersionedTextDocumentIdentifier {
                                    uri: source_to_uri(&source),
                                    version: source_version(&service, &source),
                                },
                                edits: edits.into_iter().map(lsp_types::OneOf::Left).collect(),
                            })
                            .collect(),
                    );
                    write_response(
                        &mut writer,
                        id,
                        WorkspaceEdit {
                            document_changes: Some(document_changes),
                            ..Default::default()
                        },
                    )?;
                } else {
                    write_error(
                        &mut writer,
                        id,
                        -32602,
                        &format!("rename refused: {}", rename.diagnostics.join("; ")),
                    )?;
                }
            }
            "textDocument/semanticTokens/full" => {
                let params: SemanticTokensParams =
                    serde_json::from_value(params.unwrap()).map_err(|e| e.to_string())?;
                let uri = params.text_document.uri;
                let tokens = service.semantic_tokens(&uri.to_string());
                let result = SemanticTokens {
                    result_id: None,
                    data: encode_semantic_tokens(&tokens),
                };
                write_response(&mut writer, id, result)?;
            }
            _ => {
                if id.is_some() {
                    write_response(&mut writer, id, Value::Null)?;
                }
            }
        }
    }
    Ok(())
}

fn convert_location(loc: SourceLocation) -> Location {
    Location {
        uri: source_to_uri(&loc.source),
        range: convert_range(loc.range),
    }
}

fn initialize_result() -> InitializeResult {
    let sync = TextDocumentSyncOptions {
        open_close: Some(true),
        change: Some(TextDocumentSyncKind::FULL),
        ..Default::default()
    };
    let sem = lsp_types::SemanticTokensOptions {
        legend: SemanticTokensLegend {
            token_types: [
                "keyword",
                "variable",
                "identifier",
                "string",
                "number",
                "operator",
                "macro",
                "attribute",
            ]
            .map(Into::into)
            .to_vec(),
            token_modifiers: vec![],
        },
        full: Some(SemanticTokensFullOptions::Bool(true)),
        ..Default::default()
    };
    InitializeResult {
        capabilities: ServerCapabilities {
            position_encoding: Some(PositionEncodingKind::UTF16),
            text_document_sync: Some(TextDocumentSyncCapability::Options(sync)),
            hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
            definition_provider: Some(lsp_types::OneOf::Left(true)),
            references_provider: Some(lsp_types::OneOf::Left(true)),
            completion_provider: Some(lsp_types::CompletionOptions::default()),
            rename_provider: Some(lsp_types::OneOf::Left(true)),
            semantic_tokens_provider: Some(
                SemanticTokensServerCapabilities::SemanticTokensOptions(sem),
            ),
            ..Default::default()
        },
        server_info: Some(ServerInfo {
            name: "wright-lsp".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
        }),
    }
}

fn read_message(reader: &mut impl BufRead) -> Result<String, String> {
    let mut content_length = None;
    let mut header = String::new();
    loop {
        header.clear();
        if reader.read_line(&mut header).map_err(|e| e.to_string())? == 0 {
            return Err("stdin closed".to_string());
        }
        let trimmed = header.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = value.trim().parse::<usize>().ok();
        }
    }
    let length = content_length.ok_or_else(|| "missing Content-Length header".to_string())?;
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    String::from_utf8(body).map_err(|e| e.to_string())
}

fn write_response(
    writer: &mut impl Write,
    id: Option<Value>,
    result: impl serde::Serialize,
) -> Result<(), String> {
    send_payload(
        writer,
        serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }),
    )
}

fn write_error(
    writer: &mut impl Write,
    id: Option<Value>,
    code: i64,
    message: &str,
) -> Result<(), String> {
    send_payload(
        writer,
        serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
    )
}

fn send_payload(writer: &mut impl Write, payload: Value) -> Result<(), String> {
    let body = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body)
        .and_then(|_| writer.flush())
        .map_err(|e| e.to_string())
}

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
    by_source.entry(publication_uri(root_uri)).or_default();

    let previous = ownership.remove(root_uri).unwrap_or_default();
    let current: BTreeSet<String> = by_source.keys().cloned().collect();

    for (source, diags) in by_source {
        publish_to(writer, &source, source_version(service, &source), diags)?;
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

fn other_roots_own(ownership: &PublicationOwnership, source: &str) -> bool {
    ownership.values().any(|owned| owned.contains(source))
}

fn publication_uri(source: &str) -> String {
    wright_language::document::source_to_uri(source).unwrap_or_else(|| fallback_uri().to_string())
}

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
    send_payload(
        writer,
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": PublishDiagnostics::METHOD,
            "params": params,
        }),
    )
}

fn source_version(service: &LanguageService, source: &str) -> Option<i32> {
    if let Some(document) = service.store.document(source) {
        return Some(document.version);
    }
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

#[allow(deprecated)]
fn initialize_root(params: &InitializeParams) -> Option<std::path::PathBuf> {
    params
        .root_uri
        .as_ref()
        .and_then(|u| uri_to_path(u.as_str()))
        .or_else(|| {
            params
                .workspace_folders
                .as_ref()?
                .first()
                .and_then(|f| uri_to_path(f.uri.as_str()))
        })
}

fn uri_to_path(uri: &str) -> Option<std::path::PathBuf> {
    let path = wright_language::document::uri_to_path(uri)?;
    (!path.as_os_str().is_empty() && path != std::path::Path::new("/")).then_some(path)
}

fn source_to_uri(source: &str) -> Uri {
    wright_language::document::source_to_uri(source)
        .and_then(|uri| Uri::from_str(&uri).ok())
        .unwrap_or_else(fallback_uri)
}

fn fallback_uri() -> Uri {
    Uri::from_str("file:///unknown").expect("static URI parses")
}

fn convert_range(range: Range) -> LspRange {
    LspRange::new(
        LspPosition::new(range.start.line, range.start.character),
        LspPosition::new(range.end.line, range.end.character),
    )
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

fn encode_semantic_tokens(tokens: &[WtToken]) -> Vec<lsp_types::SemanticToken> {
    let mut data = Vec::new();
    let (mut prev_line, mut prev_char) = (0u32, 0u32);
    for t in tokens {
        let line_delta = t.line.saturating_sub(prev_line);
        let char_delta = if line_delta == 0 {
            t.character.saturating_sub(prev_char)
        } else {
            t.character
        };
        let token_type = match t.token_type.as_str() {
            "keyword" => 0,
            "variable" => 1,
            "string" => 3,
            "number" => 4,
            "operator" => 5,
            "macro" => 6,
            "attribute" => 7,
            _ => 2,
        };
        data.push(lsp_types::SemanticToken {
            delta_line: line_delta,
            delta_start: char_delta,
            length: t.length,
            token_type,
            token_modifiers_bitset: 0,
        });
        prev_line = t.line;
        prev_char = t.character;
    }
    data
}
