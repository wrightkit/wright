//! Project-local run cache (ADR-0027): a small `.wright/` store that lets
//! `compile` and `inspect cost` compare the current run's element cost
//! against the previous successful run of the same key — never a verdict,
//! exit code, or part of the default JSON.
//!
//! The cache is a convenience layer for people. It never changes a
//! command's verdict, exit code, or default JSON; a comparison is reported
//! only when it is available, and `--max-total` is an explicit gate that
//! reads no cache. `--no-cache`, a non-empty `WRIGHT_NO_CACHE`, or a truthy
//! `CI` disables both reading and writing, and every cache failure is
//! ignored — a read-only directory, a corrupt record, or a concurrent
//! writer can never fail a command.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::input::sha256_hex;

/// The on-disk schema version. A record with an unknown version or kind is
/// treated as absent (ADR-0027 decision 7).
const RECORD_VERSION: u32 = 1;
/// The payload kind this implementation reads and writes. Other kinds are
/// ignored on read (decision 13).
const KIND_COST: &str = "cost";

/// A persisted cache record: one file under `.wright/` keyed by the
/// resolved input, kind, locale, and profile (decision 12). `payload` is
/// command-owned — the store never interprets it.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Record {
    version: u32,
    kind: String,
    key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    baseline: Option<String>,
    payload: serde_json::Value,
}

/// One rule's element cost inside a `cost` record, for comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuleCost {
    name: String,
    count: usize,
}

/// The `cost` payload a record carries: the element total, the budget, and
/// the per-rule shares needed to name which rules changed between runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CostPayload {
    total: usize,
    budget: usize,
    rules: Vec<RuleCost>,
}

/// Where `.wright/` lives for a run: the project root (`--root` when given,
/// otherwise the input's resolved root, otherwise the invocation cwd).
fn cache_dir(input_root: Option<&Path>, cwd: &Path) -> PathBuf {
    input_root.unwrap_or(cwd).join(".wright")
}

/// The cache-file name for one key. Keys differ in the resolved input file,
/// input kind, locale, and profile, so a development entry point and a
/// release entry point never compare with each other (decision 12).
fn key_id(input_path: Option<&Path>, kind: &str, locale: &str, profile: &str) -> String {
    let material = format!(
        "{}|{kind}|{locale}|{profile}",
        input_path
            .map(|p| p
                .canonicalize()
                .unwrap_or_else(|_| p.to_path_buf())
                .display()
                .to_string())
            .unwrap_or_else(|| "<stdin>".to_string())
    );
    sha256_hex(material.as_bytes())[..16].to_string()
}

/// Whether the cache is enabled: off under `--no-cache`, a non-empty
/// `WRIGHT_NO_CACHE`, or a truthy `CI` (the same check the renderer uses).
pub fn enabled(no_cache_flag: bool) -> bool {
    !no_cache_flag
        && !disabled_by_env(
            std::env::var("CI").ok().as_deref(),
            std::env::var("WRIGHT_NO_CACHE").ok().as_deref(),
        )
}

fn disabled_by_env(ci: Option<&str>, no_cache: Option<&str>) -> bool {
    let truthy = |v: &str| {
        !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "no"
        )
    };
    ci.is_some_and(truthy) || no_cache.is_some_and(|v| !v.is_empty())
}

/// Read the previous `cost` record for `key`, or a named baseline.
/// Returns `None` when the record is absent, unreadable, or an unknown
/// version/kind — never an error (decision 7).
fn read_record(dir: &Path, file_name: &str) -> Option<Record> {
    let text = fs::read_to_string(dir.join(file_name)).ok()?;
    let record: Record = serde_json::from_str(&text).ok()?;
    (record.version == RECORD_VERSION && record.kind == KIND_COST).then_some(record)
}

/// Write a record atomically — a temporary file in the same directory,
/// then a rename — so concurrent writers of different keys cannot corrupt
/// each other and the same key ends with one complete record (decision 12).
/// Failures are ignored: the cache is a convenience, never a gate.
fn write_record(dir: &Path, file_name: &str, record: &Record) {
    let _ = fs::create_dir_all(dir);
    let tmp = dir.join(format!(".{file_name}.tmp-{}", std::process::id()));
    if fs::write(&tmp, serde_json::to_vec(record).unwrap_or_default()).is_ok() {
        let _ = fs::rename(&tmp, dir.join(file_name));
    } else {
        let _ = fs::remove_file(&tmp);
    }
}

/// The comparison this module reports between the current run and a stored
/// reference: the total delta and the rules whose counts changed.
#[derive(Debug, Clone, Serialize)]
pub struct Comparison {
    /// The stored reference's element total.
    pub previous_total: usize,
    /// `current - previous` (signed).
    pub total_delta: isize,
    /// Rules present in both runs whose count changed.
    pub changed: Vec<RuleDelta>,
    /// Rules only in the current run.
    pub added: Vec<String>,
    /// Rules only in the stored run.
    pub removed: Vec<String>,
    /// Where the comparison came from: `previous` (the last successful run)
    /// or a named baseline.
    pub reference: String,
}

/// One rule whose element count moved between two runs.
#[derive(Debug, Clone, Serialize)]
pub struct RuleDelta {
    pub name: String,
    pub before: usize,
    pub after: usize,
    pub delta: isize,
}

fn compare(current: &CostPayload, reference: &CostPayload, reference_name: String) -> Comparison {
    let before: BTreeMap<&str, usize> = reference
        .rules
        .iter()
        .map(|r| (r.name.as_str(), r.count))
        .collect();
    let after: BTreeMap<&str, usize> = current
        .rules
        .iter()
        .map(|r| (r.name.as_str(), r.count))
        .collect();
    let mut changed = Vec::new();
    let mut added = Vec::new();
    for (name, after_count) in &after {
        match before.get(name) {
            Some(before_count) if before_count != after_count => changed.push(RuleDelta {
                name: (*name).to_string(),
                before: *before_count,
                after: *after_count,
                delta: *after_count as isize - *before_count as isize,
            }),
            None => added.push((*name).to_string()),
            _ => {}
        }
    }
    let removed = before
        .keys()
        .filter(|name| !after.contains_key(*name))
        .map(|name| (*name).to_string())
        .collect();
    Comparison {
        previous_total: reference.total,
        total_delta: current.total as isize - reference.total as isize,
        changed,
        added,
        removed,
        reference: reference_name,
    }
}

impl Comparison {
    /// Serialize the comparison as additive JSON for the result payload.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "reference": self.reference,
            "previousTotal": self.previous_total,
            "totalDelta": self.total_delta,
            "changed": self.changed.iter().map(|d| serde_json::json!({
                "name": d.name,
                "before": d.before,
                "after": d.after,
                "delta": d.delta,
            })).collect::<Vec<_>>(),
            "added": self.added,
            "removed": self.removed,
        })
    }
}

/// Inputs needed to key, read, and write one run's cost record.
pub struct CostRun {
    /// `--no-cache` flag.
    pub no_cache: bool,
    /// The project root (`--root` or the resolved input root) `.wright/` lives under.
    pub root: PathBuf,
    /// The resolved input file; `None` for stdin/directory inputs.
    pub input_path: Option<PathBuf>,
    /// The resolved input kind.
    pub kind: String,
    /// The effective locale.
    pub locale: String,
    /// The effective profile.
    pub profile: String,
    /// `--baseline <name>`: compare against the named record.
    pub baseline: Option<String>,
    /// `--save-baseline <name>`: also record the current run under `name`.
    pub save_baseline: Option<String>,
}

/// The element cost of the current run, captured once for comparison and
/// recording.
#[derive(Debug, Clone)]
pub struct CostSnapshot {
    pub total: usize,
    pub budget: usize,
    /// `(name, count)` per rule, canonical order.
    pub rules: Vec<(String, usize)>,
}

impl CostSnapshot {
    fn payload(&self) -> CostPayload {
        CostPayload {
            total: self.total,
            budget: self.budget,
            rules: self
                .rules
                .iter()
                .map(|(name, count)| RuleCost {
                    name: name.clone(),
                    count: *count,
                })
                .collect(),
        }
    }
}

/// Compare the current run against the stored reference and then record it
/// as the new previous run (and the named baseline when `--save-baseline`
/// is given). Returns the comparison when one was available.
///
/// Every filesystem failure is swallowed: the comparison and the write are
/// conveniences that never touch the verdict or exit code (decision 10).
pub fn compare_and_record(run: &CostRun, snapshot: &CostSnapshot) -> Option<Comparison> {
    if !enabled(run.no_cache) {
        return None;
    }
    let dir = cache_dir(Some(&run.root), &run.root);
    let key = key_id(
        run.input_path.as_deref(),
        &run.kind,
        &run.locale,
        &run.profile,
    );
    let file = format!("{key}.json");

    let comparison = if let Some(baseline) = &run.baseline {
        let name = format!("baseline-{baseline}.json");
        read_record(&dir, &name).map(|record| {
            let payload: CostPayload =
                serde_json::from_value(record.payload).unwrap_or(CostPayload {
                    total: 0,
                    budget: 0,
                    rules: Vec::new(),
                });
            compare(
                &snapshot.payload(),
                &payload,
                format!("baseline:{baseline}"),
            )
        })
    } else {
        read_record(&dir, &file).map(|record| {
            let payload: CostPayload =
                serde_json::from_value(record.payload).unwrap_or(CostPayload {
                    total: 0,
                    budget: 0,
                    rules: Vec::new(),
                });
            compare(&snapshot.payload(), &payload, "previous".to_string())
        })
    };

    // The current run becomes the new previous run for this key; a named
    // baseline is written additionally and never overwritten by reads.
    let record = Record {
        version: RECORD_VERSION,
        kind: KIND_COST.to_string(),
        key: key.clone(),
        baseline: None,
        payload: serde_json::to_value(snapshot.payload()).unwrap_or_default(),
    };
    write_record(&dir, &file, &record);
    if let Some(name) = &run.save_baseline {
        let mut baseline_record = record;
        baseline_record.baseline = Some(name.clone());
        write_record(&dir, &format!("baseline-{name}.json"), &baseline_record);
    }

    comparison
}

/// Look up a named baseline's payload, for the explicit `--baseline <name>`
/// usage error when the name does not exist (decision 7).
pub fn baseline_exists(run: &CostRun) -> bool {
    if !enabled(run.no_cache) {
        return false;
    }
    let dir = cache_dir(Some(&run.root), &run.root);
    let Some(name) = &run.baseline else {
        return false;
    };
    read_record(&dir, &format!("baseline-{name}.json")).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cache honors `CI` and `WRIGHT_NO_CACHE`, both of which are set in
    /// CI — so a test that exercises recording must clear them first (it is
    /// the only `compare_and_record` caller that disables via env).
    fn enable() {
        // SAFETY: tests in this module are the only env mutation; clearing
        // both guard vars before exercising `compare_and_record` is sound.
        unsafe {
            std::env::remove_var("CI");
            std::env::remove_var("WRIGHT_NO_CACHE");
        }
    }

    #[test]
    fn env_disables_only_on_a_non_empty_no_cache_or_truthy_ci() {
        assert!(!disabled_by_env(None, None));
        assert!(disabled_by_env(None, Some("1")));
        assert!(disabled_by_env(Some("true"), None));
        // Issue #723 specifies a *non-empty* `WRIGHT_NO_CACHE`: an
        // exported-but-empty value leaves the cache on.
        assert!(!disabled_by_env(None, Some("")));
        assert!(!disabled_by_env(Some("false"), Some("")));
        assert!(!disabled_by_env(Some(""), None));
    }

    fn snap(total: usize, rules: &[(&str, usize)]) -> CostSnapshot {
        CostSnapshot {
            total,
            budget: 32768,
            rules: rules.iter().map(|(n, c)| (n.to_string(), *c)).collect(),
        }
    }

    #[test]
    fn same_key_second_run_reports_zero_delta() {
        enable();
        let dir = std::env::temp_dir().join(format!("wright-cache-{}", std::process::id()));
        let run = CostRun {
            no_cache: false,
            root: dir.clone(),
            input_path: Some(dir.join("in.ws")),
            kind: "workshop".to_string(),
            locale: "en-US".to_string(),
            profile: "off".to_string(),
            baseline: None,
            save_baseline: None,
        };
        let snapshot = snap(100, &[("a", 60), ("b", 40)]);
        assert!(compare_and_record(&run, &snapshot).is_none());
        let cmp = compare_and_record(&run, &snapshot).expect("second run compares");
        assert_eq!(cmp.total_delta, 0);
        assert!(cmp.changed.is_empty() && cmp.added.is_empty() && cmp.removed.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn added_and_grown_rules_are_named() {
        enable();
        let dir = std::env::temp_dir().join(format!("wright-cache2-{}", std::process::id()));
        let run = CostRun {
            no_cache: false,
            root: dir.clone(),
            input_path: Some(dir.join("in.ws")),
            kind: "workshop".to_string(),
            locale: "en-US".to_string(),
            profile: "off".to_string(),
            baseline: None,
            save_baseline: None,
        };
        compare_and_record(&run, &snap(100, &[("a", 60), ("b", 40)]));
        let cmp = compare_and_record(&run, &snap(120, &[("a", 60), ("b", 50), ("c", 10)]))
            .expect("comparison");
        assert_eq!(cmp.total_delta, 20);
        assert_eq!(cmp.added, vec!["c"]);
        assert_eq!(cmp.changed.len(), 1);
        assert_eq!(cmp.changed[0].name, "b");
        assert_eq!(cmp.changed[0].delta, 10);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn entries_differing_only_in_locale_or_profile_never_compare() {
        enable();
        let dir = std::env::temp_dir().join(format!("wright-cache3-{}", std::process::id()));
        for (locale, profile) in [("en-US", "off"), ("zh-CN", "off"), ("en-US", "compat")] {
            let run = CostRun {
                no_cache: false,
                root: dir.clone(),
                input_path: Some(dir.join("in.ws")),
                kind: "workshop".to_string(),
                locale: locale.to_string(),
                profile: profile.to_string(),
                baseline: None,
                save_baseline: None,
            };
            compare_and_record(&run, &snap(100, &[("a", 100)]));
        }
        // Each key saw only its own first run, so no comparison exists.
        for (locale, profile) in [("en-US", "off"), ("zh-CN", "off"), ("en-US", "compat")] {
            let run = CostRun {
                no_cache: false,
                root: dir.clone(),
                input_path: Some(dir.join("in.ws")),
                kind: "workshop".to_string(),
                locale: locale.to_string(),
                profile: profile.to_string(),
                baseline: None,
                save_baseline: None,
            };
            let cmp = compare_and_record(&run, &snap(100, &[("a", 100)]));
            assert_eq!(cmp.map(|c| c.total_delta), Some(0), "{locale}/{profile}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_or_corrupt_record_yields_no_comparison() {
        enable();
        let dir = std::env::temp_dir().join(format!("wright-cache4-{}", std::process::id()));
        let run = CostRun {
            no_cache: false,
            root: dir.clone(),
            input_path: Some(dir.join("in.ws")),
            kind: "workshop".to_string(),
            locale: "en-US".to_string(),
            profile: "off".to_string(),
            baseline: None,
            save_baseline: None,
        };
        fs::create_dir_all(cache_dir(Some(&dir), &dir)).unwrap();
        // A corrupt file is ignored rather than failing the command.
        let key = key_id(run.input_path.as_deref(), "workshop", "en-US", "off");
        fs::write(
            cache_dir(Some(&dir), &dir).join(format!("{key}.json")),
            "not json",
        )
        .unwrap();
        assert!(compare_and_record(&run, &snap(100, &[("a", 100)])).is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
