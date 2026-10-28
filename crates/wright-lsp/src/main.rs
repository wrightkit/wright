use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::str::FromStr;

use lsp_types::notification::{Notification, PublishDiagnostics};
use lsp_types::{
    AnnotatedTextEdit, Diagnostic as LspDiagnostic, DiagnosticSeverity,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentChanges, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, Location, MarkupContent,
    MarkupKind, OneOf, OptionalVersionedTextDocumentIdentifier, Position as LspPosition,
    PositionEncodingKind, PublishDiagnosticsParams, Range as LspRange, ReferenceParams,
    RenameParams, ServerCapabilities, ServerInfo, SymbolKind, TextDocumentEdit,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions, TextEdit, Uri,
    WorkspaceEdit,
};
use serde_json::Value;

use wright_language::document::{Document, Position, Range};
use wright_language::service::{RenameOutcome, SourceSymbol, SourceTextEdit};
use wright_language::{LanguageService, OpyProviderConfig, OpyProviderSupport, SessionConfig};

type PublicationOwnership = BTreeMap<String, BTreeSet<String>>;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut opy_provider: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("wright-lsp {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--opy-provider" => {
                let Some(path) = args.next() else {
                    eprintln!("wright-lsp: --opy-provider requires a PATH value");
                    std::process::exit(2);
                };
                opy_provider = Some(PathBuf::from(path));
            }
            _ => {}
        }
    }
    match run(opy_provider) {
        Ok(code) => std::process::exit(i32::from(code)),
        Err(message) => {
            eprintln!("wright-lsp: {message}");
            std::process::exit(1);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    /// Before `initialize`: requests are refused `-32002`.
    Waiting,
    /// Between `initialize` and `shutdown`: the full surface is served.
    Running,
    /// After `shutdown`: requests are refused `-32600`; `exit` ends 0.
    Shutdown,
}

fn run(opy_provider: Option<PathBuf>) -> Result<u8, String> {
    let mut reader = std::io::stdin().lock();
    let mut writer = std::io::stdout().lock();

    let config = SessionConfig {
        opy_provider: OpyProviderConfig {
            executable: opy_provider,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut root = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut service = LanguageService::with_config(root.clone(), config.clone());
    let mut ownership: PublicationOwnership = BTreeMap::new();
    let mut lifecycle = Lifecycle::Waiting;
    let mut versioned_workspace_edits = false;

    loop {
        let message = match read_message(&mut reader)? {
            Frame::Message(message) => message,
            Frame::Closed => return Ok(0),
            Frame::Malformed(code, reason) => {
                write_error(&mut writer, None, code, reason, "malformed-frame")?;
                continue;
            }
        };
        let value: Value = match serde_json::from_str(&message) {
            Ok(value) => value,
            Err(error) => {
                write_error(&mut writer, None, -32700, &error.to_string(), "parse-error")?;
                continue;
            }
        };
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let id = value.get("id").cloned();
        let params = value.get("params").cloned();

        if method.is_empty() {
            continue;
        }
        if !matches!(method.as_str(), "initialize" | "shutdown" | "exit")
            && lifecycle != Lifecycle::Running
        {
            let (code, reason, refusal) = match lifecycle {
                Lifecycle::Waiting => (-32002, "the server is not initialized", "not-initialized"),
                Lifecycle::Shutdown => (-32600, "the server is shut down", "invalid-request"),
                Lifecycle::Running => unreachable!("checked above"),
            };
            reply_error(&mut writer, id, code, reason, refusal)?;
            continue;
        }

        match method.as_str() {
            "initialize" => {
                if lifecycle != Lifecycle::Waiting {
                    reply_error(
                        &mut writer,
                        id,
                        -32600,
                        "the server is already initialized",
                        "already-initialized",
                    )?;
                    continue;
                }
                let Some(init) = read_params::<InitializeParams>(&mut writer, &id, params)? else {
                    continue;
                };
                versioned_workspace_edits = init
                    .capabilities
                    .workspace
                    .as_ref()
                    .and_then(|workspace| workspace.workspace_edit.as_ref())
                    .and_then(|edit| edit.document_changes)
                    .unwrap_or(false);
                if let Some(resolved) = initialize_root(&init) {
                    root = resolved;
                    service = LanguageService::with_config(root.clone(), config.clone());
                    ownership.clear();
                }
                lifecycle = Lifecycle::Running;
                let opy_support = service.opy_provider_support();
                write_response(
                    &mut writer,
                    id,
                    serde_json::to_value(initialize_result(versioned_workspace_edits, opy_support))
                        .unwrap(),
                )?;
            }
            "initialized" => {
                write_response(&mut writer, id, Value::Null)?;
            }
            "shutdown" => {
                match lifecycle {
                    Lifecycle::Running => lifecycle = Lifecycle::Shutdown,
                    Lifecycle::Waiting => {
                        reply_error(
                            &mut writer,
                            id,
                            -32002,
                            "the server is not initialized",
                            "not-initialized",
                        )?;
                        continue;
                    }
                    Lifecycle::Shutdown => {}
                }
                write_response(&mut writer, id, Value::Null)?;
            }
            "exit" => return Ok(u8::from(lifecycle != Lifecycle::Shutdown)),
            "textDocument/didOpen" => {
                let Some(params) =
                    read_params::<DidOpenTextDocumentParams>(&mut writer, &id, params)?
                else {
                    continue;
                };
                let uri = params.text_document.uri.to_string();
                let document = Document::with_version(
                    uri.clone(),
                    params.text_document.text,
                    root.clone(),
                    params.text_document.version,
                )
                .with_language_id(params.text_document.language_id);
                service.store.open(document);
                publish_affected_diagnostics(&mut writer, &mut service, &mut ownership, &uri)?;
                write_response(&mut writer, id, Value::Null)?;
            }
            "textDocument/didChange" => {
                let Some(params) =
                    read_params::<DidChangeTextDocumentParams>(&mut writer, &id, params)?
                else {
                    continue;
                };
                let uri = params.text_document.uri.to_string();
                let applied = params
                    .content_changes
                    .iter()
                    .rev()
                    .find(|change| change.range.is_none())
                    .is_some_and(|change| {
                        service
                            .store
                            .change(&uri, &change.text, params.text_document.version)
                    });
                if applied {
                    publish_affected_diagnostics(&mut writer, &mut service, &mut ownership, &uri)?;
                }
                write_response(&mut writer, id, Value::Null)?;
            }
            "textDocument/didClose" => {
                let Some(params) =
                    read_params::<DidCloseTextDocumentParams>(&mut writer, &id, params)?
                else {
                    continue;
                };
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
                publish_affected_diagnostics(&mut writer, &mut service, &mut ownership, &uri)?;
                write_response(&mut writer, id, Value::Null)?;
            }
            "textDocument/didSave" => {
                write_response(&mut writer, id, Value::Null)?;
            }
            "textDocument/hover" => {
                let Some(params) = read_params::<HoverParams>(&mut writer, &id, params)? else {
                    continue;
                };
                if id.is_none() {
                    continue;
                }
                let position_params = params.text_document_position_params;
                let hover = service
                    .hover(
                        position_params.text_document.uri.as_str(),
                        Position {
                            line: position_params.position.line,
                            character: position_params.position.character,
                        },
                    )
                    .map(|info| Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: info.contents,
                        }),
                        range: Some(convert_range(info.range)),
                    });
                write_response(&mut writer, id, serde_json::to_value(hover).unwrap())?;
            }
            "textDocument/definition" => {
                let Some(params) = read_params::<GotoDefinitionParams>(&mut writer, &id, params)?
                else {
                    continue;
                };
                if id.is_none() {
                    continue;
                }
                let position_params = params.text_document_position_params;
                let definition = service
                    .definition(
                        position_params.text_document.uri.as_str(),
                        Position {
                            line: position_params.position.line,
                            character: position_params.position.character,
                        },
                    )
                    .map(|locations| {
                        GotoDefinitionResponse::Array(
                            locations
                                .into_iter()
                                .map(|location| Location {
                                    uri: Uri::from_str(&location.uri)
                                        .unwrap_or_else(|_| fallback_uri()),
                                    range: convert_range(location.range),
                                })
                                .collect(),
                        )
                    });
                write_response(&mut writer, id, serde_json::to_value(definition).unwrap())?;
            }
            "textDocument/references" => {
                let Some(params) = read_params::<ReferenceParams>(&mut writer, &id, params)? else {
                    continue;
                };
                if id.is_none() {
                    continue;
                }
                let position_params = params.text_document_position;
                let locations = service.references(
                    position_params.text_document.uri.as_str(),
                    Position {
                        line: position_params.position.line,
                        character: position_params.position.character,
                    },
                    params.context.include_declaration,
                );
                let locations = locations.map(|locations| {
                    locations
                        .into_iter()
                        .map(|location| Location {
                            uri: Uri::from_str(&location.uri).unwrap_or_else(|_| fallback_uri()),
                            range: convert_range(location.range),
                        })
                        .collect::<Vec<_>>()
                });
                write_response(&mut writer, id, serde_json::to_value(locations).unwrap())?;
            }
            "textDocument/documentSymbol" => {
                let Some(params) = read_params::<DocumentSymbolParams>(&mut writer, &id, params)?
                else {
                    continue;
                };
                if id.is_none() {
                    continue;
                }
                let symbols = service
                    .document_symbols(params.text_document.uri.as_str())
                    .map(|symbols| {
                        DocumentSymbolResponse::Nested(
                            symbols.iter().map(lsp_document_symbol).collect(),
                        )
                    });
                write_response(&mut writer, id, serde_json::to_value(symbols).unwrap())?;
            }
            "textDocument/rename" => {
                let Some(params) = read_params::<RenameParams>(&mut writer, &id, params)? else {
                    continue;
                };
                if id.is_none() {
                    continue;
                }
                if !versioned_workspace_edits {
                    // `WorkspaceEdit.changes` cannot carry the document
                    // version the provider validated; serving it would let a
                    // client apply the edit to a buffer that moved while the
                    // rename was pending without ever detecting it.
                    reply_error(
                        &mut writer,
                        id,
                        -32803,
                        "rename requires versioned workspace edits \
                         (workspace.workspaceEdit.documentChanges is not negotiated); \
                         the request is refused rather than risk a stale buffer",
                        "rename-unversioned-workspace-edit",
                    )?;
                    continue;
                }
                let outcome = service.rename(
                    params.text_document_position.text_document.uri.as_str(),
                    Position {
                        line: params.text_document_position.position.line,
                        character: params.text_document_position.position.character,
                    },
                    &params.new_name,
                );
                match outcome {
                    RenameOutcome::Applied(edits) => match versioned_workspace_edit(edits) {
                        Ok(edit) => {
                            write_response(&mut writer, id, serde_json::to_value(edit).unwrap())?
                        }
                        Err(uri) => reply_error(
                            &mut writer,
                            id,
                            -32803,
                            &format!(
                                "the provider produced an edit against an unparseable URI {uri}"
                            ),
                            "rename-edit-malformed-uri",
                        )?,
                    },
                    RenameOutcome::Refused { code, message } => {
                        reply_error(&mut writer, id, -32803, &message, &code)?;
                    }
                }
            }
            _ => {
                write_response(&mut writer, id, Value::Null)?;
            }
        }
    }
}

fn versioned_workspace_edit(edits: Vec<SourceTextEdit>) -> Result<WorkspaceEdit, String> {
    let mut grouped: BTreeMap<String, (i32, Vec<OneOf<TextEdit, AnnotatedTextEdit>>)> =
        BTreeMap::new();
    for edit in edits {
        let version = edit.version;
        grouped
            .entry(edit.uri)
            .or_insert_with(|| (version, Vec::new()))
            .1
            .push(OneOf::Left(TextEdit {
                range: convert_range(edit.range),
                new_text: edit.new_text,
            }));
    }
    let document_edits = grouped
        .into_iter()
        .map(|(uri, (version, edits))| {
            let parsed = Uri::from_str(&uri).map_err(|_| uri)?;
            Ok(TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier::new(parsed, version),
                edits,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(WorkspaceEdit {
        document_changes: Some(DocumentChanges::Edits(document_edits)),
        ..Default::default()
    })
}

fn initialize_result(
    versioned_workspace_edits: bool,
    opy_support: OpyProviderSupport,
) -> InitializeResult {
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
            rename_provider: versioned_workspace_edits.then_some(OneOf::Left(true)),
            hover_provider: Some(HoverProviderCapability::Simple(true)),
            definition_provider: Some(OneOf::Left(true)),
            references_provider: Some(OneOf::Left(true)),
            document_symbol_provider: opy_support.symbols.then_some(OneOf::Left(true)),
            ..Default::default()
        },
        server_info: Some(ServerInfo {
            name: "wright-lsp".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
        }),
    }
}

#[allow(deprecated)]
fn lsp_document_symbol(symbol: &SourceSymbol) -> DocumentSymbol {
    let range = convert_range(symbol.range);
    DocumentSymbol {
        name: symbol.name.clone(),
        detail: None,
        kind: symbol_kind(symbol.kind.as_str()),
        tags: None,
        deprecated: None,
        range,
        selection_range: symbol.selection_range.map(convert_range).unwrap_or(range),
        children: None,
    }
}

fn symbol_kind(kind: &str) -> SymbolKind {
    match kind {
        "constant" => SymbolKind::CONSTANT,
        "subroutine" | "def" | "macro" => SymbolKind::FUNCTION,
        _ => SymbolKind::VARIABLE,
    }
}

enum Frame {
    /// A complete, decodable frame body.
    Message(String),
    /// The stream hit EOF before a header line — the client is gone.
    Closed,
    /// The frame was structurally invalid; carries the JSON-RPC error code
    /// and message to answer with.
    Malformed(i64, &'static str),
}

const MAX_HEADER_LINE: usize = 8 * 1024;
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_CONTENT_LENGTH: u64 = 32 * 1024 * 1024;

fn read_message(reader: &mut impl BufRead) -> Result<Frame, String> {
    let mut content_length = None;
    let mut invalid: Option<&'static str> = None;
    let mut header_bytes = 0usize;
    loop {
        let mut raw = Vec::new();
        let mut read = read_bounded(reader, &mut raw)?;
        if read == 0 {
            return Ok(Frame::Closed);
        }
        header_bytes += read;
        while !raw.ends_with(b"\n") {
            raw.clear();
            read = read_bounded(reader, &mut raw)?;
            if read == 0 {
                return Ok(Frame::Closed);
            }
            header_bytes += read;
            invalid.get_or_insert("oversized header line");
        }
        if header_bytes > MAX_HEADER_BYTES {
            invalid.get_or_insert("oversized header section");
        }
        let Ok(header) = std::str::from_utf8(&raw) else {
            invalid.get_or_insert("non-UTF-8 header line");
            continue;
        };
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some(value) = header.strip_prefix("Content-Length:") {
            content_length = value.trim().parse::<u64>().ok();
            if content_length.is_none() {
                invalid.get_or_insert("invalid Content-Length");
            }
        }
    }
    let Some(length) = content_length else {
        return Ok(Frame::Malformed(
            -32600,
            invalid.unwrap_or("missing Content-Length header"),
        ));
    };
    let malformed =
        invalid.or((length > MAX_CONTENT_LENGTH).then_some("Content-Length exceeds limit"));
    if let Some(reason) = malformed {
        if length <= MAX_CONTENT_LENGTH {
            discard_bytes(reader, length)?;
        }
        return Ok(Frame::Malformed(-32600, reason));
    }
    let mut body = vec![0u8; length as usize];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    match String::from_utf8(body) {
        Ok(text) => Ok(Frame::Message(text)),
        Err(_) => Ok(Frame::Malformed(-32700, "frame body is not UTF-8")),
    }
}

fn read_bounded(reader: &mut impl BufRead, buf: &mut Vec<u8>) -> Result<usize, String> {
    reader
        .by_ref()
        .take((MAX_HEADER_LINE + 1) as u64)
        .read_until(b'\n', buf)
        .map_err(|e| e.to_string())
}

fn discard_bytes(reader: &mut impl BufRead, length: u64) -> Result<(), String> {
    std::io::copy(&mut reader.by_ref().take(length), &mut std::io::sink())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn write_msg(writer: &mut impl Write, val: Value) -> Result<(), String> {
    let body = serde_json::to_string(&val).map_err(|e| e.to_string())?;
    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body)
        .and_then(|_| writer.flush())
        .map_err(|e| e.to_string())
}

/// Answers a request; a notification (no `id`) gets no response.
fn write_response(writer: &mut impl Write, id: Option<Value>, result: Value) -> Result<(), String> {
    if id.is_none() {
        return Ok(());
    }
    write_msg(
        writer,
        serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }),
    )
}

/// Refuses a request; a notification (no `id`) gets no response.
fn reply_error(
    writer: &mut impl Write,
    id: Option<Value>,
    code: i64,
    message: &str,
    refusal_code: &str,
) -> Result<(), String> {
    if id.is_none() {
        return Ok(());
    }
    write_error(writer, id, code, message, refusal_code)
}

/// Writes an error even without an `id`, as a malformed frame requires.
fn write_error(
    writer: &mut impl Write,
    id: Option<Value>,
    code: i64,
    message: &str,
    refusal_code: &str,
) -> Result<(), String> {
    write_msg(
        writer,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message, "data": { "code": refusal_code } },
        }),
    )
}

fn parse_params<T: serde::de::DeserializeOwned>(params: Option<Value>) -> Result<T, String> {
    serde_json::from_value(params.ok_or_else(|| "missing params".to_string())?)
        .map_err(|error| error.to_string())
}

fn read_params<T: serde::de::DeserializeOwned>(
    writer: &mut impl Write,
    id: &Option<Value>,
    params: Option<Value>,
) -> Result<Option<T>, String> {
    match parse_params(params) {
        Ok(parsed) => Ok(Some(parsed)),
        Err(message) => {
            reply_error(writer, id.clone(), -32602, &message, "invalid-params")?;
            Ok(None)
        }
    }
}

fn publish_affected_diagnostics(
    writer: &mut impl Write,
    service: &mut LanguageService,
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
    service: &mut LanguageService,
    ownership: &mut PublicationOwnership,
    root_uri: &str,
) -> Result<(), String> {
    let diagnostics = service.diagnostics(root_uri);
    let mut by_source: BTreeMap<String, Vec<LspDiagnostic>> = BTreeMap::new();
    for diagnostic in diagnostics {
        if let Some(uri) = publication_uri(service, &diagnostic.source) {
            by_source
                .entry(uri)
                .or_default()
                .push(convert_diagnostic(diagnostic));
        }
    }
    if let Some(uri) = publication_uri(service, root_uri) {
        by_source.entry(uri).or_default();
    }

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

/// The URI a diagnostic is published under (#637): the store's own
/// spelling for an open document — the client's raw URI, e.g.
/// `file://localhost/x` — before any normalization, since the client
/// associates diagnostics with the URI it sent; then a normalized file
/// URI for anything else that resolves to one. Sources with no file URI
/// yield `None` rather than a fabricated `file:///unknown`.
fn publication_uri(service: &LanguageService, source: &str) -> Option<String> {
    if let Some(document) = document_for_source(service, source) {
        return Some(document.uri.clone());
    }
    wright_language::document::source_to_uri(source)
}

fn convert_diagnostic(diagnostic: wright_language::SourceDiagnostic) -> LspDiagnostic {
    LspDiagnostic {
        range: convert_range(diagnostic.range),
        severity: Some(match diagnostic.severity {
            wright_language::DiagnosticSeverity::Error => DiagnosticSeverity::ERROR,
            wright_language::DiagnosticSeverity::Warning => DiagnosticSeverity::WARNING,
            wright_language::DiagnosticSeverity::Hint => DiagnosticSeverity::HINT,
            wright_language::DiagnosticSeverity::Info => DiagnosticSeverity::INFORMATION,
        }),
        code: (!diagnostic.code.is_empty())
            .then_some(lsp_types::NumberOrString::String(diagnostic.code)),
        message: diagnostic.message,
        ..Default::default()
    }
}

fn publish_to(
    writer: &mut impl Write,
    uri: &str,
    version: Option<i32>,
    diagnostics: Vec<LspDiagnostic>,
) -> Result<(), String> {
    let Ok(uri) = Uri::from_str(uri) else {
        return Ok(());
    };
    let params = PublishDiagnosticsParams {
        uri,
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
    document_for_source(service, source).map(|document| document.version)
}

fn document_for_source<'s>(service: &'s LanguageService, source: &str) -> Option<&'s Document> {
    if let Some(document) = service.store.document(source) {
        return Some(document);
    }
    let path =
        wright_language::document::uri_to_path(source).unwrap_or_else(|| PathBuf::from(source));
    service
        .store
        .uri_for_path(&path)
        .and_then(|uri| service.store.document(&uri))
}

#[allow(deprecated)]
fn initialize_root(params: &InitializeParams) -> Option<PathBuf> {
    if let Some(uri) = &params.root_uri
        && let Some(path) = uri_to_path(uri.as_str())
    {
        return Some(path);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(uri: &str, version: i32, line: u32, new_text: &str) -> SourceTextEdit {
        let at = |character| Position { line, character };
        SourceTextEdit {
            uri: uri.to_string(),
            range: Range {
                start: at(10),
                end: at(15),
            },
            new_text: new_text.to_string(),
            version,
        }
    }

    #[test]
    fn rename_edits_are_grouped_per_document_with_their_validated_version() {
        let edits = vec![
            edit("file:///w/a.opy", 3, 0, "vault"),
            edit("file:///w/b.opy", 7, 1, "vault"),
            edit("file:///w/a.opy", 3, 4, "vault"),
        ];
        let value = serde_json::to_value(versioned_workspace_edit(edits).unwrap()).unwrap();
        assert!(value.get("changes").is_none(), "unversioned map: {value}");
        let changes = value["documentChanges"].as_array().unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0]["textDocument"]["uri"], "file:///w/a.opy");
        assert_eq!(changes[0]["textDocument"]["version"], 3);
        assert_eq!(changes[0]["edits"].as_array().unwrap().len(), 2);
        assert_eq!(changes[1]["textDocument"]["uri"], "file:///w/b.opy");
        assert_eq!(changes[1]["textDocument"]["version"], 7);
        assert_eq!(changes[1]["edits"][0]["newText"], "vault");
        assert_eq!(changes[1]["edits"][0]["range"]["start"]["character"], 10);
    }

    #[test]
    fn an_unparseable_edit_uri_refuses_the_whole_edit() {
        let edits = vec![
            edit("file:///w/a.opy", 1, 0, "vault"),
            edit("not a uri", 1, 0, "vault"),
        ];
        assert_eq!(versioned_workspace_edit(edits).unwrap_err(), "not a uri");
    }
}
