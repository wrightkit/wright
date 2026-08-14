//! The reusable compiler/session driver (M6, issue #37).
//!
//! [`CompilerSession`] is the single orchestration path shared by the CLI,
//! library consumers, and (in later milestones) tool APIs and LSP: input
//! resolution → frontend selection → validation → lowering → analysis →
//! emission. Frontends are selected by [`SourceKind`] behind one contract, so
//! the M7 native `.opy` frontend can replace the temporary adapter bridge
//! without changing callers. Every workflow returns a typed [`Envelope`]
//! whose JSON serialization is the machine-readable CLI contract.

use std::sync::Arc;

use wright_analyzer::registry::LintConfig;
use wright_analyzer::service::{Origin as ServiceOrigin, Request, SemanticService};
use wright_ir::wir;

use crate::config::{SessionConfig, SourceKind};
use crate::diag::{Diagnostic, Origin, Position, SourceSpan, Stage};
use crate::input::{self, ResolvedInput};
use crate::result::{
    AnalyzeResult, CheckResult, CompileResult, CompiledOutput, Envelope, InspectResult, LintResult,
    exit_code_from, version_info,
};
use crate::{input_identity, opy};

/// A successfully loaded program with its input and origin metadata.
#[derive(Clone)]
pub struct Loaded {
    /// The validated Workshop IR program.
    pub program: Arc<wir::Program>,
    /// Origin metadata carried into diagnostics and results.
    pub origin: Origin,
    /// The resolved input.
    pub input: ResolvedInput,
}

/// One reusable compiler session.
pub struct CompilerSession {
    /// The session configuration (input, frontend, overrides, format).
    pub config: SessionConfig,
    catalog: wright_workshop::catalog::Catalog,
    loaded: Option<Loaded>,
    diagnostics: Vec<Diagnostic>,
}

impl CompilerSession {
    /// Build a session from a configuration.
    pub fn new(config: SessionConfig) -> Result<CompilerSession, Diagnostic> {
        let catalog = wright_workshop::catalog::Catalog::builtin().map_err(|error| {
            Diagnostic::error(
                "catalog-error",
                Stage::Internal,
                format!("cannot load the built-in Workshop catalog: {error}"),
            )
        })?;
        Ok(CompilerSession {
            config,
            catalog,
            loaded: None,
            diagnostics: Vec::new(),
        })
    }

    /// Load (or reuse) the validated program for this session.
    ///
    /// Loading is idempotent: repeated calls return the same program without
    /// re-reading the input. Returns an owned snapshot so callers can hold it
    /// while mutating the session.
    pub fn load(&mut self) -> Result<Loaded, Diagnostic> {
        if let Some(loaded) = &self.loaded {
            return Ok(loaded.clone());
        }
        let mut resolved = input::resolve(&self.config)?;
        let mut program = match resolved.kind {
            SourceKind::Workshop => {
                let (program, locale) = self.load_workshop(&resolved)?;
                resolved.origin.locale = Some(locale);
                program
            }
            SourceKind::Protocol => {
                let json = resolved.text.clone();
                self.load_protocol(&json, &resolved)?
            }
            SourceKind::Opy => {
                if opy::adapter_fallback_requested() {
                    let json = opy::run_adapter(&resolved)?;
                    self.load_protocol(&json, &resolved)?
                } else {
                    // Default: the native Rust `.opy` frontend (no Node/OverPy).
                    // Use the outcome form so the file registry survives errors
                    // and diagnostics can name included files (#83).
                    let outcome = wright_opy::compile_with_overlay_outcome(
                        &resolved.text,
                        &resolved.display,
                        &resolved.root,
                        &std::collections::BTreeMap::new(),
                    );
                    let program = match outcome.hir {
                        Some(hir) => hir,
                        None => {
                            let error = outcome
                                .error
                                .expect("a failed compile outcome always carries an error");
                            return Err(opy_diag(error, &outcome.files, &resolved));
                        }
                    };
                    self.load_hir(program, &resolved)?
                }
            }
            SourceKind::Auto => {
                return Err(Diagnostic::error(
                    "input-kind-unknown",
                    Stage::Discovery,
                    "input kind could not be detected; pass `--kind opy|workshop|protocol`",
                ));
            }
        };
        // Apply the selected transformation profile (validated before/after).
        if self.config.profile != wright_transform::Profile::Off {
            wright_transform::run(&mut program, self.config.profile).map_err(|error| {
                Diagnostic::error(
                    "transform-error",
                    Stage::Internal,
                    format!("WIR transformation failed: {error}"),
                )
            })?;
        }
        let loaded = Loaded {
            program: Arc::new(program),
            origin: resolved.origin.clone(),
            input: resolved,
        };
        self.loaded = Some(loaded.clone());
        Ok(loaded)
    }

    /// The diagnostics accumulated by the last workflow run.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// The locale a Workshop input resolved to, if the last load was
    /// Workshop-origin.
    pub fn resolved_locale(&self) -> Option<String> {
        self.loaded
            .as_ref()
            .and_then(|loaded| loaded.origin.locale.clone())
    }

    fn load_workshop(
        &mut self,
        resolved: &ResolvedInput,
    ) -> Result<(wir::Program, String), Diagnostic> {
        let override_locale = self
            .config
            .locale
            .as_deref()
            .map(wright_workshop::catalog::Locale::new);
        let locale = wright_workshop::detect::resolve_locale(
            &resolved.text,
            &self.catalog,
            override_locale.as_ref(),
        )
        .map_err(|error| workshop_diag(error, resolved))?;
        let program = wright_workshop::parser::parse(&resolved.text, &self.catalog, &locale)
            .map_err(|error| workshop_diag(error, resolved))?;
        program
            .validate()
            .map_err(|error| ir_diag("validation-error", Stage::Validation, error, resolved))?;
        Ok((program, locale.to_string()))
    }

    fn load_protocol(
        &mut self,
        json: &str,
        resolved: &ResolvedInput,
    ) -> Result<wir::Program, Diagnostic> {
        let protocol =
            wright_core::hir::parse_str(json).map_err(|error| hir_diag(error, resolved))?;
        self.load_hir(protocol, resolved)
    }

    /// Ingest an already-parsed Opy HIR program: validate, convert, lower.
    fn load_hir(
        &mut self,
        protocol: wright_core::hir::Program,
        resolved: &ResolvedInput,
    ) -> Result<wir::Program, Diagnostic> {
        // The native path is protocol-validated here for the first time
        // (settings domain checks against the emission table, #86); the
        // adapter path validates inside parse_str, so this is a double
        // validation there — acceptable.
        protocol
            .validate()
            .map_err(|error| hir_diag(error, resolved))?;
        let model = protocol
            .to_ir()
            .map_err(|error| ir_diag("convert-error", Stage::Lowering, error, resolved))?;
        let program = wright_ir::lower::lower(&model)
            .map_err(|error| ir_diag("lower-error", Stage::Lowering, error, resolved))?;
        program
            .validate()
            .map_err(|error| ir_diag("validation-error", Stage::Validation, error, resolved))?;
        Ok(program)
    }

    /// `compile`: load, emit localized Workshop text, and write it out.
    pub fn compile(&mut self) -> Envelope<CompileResult> {
        let command = "compile";
        let mut result = CompileResult::default();
        let output = match self.compile_output() {
            Ok(output) => output,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, result);
            }
        };
        match &self.config.output {
            Some(path) => {
                if let Err(error) = std::fs::write(path, &output.text) {
                    self.diagnostics.push(Diagnostic::error(
                        "output-io",
                        Stage::Emission,
                        format!("cannot write output '{}': {error}", path.display()),
                    ));
                    return self.finish(command, result);
                }
                result.output = Some(CompiledOutput {
                    written_to: crate::input::display_path(path),
                    ..output
                });
            }
            None => {
                result.output = Some(CompiledOutput {
                    written_to: "stdout".to_string(),
                    ..output
                });
            }
        }
        self.finish(command, result)
    }

    fn compile_output(&mut self) -> Result<CompiledOutput, Diagnostic> {
        let loaded = self.load()?;
        let locale = loaded
            .origin
            .locale
            .clone()
            .map(|locale| wright_workshop::catalog::Locale::new(&locale))
            .unwrap_or_else(|| wright_workshop::catalog::Locale::new("en-US"));
        let text = wright_workshop::emitter::emit(&loaded.program, &self.catalog, &locale)
            .map_err(|error| workshop_diag(error, &loaded.input))?;
        let sha256 = input_identity(&text);
        Ok(CompiledOutput {
            text,
            sha256,
            locale: locale.to_string(),
            written_to: String::new(),
            input_identity: loaded.input.identity.clone(),
        })
    }

    /// `check`: load, validate, and surface analysis findings as diagnostics.
    pub fn check(&mut self) -> Envelope<CheckResult> {
        let command = "check";
        let loaded = match self.load() {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, CheckResult {});
            }
        };
        self.attach_analysis(&loaded);
        self.finish(command, CheckResult {})
    }

    /// `analyze`: load and produce the semantic summary and findings.
    pub fn analyze(&mut self) -> Envelope<AnalyzeResult> {
        let command = "analyze";
        let loaded = match self.load() {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, AnalyzeResult::default());
            }
        };
        let service = match self.service(&loaded) {
            Ok(service) => service,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, AnalyzeResult::default());
            }
        };
        let program = service_response(&service, &Request::Program);
        let findings = service_response(&service, &Request::GetFindings);
        self.finish(command, AnalyzeResult { program, findings })
    }

    /// `inspect`: load and produce the structural/semantic program model.
    pub fn inspect(&mut self) -> Envelope<InspectResult> {
        let command = "inspect";
        let loaded = match self.load() {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, InspectResult::default());
            }
        };
        let service = match self.service(&loaded) {
            Ok(service) => service,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, InspectResult::default());
            }
        };
        let program = service_response(&service, &Request::Program);
        let rules = service_response(&service, &Request::ListRules);
        let symbols = service_response(&service, &Request::ListSymbols { kind: None });
        let references: serde_json::Value = {
            let symbol_ids: Vec<u32> = symbols
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|symbol| symbol.get("id").and_then(serde_json::Value::as_u64))
                        .map(|id| id as u32)
                        .collect()
                })
                .unwrap_or_default();
            let list: Vec<serde_json::Value> = symbol_ids
                .iter()
                .map(|id| service_response(&service, &Request::FindReferences { symbol: *id }))
                .collect();
            serde_json::Value::Array(list)
        };
        self.finish(
            command,
            InspectResult {
                program,
                rules,
                symbols,
                references,
            },
        )
    }

    /// `lint`: load and produce the source identity, program summary, rule
    /// metadata, effective configuration, and findings (M12, #98).
    ///
    /// Lint findings are reported in `result.findings`, not in the envelope
    /// diagnostics (like `analyze`). Rule enable/disable and severity come
    /// from `self.config.lint`, the same configuration the CLI flags and
    /// programmatic consumers set.
    pub fn lint(&mut self) -> Envelope<LintResult> {
        let command = "lint";
        let loaded = match self.load() {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, LintResult::default());
            }
        };
        let service = match self.service_with(&loaded, self.config.lint.clone()) {
            Ok(service) => service,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, LintResult::default());
            }
        };
        let program = service_response(&service, &Request::Program);
        let lint_rules = service_response(&service, &Request::LintRules);
        let mut findings = service_response(&service, &Request::GetFindings);
        enrich_finding_spans(&mut findings, &loaded);
        let (rules, config) = match lint_rules {
            serde_json::Value::Object(mut object) => (
                object
                    .remove("rules")
                    .unwrap_or_else(|| serde_json::json!([])),
                object
                    .remove("config")
                    .unwrap_or_else(|| serde_json::json!({})),
            ),
            _ => (serde_json::json!([]), serde_json::json!({})),
        };
        self.finish(
            command,
            LintResult {
                input_identity: loaded.input.identity.clone(),
                program,
                rules,
                config,
                findings,
            },
        )
    }

    /// Build the semantic service over a loaded program.
    fn service<'a>(&self, loaded: &'a Loaded) -> Result<SemanticService<'a>, Diagnostic> {
        self.service_with(loaded, LintConfig::default())
    }

    /// Build the semantic service over a loaded program with an explicit lint
    /// configuration.
    fn service_with<'a>(
        &self,
        loaded: &'a Loaded,
        config: LintConfig,
    ) -> Result<SemanticService<'a>, Diagnostic> {
        let origin = ServiceOrigin {
            kind: loaded.origin.kind.clone(),
            locale: loaded.origin.locale.clone(),
        };
        SemanticService::with_origin_and_config(&loaded.program, origin, config)
            .map_err(|error| ir_diag("analysis-error", Stage::Analysis, error, &loaded.input))
    }

    /// Attach semantic-analysis findings to the diagnostic list (for `check`).
    fn attach_analysis(&mut self, loaded: &Loaded) {
        let Ok(service) = self.service(loaded) else {
            return;
        };
        let findings = service_response(&service, &Request::GetFindings);
        let Some(findings) = findings.as_array() else {
            return;
        };
        for finding in findings {
            let code = finding
                .get("code")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("finding");
            let severity = match finding.get("severity").and_then(serde_json::Value::as_str) {
                Some("error") => crate::diag::Severity::Error,
                Some("warning") => crate::diag::Severity::Warning,
                _ => crate::diag::Severity::Info,
            };
            let message = finding
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            self.diagnostics.push(Diagnostic {
                code: code.to_string(),
                stage: Stage::Analysis,
                severity,
                message,
                span: span_from_json(finding.get("span")),
                source: Some(loaded.origin.clone()),
            });
        }
    }

    fn finish<T: serde::Serialize>(&mut self, command: &str, result: T) -> Envelope<T> {
        let diagnostics = std::mem::take(&mut self.diagnostics);
        let has_error = diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == crate::diag::Severity::Error);
        let exit = if has_error {
            exit_code_from(&diagnostics)
        } else {
            crate::result::exit::SUCCESS
        };
        Envelope {
            wright: version_info(),
            command: command.to_string(),
            ok: !has_error,
            exit,
            diagnostics,
            result,
        }
    }
}

/// Extract the `result` payload of a semantic-service request as JSON.
fn service_response(service: &SemanticService<'_>, request: &Request) -> serde_json::Value {
    match service.handle(request) {
        wright_analyzer::service::Response::Ok { result } => result,
        wright_analyzer::service::Response::Error { .. } => serde_json::Value::Null,
    }
}

/// Convert a JSON span value (from the semantic service) to a diagnostic span.
fn span_from_json(value: Option<&serde_json::Value>) -> Option<SourceSpan> {
    let value = value?;
    let file = value.get("file")?.as_u64()? as usize;
    let start = value.get("start")?;
    let end = value.get("end")?;
    Some(SourceSpan {
        file,
        path: format!("<file {file}>"),
        start: Position {
            line: start.get("line")?.as_u64()? as u32,
            col: start.get("col")?.as_u64()? as u32,
        },
        end: Position {
            line: end.get("line")?.as_u64()? as u32,
            col: end.get("col")?.as_u64()? as u32,
        },
    })
}

/// Add the resolved `path` to every lint finding span.
///
/// File 0 is the main input and carries its resolved display path; other
/// files are named `<file N>` (matching the [`span_from_json`] convention),
/// so lint output preserves original source identity where available.
fn enrich_finding_spans(findings: &mut serde_json::Value, loaded: &Loaded) {
    let Some(list) = findings.as_array_mut() else {
        return;
    };
    for finding in list {
        let Some(span) = finding.get_mut("span") else {
            continue;
        };
        if !span.is_object() {
            continue;
        }
        let path = span
            .get("file")
            .and_then(serde_json::Value::as_u64)
            .map(|file| {
                if file == 0 {
                    loaded.input.display.clone()
                } else {
                    format!("<file {file}>")
                }
            })
            .unwrap_or_else(|| loaded.input.display.clone());
        span["path"] = serde_json::Value::String(path);
    }
}

/// Map a Workshop-language error to a driver diagnostic.
fn workshop_diag(error: wright_workshop::WorkshopError, resolved: &ResolvedInput) -> Diagnostic {
    let (code, stage, span) = match &error {
        wright_workshop::WorkshopError::Catalog(catalog) => {
            return Diagnostic::error(
                "catalog-error",
                Stage::Internal,
                format!("{}: {}", catalog.code, catalog.message),
            );
        }
        wright_workshop::WorkshopError::Unknown { kind, span, .. } => (
            format!("unknown-{kind}"),
            Stage::Frontend,
            span.map(|span| SourceSpan {
                file: span.file.index(),
                path: resolved.display.clone(),
                start: Position {
                    line: span.start.line,
                    col: span.start.col,
                },
                end: Position {
                    line: span.end.line,
                    col: span.end.col,
                },
            }),
        ),
        wright_workshop::WorkshopError::Malformed { span, .. } => (
            "parse-error".to_string(),
            Stage::Frontend,
            span.map(|span| SourceSpan {
                file: span.file.index(),
                path: resolved.display.clone(),
                start: Position {
                    line: span.start.line,
                    col: span.start.col,
                },
                end: Position {
                    line: span.end.line,
                    col: span.end.col,
                },
            }),
        ),
        wright_workshop::WorkshopError::Unsupported { span, .. } => (
            "unsupported-construct".to_string(),
            Stage::Frontend,
            span.map(|span| SourceSpan {
                file: span.file.index(),
                path: resolved.display.clone(),
                start: Position {
                    line: span.start.line,
                    col: span.start.col,
                },
                end: Position {
                    line: span.end.line,
                    col: span.end.col,
                },
            }),
        ),
    };
    Diagnostic {
        code,
        stage,
        severity: crate::diag::Severity::Error,
        message: error.to_string(),
        span,
        source: Some(resolved.origin.clone()),
    }
}

/// Map a native frontend error to a driver diagnostic.
///
/// Span paths resolve through the frontend file registry so a failure inside
/// an included file names that file; file 0 (the main file) carries the
/// resolved display path by construction (#83).
fn opy_diag(
    error: wright_opy::FrontendError,
    files: &[wright_opy::preprocess::FileRecord],
    resolved: &ResolvedInput,
) -> Diagnostic {
    let span = error.span.map(|span| SourceSpan {
        file: span.file as usize,
        path: files
            .get(span.file as usize)
            .map(|file| file.path.clone())
            .unwrap_or_else(|| resolved.display.clone()),
        start: Position {
            line: span.start.line,
            col: span.start.col,
        },
        end: Position {
            line: span.end.line,
            col: span.end.col,
        },
    });
    Diagnostic {
        code: error.code,
        stage: Stage::Frontend,
        severity: crate::diag::Severity::Error,
        message: error.message,
        span,
        source: Some(resolved.origin.clone()),
    }
}

/// Map an Opy HIR ingestion error to a driver diagnostic.
fn hir_diag(error: wright_core::hir::HirError, resolved: &ResolvedInput) -> Diagnostic {
    let stage = match &error {
        wright_core::hir::HirError::Invalid { .. } => Stage::Validation,
        _ => Stage::Frontend,
    };
    let span = error.span().map(|span| SourceSpan {
        file: span.file as usize,
        path: resolved.display.clone(),
        start: Position {
            line: span.start.line,
            col: span.start.col,
        },
        end: Position {
            line: span.end.line,
            col: span.end.col,
        },
    });
    Diagnostic {
        code: error.code().to_string(),
        stage,
        severity: crate::diag::Severity::Error,
        message: error.message(),
        span,
        source: Some(resolved.origin.clone()),
    }
}

/// Map an IR error to a driver diagnostic.
fn ir_diag(
    code: &'static str,
    stage: Stage,
    error: wright_ir::error::IrError,
    resolved: &ResolvedInput,
) -> Diagnostic {
    Diagnostic {
        code: code.to_string(),
        stage,
        severity: crate::diag::Severity::Error,
        message: error.to_string(),
        span: None,
        source: Some(resolved.origin.clone()),
    }
}
