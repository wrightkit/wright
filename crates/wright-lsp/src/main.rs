//! A thin LSP protocol adapter over [`wright_language::LanguageService`].

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::str::FromStr;

use lsp_types::notification::{Notification, PublishDiagnostics};
use lsp_types::{
    AnnotatedTextEdit, Diagnostic as LspDiagnostic, DiagnosticSeverity,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentChanges, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, HoverProviderCapability, InitializeParams, InitializeResult, Location,
    MarkupContent, MarkupKind, OneOf, OptionalVersionedTextDocumentIdentifier,
    Position as LspPosition, PositionEncodingKind, PublishDiagnosticsParams, Range as LspRange,
    ReferenceParams, RenameParams, ServerCapabilities, ServerInfo, TextDocumentEdit,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions, TextEdit, Uri,
    WorkspaceEdit,
};
use serde_json::Value;

use wright_language::document::{Document, Position, Range};
use wright_language::service::{RenameOutcome, SourceTextEdit};
use wright_language::{LanguageService, OpyProviderConfig, SessionConfig};

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
    if let Err(message) = run(opy_provider) {
        eprintln!("wright-lsp: {message}");
        std::process::exit(1);
    }
}

fn run(opy_provider: Option<PathBuf>) -> Result<(), String> {
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
    // Whether the client negotiated versioned workspace edits
    // (`workspace.workspaceEdit.documentChanges`). Renames are only served
    // when they can carry the validated document version.
    let mut versioned_workspace_edits = false;

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
                let init = params
                    .and_then(|params| serde_json::from_value::<InitializeParams>(params).ok());
                versioned_workspace_edits = init
                    .as_ref()
                    .and_then(|init| {
                        init.capabilities
                            .workspace
                            .as_ref()?
                            .workspace_edit
                            .as_ref()?
                            .document_changes
                    })
                    .unwrap_or(false);
                if let Some(init) = init {
                    if let Some(resolved) = initialize_root(&init) {
                        root = resolved;
                        service = LanguageService::with_config(root.clone(), config.clone());
                        ownership.clear();
                    }
                }
                write_response(
                    &mut writer,
                    id,
                    serde_json::to_value(initialize_result(versioned_workspace_edits)).unwrap(),
                )?;
            }
            "initialized" => {}
            "shutdown" => write_response(&mut writer, id, Value::Null)?,
            "exit" => break,
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
                publish_affected_diagnostics(&mut writer, &service, &mut ownership, &uri)?;
            }
            "textDocument/didChange" => {
                let Some(params) =
                    read_params::<DidChangeTextDocumentParams>(&mut writer, &id, params)?
                else {
                    continue;
                };
                let uri = params.text_document.uri.to_string();
                if let Some(change) = params.content_changes.last() {
                    service
                        .store
                        .change(&uri, &change.text, params.text_document.version);
                }
                publish_affected_diagnostics(&mut writer, &service, &mut ownership, &uri)?;
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
                publish_affected_diagnostics(&mut writer, &service, &mut ownership, &uri)?;
            }
            "textDocument/didSave" => {}
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
                    .map(|location| {
                        GotoDefinitionResponse::Scalar(Location {
                            uri: Uri::from_str(&location.uri).unwrap_or_else(|_| fallback_uri()),
                            range: convert_range(location.range),
                        })
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
            "textDocument/rename" => {
                let Some(params) = read_params::<RenameParams>(&mut writer, &id, params)? else {
                    continue;
                };
                if id.is_none() {
                    continue; // a notification gets no response and no provider call
                }
                if !versioned_workspace_edits {
                    // `WorkspaceEdit.changes` cannot carry the document
                    // version the provider validated; serving it would let a
                    // client apply the edit to a buffer that moved while the
                    // rename was pending without ever detecting it.
                    write_error(
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
                        Err(uri) => write_error(
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
                        write_error(&mut writer, id, -32803, &message, &code)?;
                    }
                }
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

/// `documentChanges` carries the validated document version per file so the
/// client can reject the edit if its buffer moved while the rename was
/// pending. One `TextDocumentEdit` per document; a dropped edit would apply a
/// partial rename, so an unparseable URI is returned as the error.
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

fn initialize_result(versioned_workspace_edits: bool) -> InitializeResult {
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
            // Applied renames are returned as versioned `documentChanges`;
            // a client that cannot receive the validated document version
            // cannot be handed an edit safely, so rename stays unadvertised.
            rename_provider: versioned_workspace_edits.then_some(OneOf::Left(true)),
            // Raw Workshop hover/definition/references are backed by the
            // session's canonical check/index pipeline (#555); documents
            // outside that scope still answer `null`.
            hover_provider: Some(HoverProviderCapability::Simple(true)),
            definition_provider: Some(OneOf::Left(true)),
            references_provider: Some(OneOf::Left(true)),
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

/// A JSON-RPC error response that preserves the refusal's structured code in
/// `error.data.code` so clients can distinguish refusals without parsing the
/// message.
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

/// Params for a message, answering `-32602` when a request carries none or
/// malformed ones. `None` means the message was malformed and already handled —
/// or was a notification, which gets no response — so the caller skips it and
/// the server keeps serving.
fn read_params<T: serde::de::DeserializeOwned>(
    writer: &mut impl Write,
    id: &Option<Value>,
    params: Option<Value>,
) -> Result<Option<T>, String> {
    match parse_params(params) {
        Ok(parsed) => Ok(Some(parsed)),
        Err(message) => {
            if id.is_some() {
                write_error(writer, id.clone(), -32602, &message, "invalid-params")?;
            }
            Ok(None)
        }
    }
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

    #[test]
    fn rename_is_advertised_only_with_versioned_workspace_edits() {
        assert!(
            initialize_result(true)
                .capabilities
                .rename_provider
                .is_some()
        );
        assert!(
            initialize_result(false)
                .capabilities
                .rename_provider
                .is_none()
        );
    }
}
