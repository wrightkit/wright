//! Narrow Wright adapter for the owner-side `del-rs` implementation.
//!
//! `del-rs` owns OSTW/DeltinScript parsing, project loading, semantic
//! analysis, lowering, diagnostics, and reconstruction. This crate only maps
//! those owner contracts to the historical Wright driver boundaries.

use std::path::{Path, PathBuf};

use workshop_rs::source::{FileId as WorkshopFileId, Position, Span};

pub mod diag {
    use workshop_rs::source::Span;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct SourceError {
        pub code: String,
        pub message: String,
        pub span: Option<Span>,
    }

    pub type SourceResult<T> = Result<T, SourceError>;

    impl SourceError {
        pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
            Self {
                code: code.into(),
                message: message.into(),
                span: None,
            }
        }
    }

    impl std::fmt::Display for SourceError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}: {}", self.code, self.message)
        }
    }

    impl std::error::Error for SourceError {}
}

pub use diag::{SourceError, SourceResult};

pub mod lexer {
    use super::{SourceError, SourceResult};
    use del_rs::span::FileId as DelFileId;
    use workshop_rs::source::{FileId, Position, Span};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum TokenKind {
        Ident,
        Number,
        String,
        VerbatimString,
        Eof,
        LParen,
        RParen,
        LBracket,
        RBracket,
        LBrace,
        RBrace,
        Comma,
        Colon,
        Semi,
        Dot,
        Pipe,
        At,
        Assign,
        PlusAssign,
        MinusAssign,
        StarAssign,
        SlashAssign,
        PercentAssign,
        Plus,
        Minus,
        Star,
        Slash,
        Percent,
        Power,
        PlusPlus,
        MinusMinus,
        Eq,
        Ne,
        Lt,
        Le,
        Gt,
        Ge,
        And,
        Or,
        Bang,
        Question,
    }

    #[derive(Debug, Clone, PartialEq)]
    pub struct Token {
        pub kind: TokenKind,
        pub text: String,
        pub span: Span,
    }

    pub struct LexInput<'a> {
        pub file_id: FileId,
        pub text: &'a str,
    }

    pub fn lex(input: LexInput<'_>) -> SourceResult<Vec<Token>> {
        let file = DelFileId(input.file_id.index() as u32);
        let (tokens, diagnostics) = del_rs::syntax::lexer::lex(file, input.text);
        if let Some(diagnostic) = diagnostics.first() {
            return Err(error(input.file_id, input.text, diagnostic));
        }
        Ok(tokens
            .into_iter()
            .map(|token| Token {
                kind: map_kind(token.kind),
                text: input
                    .text
                    .get(token.span.start as usize..token.span.end as usize)
                    .unwrap_or_default()
                    .to_string(),
                span: map_span(input.file_id, input.text, token.span.start, token.span.end),
            })
            .collect())
    }

    fn error(file: FileId, text: &str, diagnostic: &del_rs::Diagnostic) -> SourceError {
        SourceError {
            code: diagnostic.code.clone(),
            message: diagnostic.message.clone(),
            span: Some(map_span(
                file,
                text,
                diagnostic.primary.start,
                diagnostic.primary.end,
            )),
        }
    }

    fn map_span(file: FileId, text: &str, start: u32, end: u32) -> Span {
        Span::new(
            file,
            position(text, start as usize),
            position(text, end as usize),
        )
    }

    fn position(text: &str, offset: usize) -> Position {
        let mut line = 1;
        let mut col = 1;
        for ch in text[..offset.min(text.len())].chars() {
            if ch == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        Position::new(line, col)
    }

    fn map_kind(kind: del_rs::TokenKind) -> TokenKind {
        use del_rs::TokenKind as K;
        match kind {
            K::Ident
            | K::KwRule
            | K::KwDefine
            | K::KwGlobalVar
            | K::KwPlayerVar
            | K::KwIf
            | K::KwElse
            | K::KwFor
            | K::KwForeach
            | K::KwWhile
            | K::KwSwitch
            | K::KwCase
            | K::KwDefault
            | K::KwBreak
            | K::KwContinue
            | K::KwReturn
            | K::KwClass
            | K::KwStruct
            | K::KwEnum
            | K::KwConstructor
            | K::KwNew
            | K::KwDelete
            | K::KwIn
            | K::KwRef
            | K::KwRecursive
            | K::KwAsync
            | K::KwConst
            | K::KwImport
            | K::KwAs
            | K::KwIs
            | K::KwPublic
            | K::KwPrivate
            | K::KwProtected
            | K::KwStatic
            | K::KwVirtual
            | K::KwOverride
            | K::KwSingle
            | K::KwThis
            | K::KwRoot
            | K::KwTrue
            | K::KwFalse
            | K::KwNull
            | K::KwType
            | K::KwDisabled
            | K::KwPersist
            | K::KwVoid
            | K::KwJson => TokenKind::Ident,
            K::Int | K::Real => TokenKind::Number,
            K::Str | K::Bool => TokenKind::String,
            K::LParen => TokenKind::LParen,
            K::RParen => TokenKind::RParen,
            K::LBrace => TokenKind::LBrace,
            K::RBrace => TokenKind::RBrace,
            K::LBracket => TokenKind::LBracket,
            K::RBracket => TokenKind::RBracket,
            K::Comma => TokenKind::Comma,
            K::Semicolon => TokenKind::Semi,
            K::Colon => TokenKind::Colon,
            K::Dot => TokenKind::Dot,
            K::Arrow => TokenKind::Minus,
            K::Plus => TokenKind::Plus,
            K::Minus => TokenKind::Minus,
            K::Star => TokenKind::Star,
            K::Slash => TokenKind::Slash,
            K::Percent => TokenKind::Percent,
            K::Caret => TokenKind::Power,
            K::PlusPlus => TokenKind::PlusPlus,
            K::MinusMinus => TokenKind::MinusMinus,
            K::PlusEq => TokenKind::PlusAssign,
            K::MinusEq => TokenKind::MinusAssign,
            K::StarEq => TokenKind::StarAssign,
            K::SlashEq => TokenKind::SlashAssign,
            K::PercentEq => TokenKind::PercentAssign,
            K::CaretEq => TokenKind::Power,
            K::Eq => TokenKind::Assign,
            K::EqEq => TokenKind::Eq,
            K::Bang => TokenKind::Bang,
            K::BangEq => TokenKind::Ne,
            K::Lt => TokenKind::Lt,
            K::Gt => TokenKind::Gt,
            K::LtEq => TokenKind::Le,
            K::GtEq => TokenKind::Ge,
            K::AmpAmp => TokenKind::And,
            K::PipePipe => TokenKind::Or,
            K::Pipe => TokenKind::Pipe,
            K::Question => TokenKind::Question,
            K::At => TokenKind::At,
            K::Tilde
            | K::DotDot
            | K::Error
            | K::Whitespace
            | K::LineComment
            | K::BlockComment
            | K::DocComment => TokenKind::Ident,
            K::Eof => TokenKind::Eof,
        }
    }
}

pub mod reconstruct {
    pub use del_rs::reconstruct::{ReconstructError, reconstruct};
}

#[derive(Debug, Clone)]
pub struct ResolvedImport {
    pub path: String,
    pub span: Span,
    pub target: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct FileRecord {
    pub id: u32,
    pub path: String,
    pub source: bool,
    pub parsed: bool,
    pub imports: Vec<ResolvedImport>,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub entry: String,
    pub files: Vec<FileRecord>,
    pub inventory: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct OstwOutcome {
    pub project: Option<Project>,
    pub error: Option<SourceError>,
    pub diagnostics: Vec<SourceError>,
}

#[derive(Debug, Clone)]
pub struct SemanticOutcome {
    pub wir: Option<workshop_rs::wir::Program>,
    pub diagnostics: Vec<SourceError>,
}

pub fn compile_with_semantics(
    main_text: &str,
    main_path: Option<&str>,
    root: &Path,
) -> (OstwOutcome, SemanticOutcome) {
    compile_with_semantics_overlay(
        main_text,
        main_path,
        root,
        &std::collections::BTreeMap::new(),
    )
}

pub fn compile_with_semantics_overlay(
    main_text: &str,
    main_path: Option<&str>,
    root: &Path,
    overlay: &std::collections::BTreeMap<String, String>,
) -> (OstwOutcome, SemanticOutcome) {
    let mut source_overlay = overlay.clone();
    if let Some(main_path) = main_path {
        source_overlay.insert(main_path.replace('\\', "/"), main_text.to_string());
    }
    let project = del_rs::project::load_project_with_overlay(
        del_rs::project::ProjectOptions {
            root: root.to_path_buf(),
            entry: main_path.map(PathBuf::from),
            config: None,
        },
        &source_overlay,
    );
    let project_view = project_view(&project);
    let project_diagnostics = project
        .diagnostics
        .iter()
        .map(|diagnostic| map_diagnostic(&project, diagnostic))
        .collect::<Vec<_>>();
    let outcome = OstwOutcome {
        project: Some(project_view),
        error: None,
        diagnostics: project_diagnostics.clone(),
    };
    let provider = match del_rs::semantic::provider::CatalogProvider::new() {
        Ok(provider) => provider,
        Err(error) => {
            let diagnostic = SourceError::new("catalog-error", error.to_string());
            return (
                outcome,
                SemanticOutcome {
                    wir: None,
                    diagnostics: vec![diagnostic],
                },
            );
        }
    };
    let semantic = del_rs::api::check_project_api(&project, &provider);
    let mut diagnostics = semantic
        .diagnostics
        .iter()
        .map(|diagnostic| map_diagnostic(&project, diagnostic))
        .collect::<Vec<_>>();
    let (program, lowering) = del_rs::workshop::lower_project_to_wir_best_effort(&semantic);
    diagnostics.extend(
        lowering
            .iter()
            .map(|diagnostic| map_diagnostic(&project, diagnostic)),
    );
    (
        outcome,
        SemanticOutcome {
            wir: Some(program),
            diagnostics,
        },
    )
}

fn project_view(project: &del_rs::project::Project) -> Project {
    let mut files: Vec<FileRecord> = project
        .files
        .iter()
        .map(|file| {
            let source = project.sources.get(*file);
            let has_source = !source.text.is_empty();
            FileRecord {
                id: file.0,
                path: source.name.to_string_lossy().replace('\\', "/"),
                source: has_source,
                parsed: has_source,
                imports: project
                    .imports
                    .iter()
                    .filter(|edge| edge.importer == *file)
                    .map(|edge| ResolvedImport {
                        path: project
                            .sources
                            .get(edge.imported)
                            .name
                            .to_string_lossy()
                            .into(),
                        span: map_span(project, edge.span),
                        target: Some(edge.imported.0),
                    })
                    .collect(),
            }
        })
        .collect();
    files.sort_by_key(|file| file.id);
    Project {
        entry: project
            .sources
            .get(project.entry)
            .name
            .to_string_lossy()
            .replace('\\', "/"),
        files,
        inventory: inventory(&project.root),
    }
}

fn inventory(root: &Path) -> Vec<String> {
    fn visit(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(root, &path, out);
            } else if matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("del" | "ostw")
            ) {
                if let Ok(relative) = path.strip_prefix(root) {
                    out.push(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }

    let mut files = Vec::new();
    visit(root, root, &mut files);
    files.sort();
    files
}

fn map_diagnostic(
    project: &del_rs::project::Project,
    diagnostic: &del_rs::Diagnostic,
) -> SourceError {
    let code = match diagnostic.phase {
        del_rs::Phase::Lex => "ostw-lex-error".to_string(),
        del_rs::Phase::Parse => "ostw-parse-error".to_string(),
        del_rs::Phase::Project if diagnostic.code == "PJ002" => "ostw-missing-import".to_string(),
        del_rs::Phase::Project => "ostw-project-error".to_string(),
        del_rs::Phase::Semantic | del_rs::Phase::Hir | del_rs::Phase::Oracle => {
            "ostw-unsupported".to_string()
        }
    };
    SourceError {
        code,
        message: diagnostic.message.clone(),
        span: Some(map_span(project, diagnostic.primary)),
    }
}

fn map_span(project: &del_rs::project::Project, span: del_rs::Span) -> Span {
    let start = project.sources.line_col(span, span.start);
    let end = project.sources.line_col(span, span.end);
    Span::new(
        WorkshopFileId::from_index(span.file.0 as usize),
        Position::new(start.line, start.col),
        Position::new(end.line, end.col),
    )
}
