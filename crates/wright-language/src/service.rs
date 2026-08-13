//! The editor-neutral language service (#63, #65, #66).
//!
//! Composes the native `.opy` frontend, the semantic index/analyzer, the M9
//! safe-edit contract, and the workshop catalog. Every result is tagged with
//! the document version it was computed for, so stale results are
//! deterministic and replaceable (#64). No LSP types appear here.

use serde::Serialize;
use wright_analyzer::analysis::{self, Finding};
use wright_analyzer::symbols::{SemanticIndex, Symbol};
use wright_ir::wir;

use crate::document::{Document, DocumentStore, Position, Range};

/// A language diagnostic (editor-neutral).
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: String,
    pub code: String,
    pub message: String,
    /// The document version this diagnostic was computed for.
    pub document_version: i32,
}

/// Hover content.
#[derive(Debug, Clone, Serialize)]
pub struct Hover {
    pub contents: String,
    pub range: Option<Range>,
    pub document_version: i32,
}

/// A completion item.
#[derive(Debug, Clone, Serialize)]
pub struct CompletionItem {
    pub label: String,
    pub kind: String,
    pub detail: Option<String>,
}

/// A semantic token.
#[derive(Debug, Clone, Serialize)]
pub struct SemanticToken {
    pub line: u32,
    pub character: u32,
    pub length: u32,
    pub token_type: String,
}

/// A source-aware location: a source/document identity plus a range.
#[derive(Debug, Clone, Serialize)]
pub struct SourceLocation {
    /// The source identity: the document URI for the main file, or the
    /// resolved path for an included file.
    pub source: String,
    pub range: Range,
}

/// The result of a rename request.
#[derive(Debug, Clone, Serialize)]
pub struct RenameResult {
    /// The current document version.
    pub document_version: i32,
    /// Whether the rename validates through the compiler pipeline.
    pub ok: bool,
    /// The previewed edited source text.
    pub preview: Option<String>,
    /// The applicable full-document replacement range.
    pub range: Option<Range>,
    /// Structured diagnostics from validation, when the edit was rejected.
    pub diagnostics: Vec<crate::document::Range>,
}

/// The analyzed state of one document.
pub struct Analysis {
    pub program: wir::Program,
    pub index: Option<SemanticIndex>,
    pub findings: Vec<Finding>,
    pub parse_errors: Vec<wright_opy::FrontendError>,
}

/// The editor-neutral language service over a workspace.
pub struct LanguageService {
    pub store: DocumentStore,
    /// The include root used for project compilation.
    pub root: std::path::PathBuf,
}

impl LanguageService {
    /// A service with the given project root.
    pub fn new(root: std::path::PathBuf) -> LanguageService {
        LanguageService {
            store: DocumentStore::new(root.clone()),
            root,
        }
    }

    /// Analyze one document: preprocess → parse → lower → semantic index.
    /// Open-document overlays (unsaved editor buffers) participate in include
    /// resolution before the filesystem.
    pub fn analyze(&self, document: &Document) -> Analysis {
        let mut parse_errors = Vec::new();
        let overlay = self.store.overlay(&self.root);
        let hir = match wright_opy::compile_with_overlay(
            &document.text,
            &document.uri,
            &self.root,
            &overlay,
        ) {
            Ok(hir) => hir,
            Err(error) => {
                parse_errors.push(error);
                return Analysis {
                    program: wir::Program::default(),
                    index: None,
                    findings: Vec::new(),
                    parse_errors,
                };
            }
        };
        let mut findings = Vec::new();
        let mut program = wir::Program::default();
        if let Ok(model) = hir.to_ir() {
            if let Ok(lowered) = wright_ir::lower::lower(&model) {
                program = lowered;
                findings = analysis::analyze(&program);
            }
        }
        let index = SemanticIndex::build(&program).ok();
        Analysis {
            program,
            index,
            findings,
            parse_errors,
        }
    }

    /// Diagnostics for a document: parse errors and semantic findings.
    pub fn diagnostics(&self, uri: &str) -> Vec<Diagnostic> {
        let Some(document) = self.store.document(uri) else {
            return Vec::new();
        };
        let analysis = self.analyze(document);
        let mut diagnostics = Vec::new();
        for error in &analysis.parse_errors {
            let range = error.span.map_or_else(empty_range, |span| {
                document.from_span(&wright_ir::source::Span::new(
                    wright_ir::ids::Id::from_index(span.file as usize),
                    wright_ir::source::Position::new(span.start.line, span.start.col),
                    wright_ir::source::Position::new(span.end.line, span.end.col),
                ))
            });
            diagnostics.push(Diagnostic {
                range,
                severity: "error".to_string(),
                code: error.code.clone(),
                message: error.message.clone(),
                document_version: document.version,
            });
        }
        for finding in &analysis.findings {
            let range = finding
                .span
                .map_or_else(empty_range, |span| document.from_span(&span));
            diagnostics.push(Diagnostic {
                range,
                severity: severity_name(finding.severity).to_string(),
                code: finding.code.to_string(),
                message: finding.message.clone(),
                document_version: document.version,
            });
        }
        diagnostics
    }

    /// Hover content for a symbol at a position.
    pub fn hover(&self, uri: &str, position: Position) -> Option<Hover> {
        let document = self.store.document(uri)?;
        let analysis = self.analyze(document);
        let symbol = self.symbol_at(&analysis, document, position)?;
        let usage = analysis.index.as_ref().map(|index| index.usage(symbol.id));
        let mut contents = format!("**{}** · {}", symbol.name, symbol_kind_name(symbol.kind));
        if let Some(usage) = usage {
            contents.push_str(&format!(
                "\nreads: {}, writes: {}, calls: {}, rules: {}",
                usage.reads, usage.writes, usage.calls, usage.rules
            ));
        }
        let range = symbol.span.map(|span| document.from_span(&span));
        Some(Hover {
            contents,
            range,
            document_version: document.version,
        })
    }

    /// The definition location of the symbol referenced at a position.
    ///
    /// Prefers the symbol's definition site (a `def` body for subroutines),
    /// falling back to its declaration. Returns a source-aware location:
    /// cross-file declarations carry their own source identity (the included
    /// file), not the requesting document.
    pub fn definition(&self, uri: &str, position: Position) -> Option<SourceLocation> {
        let document = self.store.document(uri)?;
        let analysis = self.analyze(document);
        let symbol = self.symbol_at(&analysis, document, position)?;
        let span = analysis
            .index
            .as_ref()
            .and_then(|index| {
                index
                    .references(symbol.id)
                    .iter()
                    .find(|reference| {
                        reference.kind == wright_analyzer::symbols::ReferenceKind::Definition
                    })
                    .and_then(|reference| reference.span)
            })
            .or(symbol.span)?;
        Some(self.source_location(&analysis.program, document, span))
    }

    /// Every reference location of the symbol at a position.
    ///
    /// Each location is source-aware and preserves the compiler's `span.file`
    /// provenance so references into included files point at the correct
    /// source identity.
    pub fn references(&self, uri: &str, position: Position) -> Vec<SourceLocation> {
        let Some(document) = self.store.document(uri) else {
            return Vec::new();
        };
        let analysis = self.analyze(document);
        let Some(symbol) = self.symbol_at(&analysis, document, position) else {
            return Vec::new();
        };
        analysis
            .index
            .as_ref()
            .map(|index| {
                index
                    .references(symbol.id)
                    .iter()
                    .filter_map(|reference| reference.span)
                    .map(|span| self.source_location(&analysis.program, document, span))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Map a compiler span to its source identity and 0-based range.
    fn source_location(
        &self,
        program: &wir::Program,
        document: &Document,
        span: wright_ir::source::Span,
    ) -> SourceLocation {
        let file_index = span.file.index();
        let source = self.source_identity(program, document, file_index);
        let source_text = if file_index == 0 {
            document.text.clone()
        } else {
            let path = std::path::PathBuf::from(&source);
            self.store.text_for_path(&path).unwrap_or_default()
        };
        SourceLocation {
            source,
            range: crate::document::span_to_range(&span, &source_text),
        }
    }

    /// The editor-neutral source identity for a compiler file index: the
    /// requesting document URI for the main file, or a resolved path for an
    /// included file (relative include paths resolve against the workspace
    /// root).
    fn source_identity(
        &self,
        program: &wir::Program,
        document: &Document,
        file_index: usize,
    ) -> String {
        if file_index == 0 {
            return document.uri.clone();
        }
        match program
            .files
            .get(wright_ir::ids::Id::from_index(file_index))
        {
            Some(file) => {
                let path = std::path::PathBuf::from(&file.path);
                if path.is_absolute() {
                    path.to_string_lossy().into_owned()
                } else {
                    self.root.join(path).to_string_lossy().into_owned()
                }
            }
            None => format!("<file {file_index}>"),
        }
    }

    /// Completion items: declared symbols, builtin names, and keywords.
    pub fn completion(&self, uri: &str, position: Position) -> Vec<CompletionItem> {
        let Some(document) = self.store.document(uri) else {
            return Vec::new();
        };
        let analysis = self.analyze(document);

        // Position/context: the identifier being typed and whether the
        // position follows a member-access dot.
        let prefix = word_prefix(&document.text, position);
        let member = member_receiver(&document.text, position);

        if let Some(receiver) = member {
            // Member context: enum members when the receiver is a known enum
            // domain, otherwise the corpus-evidenced receiver methods.
            let catalog = wright_workshop::catalog::Catalog::builtin().ok();
            if let Some(domain) = catalog.and_then(|catalog| {
                catalog
                    .enum_domains()
                    .find(|domain| domain.domain == receiver)
                    .cloned()
            }) {
                return domain
                    .members
                    .iter()
                    .filter(|member| member.member.starts_with(&prefix))
                    .map(|member| CompletionItem {
                        label: member.member.clone(),
                        kind: "enumMember".to_string(),
                        detail: Some(domain.domain.clone()),
                    })
                    .collect();
            }
            return RECEIVER_MEMBERS
                .iter()
                .filter(|member| member.starts_with(&prefix))
                .map(|member| CompletionItem {
                    label: member.to_string(),
                    kind: "method".to_string(),
                    detail: Some("receiver member".to_string()),
                })
                .collect();
        }

        let mut items = Vec::new();
        if let Some(index) = &analysis.index {
            for symbol in index.symbols() {
                if symbol.name.starts_with(&prefix) {
                    items.push(CompletionItem {
                        label: symbol.name.clone(),
                        kind: symbol_kind_name(symbol.kind).to_string(),
                        detail: None,
                    });
                }
            }
        }
        for builtin in BUILTIN_NAMES {
            if builtin.starts_with(&prefix) {
                items.push(CompletionItem {
                    label: builtin.to_string(),
                    kind: "function".to_string(),
                    detail: Some("builtin".to_string()),
                });
            }
        }
        for keyword in KEYWORDS {
            if keyword.starts_with(&prefix) {
                items.push(CompletionItem {
                    label: keyword.to_string(),
                    kind: "keyword".to_string(),
                    detail: None,
                });
            }
        }
        items
    }

    /// Rename the symbol at a position using the M9 safe-edit contract.
    ///
    /// Returns `None` when no symbol resolves at the position (an explicit
    /// refusal); the caller must surface that as a structured refusal rather
    /// than an empty edit.
    pub fn rename(&self, uri: &str, position: Position, new_name: &str) -> Option<RenameResult> {
        let document = self.store.document(uri)?;
        let analysis = self.analyze(document);
        let symbol = self.symbol_at(&analysis, document, position)?;
        let (symbol_kind, from) = (symbol_kind_name(symbol.kind), symbol.name.clone());
        let request = wright_driver::edit::RenameRequest {
            symbol_kind: symbol_kind.to_string(),
            from,
            to: new_name.to_string(),
            source_identity: wright_driver::input_identity(&document.text),
        };
        let edit = wright_driver::edit::rename_symbol(&document.text, &request).ok()?;
        let config = wright_driver::SessionConfig {
            input: wright_driver::InputSpec::Stdin,
            ..wright_driver::SessionConfig::default()
        };
        let validation = wright_driver::edit::validate_edit(&document.text, &edit, &config);
        Some(RenameResult {
            document_version: document.version,
            ok: validation.ok,
            preview: validation.preview,
            range: Some(full_document_range(&document.text)),
            diagnostics: Vec::new(),
        })
    }

    /// Semantic tokens for a document, classified by the native lexer.
    pub fn semantic_tokens(&self, uri: &str) -> Vec<SemanticToken> {
        let Some(document) = self.store.document(uri) else {
            return Vec::new();
        };
        let analysis = self.analyze(document);
        // Token classification needs the raw lexer stream plus the semantic
        // index for symbol identity (never name-string membership alone).
        let tokens = match wright_opy::lexer::lex(wright_opy::lexer::LexInput {
            file_id: 0,
            text: &document.text,
        }) {
            Ok(tokens) => tokens,
            Err(_) => return Vec::new(),
        };
        let mut result = Vec::new();
        for token in &tokens {
            if token.kind == wright_opy::lexer::TokenKind::Eof {
                continue;
            }
            let token_type = classify_token(token, analysis.index.as_ref());
            if token_type.is_empty() {
                continue;
            }
            result.push(SemanticToken {
                line: token.span.start.line.saturating_sub(1),
                character: crate::document::char_offset_to_utf16(
                    document
                        .text
                        .lines()
                        .nth(token.span.start.line.saturating_sub(1) as usize)
                        .unwrap_or_default(),
                    token.span.start.col.saturating_sub(1) as usize,
                ) as u32,
                length: crate::document::utf16_len(&token.text).max(1) as u32,
                token_type,
            });
        }
        result
    }

    /// The symbol whose declaration or reference span contains a position.
    fn symbol_at(
        &self,
        analysis: &Analysis,
        document: &Document,
        position: Position,
    ) -> Option<Symbol> {
        let (line, col) = document.to_line_col(position);
        let index = analysis.index.as_ref()?;
        for symbol in index.symbols() {
            let symbol_id = symbol.id;
            if let Some(span) = symbol.span {
                if span_contains(span, line, col) {
                    return Some(symbol.clone());
                }
            }
            for reference in index.references(symbol_id) {
                if let Some(span) = reference.span {
                    if span_contains(span, line, col) {
                        return Some(symbol.clone());
                    }
                }
            }
        }
        None
    }
}

fn span_contains(span: wright_ir::source::Span, line: u32, col: u32) -> bool {
    (span.start.line, span.start.col) <= (line, col)
        && (line, col)
            <= (
                span.end.line,
                span.end.col.saturating_sub(1).max(span.start.col),
            )
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

/// A 0-based range covering the entire source text, including any trailing
/// newline (so a full-document replacement can delete the final line break).
fn full_document_range(text: &str) -> Range {
    let lines: Vec<&str> = text.split('\n').collect();
    let line_count = lines.len().max(1) as u32;
    let last_line_len = lines.last().unwrap_or(&"").chars().count() as u32;
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: line_count - 1,
            character: last_line_len,
        },
    }
}

fn severity_name(severity: analysis::Severity) -> &'static str {
    match severity {
        analysis::Severity::Warning => "warning",
        analysis::Severity::Info => "info",
    }
}

fn symbol_kind_name(kind: wright_analyzer::symbols::SymbolKind) -> &'static str {
    match kind {
        wright_analyzer::symbols::SymbolKind::GlobalVariable => "globalVariable",
        wright_analyzer::symbols::SymbolKind::PlayerVariable => "playerVariable",
        wright_analyzer::symbols::SymbolKind::Subroutine => "subroutine",
        wright_analyzer::symbols::SymbolKind::Rule => "rule",
    }
}

/// Corpus-evidenced `.opy` builtin names offered by completion.
const BUILTIN_NAMES: &[&str] = &[
    "abs",
    "createBeam",
    "debug",
    "disableInspector",
    "getAllPlayers",
    "len",
    "playEffect",
    "print",
    "random.choice",
    "random.uniform",
    "range",
    "sqrt",
    "vect",
    "wait",
];

/// Source keywords offered by completion.
const KEYWORDS: &[&str] = &[
    "rule",
    "globalvar",
    "playervar",
    "subroutine",
    "def",
    "enum",
    "macro",
    "if",
    "elif",
    "else",
    "for",
    "while",
    "in",
    "and",
    "or",
    "not",
    "pass",
    "true",
    "false",
    "None",
];

/// Corpus-evidenced receiver members offered in member-access completion.
const RECEIVER_MEMBERS: &[&str] = &["append", "format", "uniform", "choice", "hasSpawned"];

/// The identifier being typed immediately before a position.
fn word_prefix(text: &str, position: Position) -> String {
    let line = text.lines().nth(position.line as usize).unwrap_or_default();
    let char_end = crate::document::utf16_offset_to_char(line, position.character as usize);
    line[..char_end]
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

/// The receiver immediately before a member-access dot, when the position
/// follows `<receiver>.` (possibly with a partial member typed after the dot).
fn member_receiver(text: &str, position: Position) -> Option<String> {
    let line = text.lines().nth(position.line as usize)?;
    let char_end = crate::document::utf16_offset_to_char(line, position.character as usize);
    let before = &line[..char_end];
    let trimmed = before.trim_end().strip_suffix('.')?;
    let name = trimmed
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<Vec<_>>();
    if name.is_empty() {
        None
    } else {
        Some(name.into_iter().rev().collect())
    }
}

/// Classify one token by parser/semantic identity: keywords by parser
/// identity, symbol references by semantic index spans and symbol kinds,
/// builtin functions by the corpus catalog, and everything else as an
/// identifier.
fn classify_token(token: &wright_opy::lexer::Token, index: Option<&SemanticIndex>) -> String {
    use wright_opy::lexer::TokenKind;
    match token.kind {
        TokenKind::Ident => {
            if KEYWORDS.contains(&token.text.as_str()) {
                return "keyword".to_string();
            }
            if let Some(index) = index {
                if let Some(kind) =
                    symbol_kind_at(index, token.span.start.line, token.span.start.col)
                {
                    return match kind {
                        wright_analyzer::symbols::SymbolKind::GlobalVariable
                        | wright_analyzer::symbols::SymbolKind::PlayerVariable => {
                            "variable".to_string()
                        }
                        wright_analyzer::symbols::SymbolKind::Subroutine => "function".to_string(),
                        wright_analyzer::symbols::SymbolKind::Rule => "class".to_string(),
                    };
                }
            }
            if BUILTIN_NAMES.contains(&token.text.as_str()) {
                "function".to_string()
            } else {
                "identifier".to_string()
            }
        }
        TokenKind::Number => "number".to_string(),
        TokenKind::String => "string".to_string(),
        TokenKind::Directive => "macro".to_string(),
        TokenKind::At => "attribute".to_string(),
        TokenKind::Newline | TokenKind::Indent(_) => String::new(),
        _ => "operator".to_string(),
    }
}

/// The semantic kind of the symbol whose declaration or reference span
/// contains a 1-based line/column, when one exists.
fn symbol_kind_at(
    index: &SemanticIndex,
    line: u32,
    col: u32,
) -> Option<wright_analyzer::symbols::SymbolKind> {
    for symbol in index.symbols() {
        if symbol
            .span
            .is_some_and(|span| span_contains(span, line, col))
        {
            return Some(symbol.kind);
        }
        for reference in index.references(symbol.id) {
            if reference
                .span
                .is_some_and(|span| span_contains(span, line, col))
            {
                return Some(symbol.kind);
            }
        }
    }
    None
}
