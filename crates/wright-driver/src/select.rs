use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
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
            return Err(format!("unknown rule id '{rule}'"));
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
    /// `exit` to `USAGE`. `source_files` is `None` when nothing
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
            Some((message, fields)) => {
                let mut diagnostic = Diagnostic::usage_error(
                    "invalid-selection",
                    crate::diag::Stage::Discovery,
                    message,
                );
                diagnostic.fields = fields;
                diagnostics.push(diagnostic);
                (diagnostics, exit.max(crate::result::exit::USAGE))
            }
            None => (diagnostics, exit),
        }
    }

    /// Apply the selection to serialized findings paired with their typed
    /// identities (#1001): each finding's `FindingMeta` supplies severity
    /// and code, so nothing re-parses them out of the serialized rows.
    /// `meta` must be index-aligned with `findings` (the
    /// `GetFindings`/`findings_meta` contract); the `file` selector still
    /// resolves against each row's `span.path` spelling — it is part of
    /// the wire projection, not the typed identity.
    pub fn apply_findings_meta(
        &self,
        findings: Vec<serde_json::Value>,
        meta: Vec<crate::result::FindingMeta>,
        file_bases: &[PathBuf],
    ) -> (
        Vec<serde_json::Value>,
        Vec<crate::result::FindingMeta>,
        Option<SelectionOutcome>,
    ) {
        debug_assert_eq!(
            findings.len(),
            meta.len(),
            "finding meta must parallel the payload"
        );
        let paired: Vec<(serde_json::Value, crate::result::FindingMeta)> =
            findings.into_iter().zip(meta).collect();
        let (kept, outcome) = self.apply(
            paired,
            |(json, meta)| {
                (
                    meta.severity,
                    meta.code.as_str(),
                    crate::service::json_span_path(json),
                )
            },
            file_bases,
        );
        let (findings, meta): (Vec<_>, Vec<_>) = kept.into_iter().unzip();
        (findings, meta, outcome)
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
        let max_severity = items.iter().map(|item| key(item).0).max();
        let mut kept: Vec<T> = items
            .into_iter()
            .filter(|item| {
                let (severity, code, path) = key(item);
                self.severity.is_none_or(|min| severity >= min)
                    && self.rule.as_ref().is_none_or(|rule| rule == code)
                    && file_matcher
                        .as_ref()
                        .is_none_or(|matcher| matcher.matches_spelling_or_file_identity(path))
            })
            .collect();
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
                .is_none_or(|matcher| matcher.matches_spelling_or_file_identity(path(item)))
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

const FILE_LIST_LIMIT: usize = 10;

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

fn finish_bases(config: &SessionConfig, loaded: Option<&Loaded>) -> Vec<PathBuf> {
    if let Some(loaded) = loaded {
        return file_bases(&loaded.input);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let absolute = |path: &Path| cwd.join(path);
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

pub(crate) fn file_error(
    file: &str,
    source_files: &[String],
    file_bases: &[PathBuf],
) -> Option<(String, serde_json::Map<String, serde_json::Value>)> {
    let matcher = FileMatch::new(file, file_bases);
    if source_files
        .iter()
        .any(|source| matcher.matches_spelling_or_file_identity(Some(source.as_str())))
    {
        return None;
    }
    let total = source_files.len();
    let mut listed: Vec<String> = source_files
        .iter()
        .take(FILE_LIST_LIMIT)
        .map(|source| format!("'{source}'"))
        .collect();
    if total > FILE_LIST_LIMIT {
        listed.push("…".to_string());
    }
    let mut fields = serde_json::Map::new();
    fields.insert("file".to_string(), file.into());
    Some((
        format!(
            "unknown file '{file}'; the loaded program's source files ({total} total): {}",
            listed.join(", ")
        ),
        fields,
    ))
}

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

    fn matches_spelling_or_file_identity(&self, path: Option<&str>) -> bool {
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
        .filter_map(|base| base.join(path).canonicalize().ok())
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

    /// The `FindingMeta` a `GetFindings` fetch would have produced for
    /// `findings` — tests build the parallel typed channel explicitly
    /// (#1001).
    fn meta(findings: &[serde_json::Value]) -> Vec<crate::result::FindingMeta> {
        findings
            .iter()
            .map(|finding| crate::result::FindingMeta {
                severity: Severity::from_label(finding["severity"].as_str()),
                code: finding["code"].as_str().unwrap_or_default().to_string(),
                span_file: None,
            })
            .collect()
    }

    #[test]
    fn inactive_selection_returns_items_untouched() {
        let items = vec![finding("a", "warning", None)];
        let (kept, _, outcome) =
            FindingSelection::default().apply_findings_meta(items.clone(), meta(&items), &[]);
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
        let (kept, _, outcome) = selection(json!({"severity": "warning"})).apply_findings_meta(
            items.clone(),
            meta(&items),
            &[],
        );
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
        let (kept, _, _) = selection(json!({"rule": "a", "file": "x.ws"})).apply_findings_meta(
            items.clone(),
            meta(&items),
            &[],
        );
        assert_eq!(kept.len(), 1);
        let items = vec![finding("a", "warning", None)];
        let (kept, _, _) = selection(json!({"file": "x.ws"})).apply_findings_meta(
            items.clone(),
            meta(&items),
            &[],
        );
        assert!(kept.is_empty());
    }

    #[test]
    fn file_selection_matches_by_canonical_identity() {
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
        let (kept, _, _) = selection(json!({"file": abs.to_str().unwrap()})).apply_findings_meta(
            items.clone(),
            meta(&items),
            &file_bases(&input),
        );
        assert_eq!(kept.len(), 1);

        let items = vec![finding("a", "warning", Some(abs.to_str().unwrap()))];
        let (kept, _, _) = selection(json!({"file": "sub/f.ws"})).apply_findings_meta(
            items.clone(),
            meta(&items),
            &file_bases(&input),
        );
        assert_eq!(kept.len(), 1);

        let items = vec![finding("a", "warning", Some("sub/f.ws"))];
        let (kept, _, _) = selection(json!({"file": "sub/other.ws"})).apply_findings_meta(
            items.clone(),
            meta(&items),
            &file_bases(&input),
        );
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
        let (kept, _, outcome) =
            selection(json!({"max": 1})).apply_findings_meta(items.clone(), meta(&items), &[]);
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
