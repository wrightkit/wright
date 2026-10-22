//! Finding selection shared by the CLI workflow flags and the
//! `wright-agent/v1` request fields (#430). One selection model drives every
//! finding-shaped output (envelope diagnostics and `result.findings`), so the
//! CLI and the agent contract return the same selected set.
//!
//! Selection only narrows what is *reported*: verdicts and exit codes are
//! computed on the full set before selection applies, and the returned
//! [`SelectionOutcome`] keeps the true total, the truncation count, and the
//! full set's highest severity so no surface can understate the result.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use wright_analyzer::service::marker;

use serde::{Deserialize, Serialize};
use wright_analyzer::registry::LintRegistry;

use crate::config::{InputSpec, SessionConfig};
use crate::diag::{Diagnostic, Severity};
use crate::input::ResolvedInput;
use crate::session::Loaded;

/// A finding/diagnostic selection: `severity`/`rule`/`file` filter the set,
/// `max` truncates what remains.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingSelection {
    /// Minimum severity reported (`info` selects everything, `error` selects
    /// only errors).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
    /// Select only findings produced by this rule id (the finding `code`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    /// Select only findings located in this source file, matched by file
    /// identity: the reported `span.path` spelling differs per surface
    /// (root-relative findings, cwd-relative diagnostics), so any spelling
    /// that resolves to the same file selects it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Report at most this many findings after filtering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<usize>,
}

/// The recorded outcome of applying a [`FindingSelection`]: the true total
/// and the count dropped by `max`. `by_code` breaks the *filtered* set down
/// per rule code before `max` truncates it, so a capped result still
/// reports every code's real count (#791). `max_severity` preserves the
/// full set's highest severity for verdict/exit computation and is not
/// part of the wire payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SelectionOutcome {
    /// Findings present before selection.
    pub total: usize,
    /// Filtered findings dropped by `max`.
    pub withheld: usize,
    /// Per-code counts over the filtered set before `max` truncation
    /// (#791). Empty — and omitted — for selections that carry no code.
    #[serde(rename = "byCode", skip_serializing_if = "BTreeMap::is_empty")]
    pub by_code: BTreeMap<String, usize>,
    /// Highest severity over the full, unselected set.
    #[serde(skip)]
    pub max_severity: Option<Severity>,
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Info => 0,
        Severity::Warning => 1,
        Severity::Error => 2,
    }
}

/// Serialized findings always carry a known severity; a missing or unknown
/// value falls back to `Warning`, the same conservative default the CLI
/// verdict uses (`SummaryStatus::from_finding_severity`), so malformed data
/// can never understate the true maximum severity.
fn parse_severity(value: Option<&str>) -> Severity {
    match value {
        Some("error") => Severity::Error,
        Some("info") | Some("notice") => Severity::Info,
        _ => Severity::Warning,
    }
}

impl FindingSelection {
    /// Whether any selection dimension is set.
    pub fn is_active(&self) -> bool {
        self.severity.is_some() || self.rule.is_some() || self.file.is_some() || self.max.is_some()
    }

    /// The selection is a usage error when `rule` names no registered lint
    /// rule: silently selecting nothing would report a clean result for a
    /// typo'd id.
    pub fn validate(&self, registry: &LintRegistry) -> Result<(), String> {
        if let Some(rule) = &self.rule
            && !registry.contains(rule)
        {
            return Err(format!("unknown rule id '{}'", marker::escape(rule)));
        }
        Ok(())
    }

    /// Apply the selection to envelope diagnostics. `file_bases` are the
    /// bases under which `file` and reported `span.path` spellings resolve
    /// (see [`file_bases`]); an empty slice keeps exact-match semantics.
    pub fn apply_diagnostics(
        &self,
        diagnostics: Vec<Diagnostic>,
        file_bases: &[PathBuf],
    ) -> (Vec<Diagnostic>, Option<SelectionOutcome>) {
        self.apply(
            diagnostics,
            |diagnostic| {
                (
                    diagnostic.severity,
                    diagnostic.code.as_str(),
                    diagnostic.span.as_ref().map(|span| span.path.as_str()),
                )
            },
            file_bases,
        )
    }

    /// ADR-0035: a `file` selector names a source file of the loaded
    /// program or the run fails `invalid-selection` — a selector can never
    /// silently empty a result. Call after `apply_diagnostics` so the
    /// unmatched filter cannot drop its own refusal; a refusal raises
    /// `exit` to `SOURCE_ERROR`. `source_files` is `None` when nothing
    /// loaded (the input error already reports the failure).
    pub fn finish_diagnostics(
        &self,
        mut diagnostics: Vec<Diagnostic>,
        exit: u8,
        source_files: Option<&[String]>,
        file_bases: &[PathBuf],
    ) -> (Vec<Diagnostic>, u8) {
        match self
            .file
            .as_deref()
            .zip(source_files)
            .and_then(|(file, files)| file_error(file, files, file_bases))
        {
            Some(message) => {
                diagnostics.push(Diagnostic::error(
                    "invalid-selection",
                    crate::diag::Stage::Discovery,
                    message,
                ));
                (diagnostics, exit.max(crate::result::exit::SOURCE_ERROR))
            }
            None => (diagnostics, exit),
        }
    }

    /// Apply the selection to serialized findings (`code`, `severity`,
    /// `span.path` fields); `file_bases` resolves `file` like
    /// [`apply_diagnostics`](Self::apply_diagnostics).
    pub fn apply_findings(
        &self,
        findings: Vec<serde_json::Value>,
        file_bases: &[PathBuf],
    ) -> (Vec<serde_json::Value>, Option<SelectionOutcome>) {
        self.apply(
            findings,
            |finding| {
                (
                    parse_severity(finding.get("severity").and_then(|v| v.as_str())),
                    finding.get("code").and_then(|v| v.as_str()).unwrap_or(""),
                    crate::service::json_span_path(finding),
                )
            },
            file_bases,
        )
    }

    fn apply<T>(
        &self,
        items: Vec<T>,
        key: impl Fn(&T) -> (Severity, &str, Option<&str>),
        file_bases: &[PathBuf],
    ) -> (Vec<T>, Option<SelectionOutcome>) {
        if !self.is_active() {
            return (items, None);
        }
        let file_matcher = self
            .file
            .as_deref()
            .map(|file| FileMatch::new(file, file_bases));
        let total = items.len();
        let max_severity = items
            .iter()
            .map(|item| key(item).0)
            .max_by_key(|severity| severity_rank(*severity));
        let mut kept: Vec<T> = items
            .into_iter()
            .filter(|item| {
                let (severity, code, path) = key(item);
                self.severity
                    .is_none_or(|min| severity_rank(severity) >= severity_rank(min))
                    && self.rule.as_ref().is_none_or(|rule| rule == code)
                    && file_matcher
                        .as_ref()
                        .is_none_or(|matcher| matcher.matches(path))
            })
            .collect();
        // #791: per-code counts describe the filtered set `max` is about
        // to truncate, so a capped result still reports every code's real
        // count; `total` alone cannot.
        let mut by_code = BTreeMap::new();
        for item in &kept {
            let code = key(item).1;
            if !code.is_empty() {
                *by_code.entry(code.to_string()).or_insert(0) += 1;
            }
        }
        let withheld = self.max.map_or(0, |max| kept.len().saturating_sub(max));
        if let Some(max) = self.max {
            kept.truncate(max);
        }
        (
            kept,
            Some(SelectionOutcome {
                total,
                withheld,
                by_code,
                max_severity,
            }),
        )
    }
}

/// Apply one list selection to a semantic operation's items (#531): the
/// caller's `keep` encodes the operation-specific filters, `file` and `max`
/// are shared. `total` counts the set before selection and `withheld`
/// counts only what `max` dropped — the same semantics `FindingSelection`
/// establishes for findings (#430).
pub(crate) fn select_list<T>(
    items: Vec<T>,
    file: Option<&str>,
    file_bases: &[PathBuf],
    path: impl Fn(&T) -> Option<&str>,
    keep: impl Fn(&T) -> bool,
    max: Option<usize>,
) -> (Vec<T>, SelectionOutcome) {
    let matcher = file.map(|file| FileMatch::new(file, file_bases));
    let total = items.len();
    let mut kept: Vec<T> = items
        .into_iter()
        .filter(|item| {
            matcher
                .as_ref()
                .is_none_or(|matcher| matcher.matches(path(item)))
                && keep(item)
        })
        .collect();
    let withheld = max.map_or(0, |max| kept.len().saturating_sub(max));
    if let Some(max) = max {
        kept.truncate(max);
    }
    (
        kept,
        SelectionOutcome {
            total,
            withheld,
            by_code: BTreeMap::new(),
            max_severity: None,
        },
    )
}

/// How many source-file spellings an unmatched-`file` error lists
/// (ADR-0035): the program's own files, bounded, with the total.
const FILE_LIST_LIMIT: usize = 10;

/// The session finish-path diagnostic pipeline: apply the configured
/// selection to the accumulated diagnostics, then (ADR-0035) refuse
/// `invalid-selection` when `file` names no source file of the loaded
/// program — joining after the filter so it cannot drop its own refusal.
/// Returns the diagnostics, the selection outcome, and the exit code.
pub(crate) fn envelope_diagnostics(
    config: &SessionConfig,
    loaded: Option<&Loaded>,
    diagnostics: Vec<Diagnostic>,
) -> (Vec<Diagnostic>, Option<SelectionOutcome>, u8) {
    let bases = finish_bases(config, loaded);
    let exit = crate::result::exit_code_from(&diagnostics);
    let (diagnostics, outcome) = config.selection.apply_diagnostics(diagnostics, &bases);
    let (diagnostics, exit) = config.selection.finish_diagnostics(
        diagnostics,
        exit,
        loaded.map(|loaded| loaded.source_files.as_slice()),
        &bases,
    );
    (diagnostics, outcome, exit)
}

/// The bases under which `file` and reported `span.path` spellings resolve
/// on the finish path (#430): the loaded input's cwd and root, or the
/// config-derived equivalents so pre-load diagnostics select the same way.
/// Extra bases are safe — they only ever add true file identities.
fn finish_bases(config: &SessionConfig, loaded: Option<&Loaded>) -> Vec<PathBuf> {
    if let Some(loaded) = loaded {
        return file_bases(&loaded.input);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let absolute = |path: &Path| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        }
    };
    let mut bases = vec![cwd.clone()];
    if let Some(root) = &config.root {
        bases.push(absolute(root));
    }
    if let InputSpec::Path(path) = &config.input {
        let path = absolute(path);
        bases.push(path.clone());
        if let Some(parent) = path.parent() {
            bases.push(parent.to_path_buf());
        }
    }
    bases
}

/// A `file` selector must name a source file of the loaded program
/// (ADR-0035): it is matched against the program's own source spellings
/// under the same bases `span.path` matching uses, so the value as passed,
/// root-relative, or absolute all select the same file — no suffix
/// matching. `Some` carries the `invalid-selection` message listing the
/// program's source files (bounded, with the total); a `file` that matches
/// a program file but selects nothing stays a valid empty result.
pub(crate) fn file_error(
    file: &str,
    source_files: &[String],
    file_bases: &[PathBuf],
) -> Option<String> {
    let matcher = FileMatch::new(file, file_bases);
    if source_files
        .iter()
        .any(|source| matcher.matches(Some(source.as_str())))
    {
        return None;
    }
    let total = source_files.len();
    let mut listed: Vec<String> = source_files
        .iter()
        .take(FILE_LIST_LIMIT)
        .map(|source| format!("'{}'", marker::escape(source)))
        .collect();
    if total > FILE_LIST_LIMIT {
        listed.push("…".to_string());
    }
    Some(format!(
        "unknown file '{}'; the loaded program's source files ({total} total): {}",
        marker::escape(file),
        listed.join(", ")
    ))
}

/// The `file` dimension matched by file identity rather than one string
/// spelling: `span.path` is reported root-relative on finding surfaces and
/// cwd-relative on diagnostic surfaces, so both sides resolve under the
/// session's bases (invocation cwd and the input root) to a canonical path.
/// Pseudo-paths (`<stdin>`, `<file N>`, `<provider-artifact>`) do not resolve
/// and keep exact-match semantics only.
struct FileMatch {
    spelled: String,
    canonical: BTreeSet<PathBuf>,
    bases: Vec<PathBuf>,
}

impl FileMatch {
    fn new(file: &str, bases: &[PathBuf]) -> FileMatch {
        FileMatch {
            canonical: canonical_forms(Path::new(file), bases),
            spelled: file.to_string(),
            bases: bases.to_vec(),
        }
    }

    fn matches(&self, path: Option<&str>) -> bool {
        let Some(path) = path else { return false };
        path == self.spelled
            || canonical_forms(Path::new(path), &self.bases)
                .iter()
                .any(|form| self.canonical.contains(form))
    }
}

pub(crate) fn file_bases(input: &ResolvedInput) -> Vec<PathBuf> {
    vec![input.cwd.clone(), input.root.clone()]
}

fn canonical_forms(path: &Path, bases: &[PathBuf]) -> BTreeSet<PathBuf> {
    bases
        .iter()
        .filter_map(|base| {
            let full = if path.is_absolute() {
                path.to_path_buf()
            } else {
                base.join(path)
            };
            full.canonicalize().ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn finding(code: &str, severity: &str, path: Option<&str>) -> serde_json::Value {
        json!({
            "code": code,
            "severity": severity,
            "message": "msg",
            "span": { "path": path },
        })
    }

    fn selection(input: serde_json::Value) -> FindingSelection {
        serde_json::from_value(input).unwrap()
    }

    #[test]
    fn inactive_selection_returns_items_untouched() {
        let items = vec![finding("a", "warning", None)];
        let (kept, outcome) = FindingSelection::default().apply_findings(items.clone(), &[]);
        assert_eq!(kept, items);
        assert!(outcome.is_none());
    }

    #[test]
    fn severity_threshold_keeps_at_or_above() {
        let items = vec![
            finding("a", "info", None),
            finding("b", "warning", None),
            finding("c", "error", None),
        ];
        let (kept, outcome) = selection(json!({"severity": "warning"})).apply_findings(items, &[]);
        assert_eq!(
            kept.iter()
                .map(|f| f["code"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["b", "c"]
        );
        let outcome = outcome.unwrap();
        assert_eq!(outcome.total, 3);
        assert_eq!(outcome.withheld, 0);
        assert_eq!(outcome.max_severity, Some(Severity::Error));
    }

    #[test]
    fn rule_and_file_filter() {
        let items = vec![
            finding("a", "warning", Some("x.ws")),
            finding("a", "warning", Some("y.ws")),
            finding("b", "warning", Some("x.ws")),
        ];
        let (kept, _) = selection(json!({"rule": "a", "file": "x.ws"})).apply_findings(items, &[]);
        assert_eq!(kept.len(), 1);
        let (kept, _) = selection(json!({"file": "x.ws"}))
            .apply_findings(vec![finding("a", "warning", None)], &[]);
        assert!(kept.is_empty());
    }

    #[test]
    fn file_selection_matches_by_canonical_identity() {
        // #430 review: `span.path` spellings differ per surface — findings
        // report root-relative paths, envelope diagnostics report the
        // cwd-relative display path. `file` must select the same file from
        // any spelling that resolves to it.
        let root = std::env::temp_dir().join(format!("wright-select-{}", std::process::id()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let abs = root.join("sub/f.ws");
        std::fs::write(&abs, "x").unwrap();
        let input = ResolvedInput {
            kind: crate::config::SourceKind::Workshop,
            text: String::new(),
            path: Some(abs.clone()),
            target: crate::input::InputTarget::File,
            root: root.clone(),
            cwd: std::env::current_dir().unwrap(),
            display: abs.display().to_string(),
            identity: String::new(),
            origin: crate::diag::Origin {
                kind: "workshop".to_string(),
                locale: None,
            },
        };

        let items = vec![finding("a", "warning", Some("sub/f.ws"))];
        let (kept, _) = selection(json!({"file": abs.to_str().unwrap()}))
            .apply_findings(items, &file_bases(&input));
        assert_eq!(kept.len(), 1);

        let items = vec![finding("a", "warning", Some(abs.to_str().unwrap()))];
        let (kept, _) =
            selection(json!({"file": "sub/f.ws"})).apply_findings(items, &file_bases(&input));
        assert_eq!(kept.len(), 1);

        let items = vec![finding("a", "warning", Some("sub/f.ws"))];
        let (kept, _) =
            selection(json!({"file": "sub/other.ws"})).apply_findings(items, &file_bases(&input));
        assert!(kept.is_empty());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn max_truncates_and_reports_withheld() {
        let items = vec![
            finding("a", "warning", None),
            finding("b", "warning", None),
            finding("c", "warning", None),
            finding("d", "warning", None),
        ];
        let (kept, outcome) = selection(json!({"max": 1})).apply_findings(items, &[]);
        assert_eq!(kept.len(), 1);
        let outcome = outcome.unwrap();
        assert_eq!(outcome.total, 4);
        assert_eq!(outcome.withheld, 3);
    }

    #[test]
    fn diagnostics_share_the_same_semantics() {
        let mut items = vec![
            Diagnostic::error("parse-error", crate::diag::Stage::Frontend, "a"),
            Diagnostic::warning("warn-code", crate::diag::Stage::Analysis, "b"),
        ];
        items[0].severity = Severity::Error;
        let (kept, outcome) = selection(json!({"severity": "error"})).apply_diagnostics(items, &[]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].code, "parse-error");
        assert_eq!(outcome.unwrap().max_severity, Some(Severity::Error));
    }

    #[test]
    fn unknown_rule_is_an_error() {
        let registry = LintRegistry::default();
        assert!(
            selection(json!({"rule": "min-wait-loop"}))
                .validate(&registry)
                .is_ok()
        );
        let err = selection(json!({"rule": "not-a-rule"}))
            .validate(&registry)
            .unwrap_err();
        assert!(err.contains("not-a-rule"));
    }

    #[test]
    fn outcome_serializes_only_wire_fields() {
        let outcome = SelectionOutcome {
            total: 4,
            withheld: 3,
            by_code: BTreeMap::new(),
            max_severity: Some(Severity::Warning),
        };
        assert_eq!(
            serde_json::to_value(&outcome).unwrap(),
            json!({"total": 4, "withheld": 3})
        );
    }
}
