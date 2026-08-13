//! `wright-lsp` — the Wright language server (M10, issue #67).
//!
//! A thin LSP protocol adapter over [`wright_language::LanguageService`]:
//! all semantic logic lives in the editor-neutral service crate; this
//! binary only maps LSP DTOs to and from it, handles the stdio
//! Content-Length framing, and manages document lifecycle with correct
//! version identity. No duplicate semantic logic exists here.

use std::io::{BufRead, Write};
use std::str::FromStr;

use lsp_types::notification::{Notification, PublishDiagnostics};
use lsp_types::{
    CompletionItem as LspCompletionItem, CompletionItemKind, CompletionParams, CompletionResponse,
    Diagnostic as LspDiagnostic, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, GotoDefinitionParams,
    GotoDefinitionResponse, Hover as LspHover, HoverContents, InitializeResult, Location,
    MarkupContent, MarkupKind, Position as LspPosition, PositionEncodingKind,
    PublishDiagnosticsParams, Range as LspRange, ReferenceParams, RenameParams, SemanticTokens,
    SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensParams,
    SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo, TextDocumentPositionParams,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions, TextEdit, Uri,
    WorkDoneProgressOptions, WorkspaceEdit,
};
use serde_json::Value;

use wright_language::document::{Document, Position, Range};
use wright_language::{LanguageService, SemanticToken as WtToken};

fn main() {
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

    // The language service uses the process working directory as the root.
    let root = std::env::current_dir().map_err(|error| error.to_string())?;
    let mut service = LanguageService::new(root);

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
                    std::env::current_dir().map_err(|e| e.to_string())?,
                    params.text_document.version,
                );
                service.store.open(document);
                publish_diagnostics(&mut writer, &mut service, &uri)?;
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
                publish_diagnostics(&mut writer, &mut service, &uri)?;
            }
            "textDocument/didClose" => {
                let params: DidCloseTextDocumentParams =
                    serde_json::from_value(params.unwrap()).map_err(|error| error.to_string())?;
                service.store.close(&params.text_document.uri.to_string());
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
                    .map(|range| {
                        GotoDefinitionResponse::Scalar(Location {
                            uri: params
                                .text_document_position_params
                                .text_document
                                .uri
                                .clone(),
                            range: convert_range(range),
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
                    .map(|range| Location {
                        uri: uri.clone(),
                        range: convert_range(range),
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
                match service.rename(&uri.to_string(), position, &params.new_name) {
                    Some(rename) if rename.ok => {
                        let range = rename
                            .range
                            .map(convert_range)
                            .expect("rename result always carries a full-document range");
                        let preview = rename.preview.unwrap_or_default();
                        let edit = WorkspaceEdit {
                            changes: Some(
                                [(
                                    uri.clone(),
                                    vec![TextEdit {
                                        range,
                                        new_text: preview,
                                    }],
                                )]
                                .into_iter()
                                .collect(),
                            ),
                            ..Default::default()
                        };
                        write_response(&mut writer, id, serde_json::to_value(edit).unwrap())?;
                    }
                    _ => {
                        write_error(
                            &mut writer,
                            id,
                            -32602,
                            "rename refused: no symbol resolvable at the position or the edit failed validation",
                        )?;
                    }
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

/// Publish the diagnostics of a document as an LSP notification.
fn publish_diagnostics(
    writer: &mut impl Write,
    service: &mut LanguageService,
    uri: &str,
) -> Result<(), String> {
    let url = Uri::from_str(uri).map_err(|error| error.to_string())?;
    let diagnostics: Vec<LspDiagnostic> = service
        .diagnostics(uri)
        .into_iter()
        .map(|diagnostic| LspDiagnostic {
            range: convert_range(diagnostic.range),
            severity: Some(match diagnostic.severity.as_str() {
                "error" => DiagnosticSeverity::ERROR,
                "warning" => DiagnosticSeverity::WARNING,
                _ => DiagnosticSeverity::INFORMATION,
            }),
            code: Some(lsp_types::NumberOrString::String(diagnostic.code)),
            message: diagnostic.message,
            ..Default::default()
        })
        .collect();
    let params = PublishDiagnosticsParams {
        uri: url,
        diagnostics,
        version: service
            .store
            .document(uri)
            .map(|document| document.version as i32),
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

fn convert_position(position: LspPosition) -> Position {
    Position {
        line: position.line,
        character: position.character,
    }
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
