//! A thin LSP protocol adapter over [`wright_language::LanguageService`].

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::str::FromStr;

use lsp_types::notification::{Notification, PublishDiagnostics};
use lsp_types::{
    Diagnostic as LspDiagnostic, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, InitializeParams, InitializeResult,
    Position as LspPosition, PositionEncodingKind, PublishDiagnosticsParams, Range as LspRange,
    ServerCapabilities, ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, Uri,
};
use serde_json::Value;

use wright_language::LanguageService;
use wright_language::document::{Document, Range};

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
    let mut reader = std::io::stdin().lock();
    let mut writer = std::io::stdout().lock();

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
                write_response(
                    &mut writer,
                    id,
                    serde_json::to_value(initialize_result()).unwrap(),
                )?;
            }
            "initialized" => {}
            "shutdown" => write_response(&mut writer, id, Value::Null)?,
            "exit" => break,
            "textDocument/didOpen" => {
                let params: DidOpenTextDocumentParams = parse_params(params)?;
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
                let params: DidChangeTextDocumentParams = parse_params(params)?;
                let uri = params.text_document.uri.to_string();
                if let Some(change) = params.content_changes.last() {
                    service
                        .store
                        .change(&uri, &change.text, params.text_document.version);
                }
                publish_affected_diagnostics(&mut writer, &service, &mut ownership, &uri)?;
            }
            "textDocument/didClose" => {
                let params: DidCloseTextDocumentParams = parse_params(params)?;
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
            _ => {
                if id.is_some() {
                    write_response(&mut writer, id, Value::Null)?;
                }
            }
        }
    }
    Ok(())
}

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
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).map_err(|e| e.to_string())? == 0 {
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
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    String::from_utf8(body).map_err(|e| e.to_string())
}

fn write_msg(writer: &mut impl Write, val: Value) -> Result<(), String> {
    let body = serde_json::to_string(&val).map_err(|e| e.to_string())?;
    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body)
        .and_then(|_| writer.flush())
        .map_err(|e| e.to_string())
}

fn write_response(writer: &mut impl Write, id: Option<Value>, result: Value) -> Result<(), String> {
    write_msg(
        writer,
        serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }),
    )
}

fn parse_params<T: serde::de::DeserializeOwned>(params: Option<Value>) -> Result<T, String> {
    serde_json::from_value(params.unwrap()).map_err(|error| error.to_string())
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
        uri: source_to_uri(source),
        diagnostics,
        version,
    };
    let notification = serde_json::json!({
        "jsonrpc": "2.0",
        "method": PublishDiagnostics::METHOD,
        "params": serde_json::to_value(params).unwrap(),
    });
    write_msg(writer, notification)
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
        .map(|d| d.version)
}

#[allow(deprecated)]
fn initialize_root(params: &InitializeParams) -> Option<PathBuf> {
    if let Some(uri) = &params.root_uri {
        if let Some(path) = uri_to_path(uri.as_str()) {
            return Some(path);
        }
    }
    params
        .workspace_folders
        .as_ref()
        .and_then(|f| f.first())
        .and_then(|f| uri_to_path(f.uri.as_str()))
}

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let path = wright_language::document::uri_to_path(uri)?;
    if path.as_os_str().is_empty() || path == std::path::Path::new("/") {
        return None;
    }
    Some(path)
}

fn source_to_uri(source: &str) -> Uri {
    Uri::from_str(&publication_uri(source)).unwrap_or_else(|_| fallback_uri())
}

fn fallback_uri() -> Uri {
    Uri::from_str("file:///unknown").unwrap()
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
