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
    pub document_version: i64,
}

/// Hover content.
#[derive(Debug, Clone, Serialize)]
pub struct Hover {
    pub contents: String,
    pub range: Option<Range>,
    pub document_version: i64,
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

/// The result of a rename request.
#[derive(Debug, Clone, Serialize)]
pub struct RenameResult {
    /// The current document version.
    pub document_version: i64,
    /// Whether the rename validates through the compiler pipeline.
    pub ok: bool,
    /// The previewed edited source text.
    pub preview: Option<String>,
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
    pub fn analyze(&self, document: &Document) -> Analysis {
        let mut parse_errors = Vec::new();
        let hir = match wright_opy::compile(&document.text, &document.uri, &self.root) {
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

    /// The declaration range of the symbol referenced at a position.
    pub fn definition(&self, uri: &str, position: Position) -> Option<Range> {
        let document = self.store.document(uri)?;
        let analysis = self.analyze(document);
        let symbol = self.symbol_at(&analysis, document, position)?;
        symbol.span.map(|span| document.from_span(&span))
    }

    /// Every reference range of the symbol at a position.
    pub fn references(&self, uri: &str, position: Position) -> Vec<Range> {
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
                    .filter_map(|reference| reference.span.map(|span| document.from_span(&span)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Completion items: declared symbols, builtin names, and keywords.
    pub fn completion(&self, uri: &str, _position: Position) -> Vec<CompletionItem> {
        let Some(document) = self.store.document(uri) else {
            return Vec::new();
        };
        let analysis = self.analyze(document);
        let mut items = Vec::new();
        if let Some(index) = &analysis.index {
            for symbol in index.symbols() {
                items.push(CompletionItem {
                    label: symbol.name.clone(),
                    kind: symbol_kind_name(symbol.kind).to_string(),
                    detail: None,
                });
            }
        }
        for builtin in BUILTIN_NAMES {
            items.push(CompletionItem {
                label: builtin.to_string(),
                kind: "function".to_string(),
                detail: Some("builtin".to_string()),
            });
        }
        for keyword in KEYWORDS {
            items.push(CompletionItem {
                label: keyword.to_string(),
                kind: "keyword".to_string(),
                detail: None,
            });
        }
        items
    }

    /// Rename the symbol at a position using the M9 safe-edit contract.
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
            diagnostics: Vec::new(),
        })
    }

    /// Semantic tokens for a document, classified by the native lexer.
    pub fn semantic_tokens(&self, uri: &str) -> Vec<SemanticToken> {
        let Some(document) = self.store.document(uri) else {
            return Vec::new();
        };
        let analysis = self.analyze(document);
        let mut declared: Vec<String> = analysis
            .index
            .as_ref()
            .map(|index| index.symbols().map(|symbol| symbol.name.clone()).collect())
            .unwrap_or_default();
        // Token classification needs the raw lexer stream.
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
            let token_type = classify_token(token, &mut declared);
            if token_type.is_empty() {
                continue;
            }
            result.push(SemanticToken {
                line: token.span.start.line.saturating_sub(1),
                character: token.span.start.col.saturating_sub(1),
                length: token.text.chars().count().max(1) as u32,
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

/// Classify one token by parser/semantic identity.
fn classify_token(token: &wright_opy::lexer::Token, declared: &mut [String]) -> String {
    use wright_opy::lexer::TokenKind;
    match token.kind {
        TokenKind::Ident => {
            if KEYWORDS.contains(&token.text.as_str()) {
                "keyword".to_string()
            } else if declared.iter().any(|name| name == &token.text) {
                "variable".to_string()
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
