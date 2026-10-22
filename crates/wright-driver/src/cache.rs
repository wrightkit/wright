//! Project-local run cache (ADR-0027): a small `.wright/` store that lets
//! `compile` and `inspect cost` compare the current run's element cost
//! against the previous successful run of the same key — never a verdict,
//! exit code, or part of the default JSON.
//!
//! The cache is a convenience layer for people. It never changes a
//! command's verdict, exit code, or default JSON; a comparison is reported
//! only when it is available, and `--max-total` is an explicit gate that
//! reads no cache. `--no-cache` or a non-empty `WRIGHT_NO_CACHE` disables
//! both reading and writing, and every cache failure is ignored — a
//! read-only directory, a corrupt record, or a concurrent writer can never
//! fail a command.
//!
//! Under a truthy `CI` (ADR-0030 decision 14, amending ADR-0027 decisions
//! 7 and 9) only the explicit baseline flags touch `.wright/`, so an
//! action can restore and save it with the platform cache: `--baseline`
//! compares against the named record and `--save-baseline` writes it. The
//! implicit previous-run record stays off, and a baseline that is missing,
//! unreadable, incompatible or keyed to a different input, kind, locale
//! or profile turns the comparison off silently — never an error, warning
//! or output line.

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

/// The truthiness check the renderer's environment detection uses:
/// unset, empty, `0`, `false` and `no` are falsy, anything else truthy.
fn truthy(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "false" | "no"
    )
}

/// Whether the process runs under CI: a truthy `CI` variable (the same
/// check the renderer uses, ADR-0027 decision 9). Under CI the implicit
/// previous-run record stays off and an unusable `--baseline` degrades to
/// no comparison instead of a usage error; the explicit `--baseline` and
/// `--save-baseline` flags still read and write `.wright/` so a CI cache
/// can restore and save it (ADR-0030 decision 14).
pub fn ci() -> bool {
    CacheEnvironment::from_env().ci
}

#[derive(Clone, Copy)]
pub(crate) struct CacheEnvironment {
    pub(crate) ci: bool,
    no_cache: bool,
}

impl CacheEnvironment {
    pub(crate) fn from_env() -> Self {
        Self::from_values(
            std::env::var("CI").ok().as_deref(),
            std::env::var("WRIGHT_NO_CACHE").ok().as_deref(),
        )
    }

    fn from_values(ci: Option<&str>, no_cache: Option<&str>) -> Self {
        Self {
            ci: ci.is_some_and(truthy),
            no_cache: no_cache.is_some_and(|value| !value.is_empty()),
        }
    }

    fn disabled(self, no_cache_flag: bool) -> bool {
        no_cache_flag || self.no_cache
    }
}

/// Read the previous `cost` record for `key`, or a named baseline.
/// Returns `None` when the record is absent, unreadable, or an unknown
/// version/kind — never an error (decision 7).
fn read_record(dir: &Path, file_name: &str) -> Option<Record> {
    let text = fs::read_to_string(dir.join(file_name)).ok()?;
    let record: Record = serde_json::from_str(&text).ok()?;
    (record.version == RECORD_VERSION && record.kind == KIND_COST).then_some(record)
}

/// Read the named baseline usable for `key`: a missing file, an
/// unreadable or incompatible record, or a record keyed to a different
/// input, kind, locale or profile (decision 12) all yield `None` — the
/// baseline is unusable and the caller degrades it: a usage error outside
/// CI, silence under it (ADR-0030 decision 14).
fn read_baseline(dir: &Path, name: &str, key: &str) -> Option<Record> {
    read_record(dir, &format!("baseline-{name}.json")).filter(|record| record.key == key)
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
/// Under CI only the explicit flags act (ADR-0030 decision 14): a run
/// naming neither `--baseline` nor `--save-baseline` touches nothing, the
/// implicit previous-run record is neither read nor written, and an
/// unusable `--baseline` record yields no comparison silently.
///
/// Every filesystem failure is swallowed: the comparison and the write are
/// conveniences that never touch the verdict or exit code (decision 10).
pub fn compare_and_record(run: &CostRun, snapshot: &CostSnapshot) -> Option<Comparison> {
    compare_and_record_in(run, snapshot, CacheEnvironment::from_env())
}

pub(crate) fn compare_and_record_in(
    run: &CostRun,
    snapshot: &CostSnapshot,
    environment: CacheEnvironment,
) -> Option<Comparison> {
    if environment.disabled(run.no_cache) {
        return None;
    }
    // Under CI the implicit previous-run record stays off entirely — a
    // run that names no explicit baseline flag touches nothing (ADR-0030
    // decision 14).
    if environment.ci && run.baseline.is_none() && run.save_baseline.is_none() {
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
        read_baseline(&dir, baseline, &key).map(|record| {
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
    } else if environment.ci {
        None
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
    // Under CI the previous-run record stays off — only the explicit
    // `--save-baseline` write reaches `.wright/`.
    let record = Record {
        version: RECORD_VERSION,
        kind: KIND_COST.to_string(),
        key: key.clone(),
        baseline: None,
        payload: serde_json::to_value(snapshot.payload()).unwrap_or_default(),
    };
    if !environment.ci {
        write_record(&dir, &file, &record);
    }
    if let Some(name) = &run.save_baseline {
        let mut baseline_record = record;
        baseline_record.baseline = Some(name.clone());
        write_record(&dir, &format!("baseline-{name}.json"), &baseline_record);
    }

    comparison
}

/// Whether the explicitly named `--baseline` has a usable record under
/// `.wright/` for this run's key — present, readable, a known
/// version/kind, and keyed to this input, kind, locale and profile. The
/// caller decides what an unusable one means: a usage error outside CI,
/// silence under it (ADR-0030 decision 14).
pub fn baseline_exists(run: &CostRun) -> bool {
    baseline_exists_in(run, CacheEnvironment::from_env())
}

pub(crate) fn baseline_exists_in(run: &CostRun, environment: CacheEnvironment) -> bool {
    if environment.disabled(run.no_cache) {
        return false;
    }
    let Some(name) = &run.baseline else {
        return false;
    };
    let dir = cache_dir(Some(&run.root), &run.root);
    let key = key_id(
        run.input_path.as_deref(),
        &run.kind,
        &run.locale,
        &run.profile,
    );
    read_baseline(&dir, name, &key).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCAL_CACHE: CacheEnvironment = CacheEnvironment {
        ci: false,
        no_cache: false,
    };
    const CI_CACHE: CacheEnvironment = CacheEnvironment {
        ci: true,
        no_cache: false,
    };

    #[test]
    fn only_truthy_ci_marks_ci_and_only_non_empty_no_cache_disables() {
        assert!(!truthy(""));
        assert!(!truthy("0"));
        assert!(!truthy("false"));
        assert!(!truthy(" no "));
        assert!(truthy("true"));
        assert!(truthy("1"));
        for ci in [None, Some(""), Some("0"), Some("false"), Some(" no ")] {
            assert!(!CacheEnvironment::from_values(ci, None).ci);
        }
        for value in [None, Some("")] {
            assert!(!CacheEnvironment::from_values(None, value).disabled(false));
        }
        for value in ["1", "0", "false", " "] {
            assert!(CacheEnvironment::from_values(None, Some(value)).disabled(false));
        }
        assert!(LOCAL_CACHE.disabled(true));
    }

    fn snap(total: usize, rules: &[(&str, usize)]) -> CostSnapshot {
        CostSnapshot {
            total,
            budget: 32768,
            rules: rules.iter().map(|(n, c)| (n.to_string(), *c)).collect(),
        }
    }

    fn cost_run(dir: &Path) -> CostRun {
        CostRun {
            no_cache: false,
            root: dir.to_path_buf(),
            input_path: Some(dir.join("in.ws")),
            kind: "workshop".to_string(),
            locale: "en-US".to_string(),
            profile: "off".to_string(),
            baseline: None,
            save_baseline: None,
        }
    }

    #[test]
    fn same_key_second_run_reports_zero_delta() {
        let dir = std::env::temp_dir().join(format!("wright-cache-{}", std::process::id()));
        let run = cost_run(&dir);
        let snapshot = snap(100, &[("a", 60), ("b", 40)]);
        assert!(compare_and_record_in(&run, &snapshot, LOCAL_CACHE).is_none());
        let cmp = compare_and_record_in(&run, &snapshot, LOCAL_CACHE).expect("second run compares");
        assert_eq!(cmp.total_delta, 0);
        assert!(cmp.changed.is_empty() && cmp.added.is_empty() && cmp.removed.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn added_and_grown_rules_are_named() {
        let dir = std::env::temp_dir().join(format!("wright-cache2-{}", std::process::id()));
        let run = cost_run(&dir);
        compare_and_record_in(&run, &snap(100, &[("a", 60), ("b", 40)]), LOCAL_CACHE);
        let cmp = compare_and_record_in(
            &run,
            &snap(120, &[("a", 60), ("b", 50), ("c", 10)]),
            LOCAL_CACHE,
        )
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
        let dir = std::env::temp_dir().join(format!("wright-cache3-{}", std::process::id()));
        for (locale, profile) in [("en-US", "off"), ("zh-CN", "off"), ("en-US", "compat")] {
            let run = CostRun {
                locale: locale.to_string(),
                profile: profile.to_string(),
                ..cost_run(&dir)
            };
            compare_and_record_in(&run, &snap(100, &[("a", 100)]), LOCAL_CACHE);
        }
        // Each key saw only its own first run, so no comparison exists.
        for (locale, profile) in [("en-US", "off"), ("zh-CN", "off"), ("en-US", "compat")] {
            let run = CostRun {
                locale: locale.to_string(),
                profile: profile.to_string(),
                ..cost_run(&dir)
            };
            let cmp = compare_and_record_in(&run, &snap(100, &[("a", 100)]), LOCAL_CACHE);
            assert_eq!(cmp.map(|c| c.total_delta), Some(0), "{locale}/{profile}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_or_corrupt_record_yields_no_comparison() {
        let dir = std::env::temp_dir().join(format!("wright-cache4-{}", std::process::id()));
        let run = cost_run(&dir);
        fs::create_dir_all(cache_dir(Some(&dir), &dir)).unwrap();
        // A corrupt file is ignored rather than failing the command.
        let key = key_id(run.input_path.as_deref(), "workshop", "en-US", "off");
        fs::write(
            cache_dir(Some(&dir), &dir).join(format!("{key}.json")),
            "not json",
        )
        .unwrap();
        assert!(compare_and_record_in(&run, &snap(100, &[("a", 100)]), LOCAL_CACHE).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    /// ADR-0030 decision 14: under CI a run that names neither `--baseline`
    /// nor `--save-baseline` touches nothing — the implicit previous-run
    /// record stays off entirely.
    #[test]
    fn ci_run_without_baseline_flags_touches_nothing() {
        let dir = std::env::temp_dir().join(format!("wright-cache-ci1-{}", std::process::id()));
        let run = cost_run(&dir);
        // A record left by a local run is not even read under CI.
        compare_and_record_in(&run, &snap(100, &[("a", 100)]), LOCAL_CACHE);
        assert!(dir.join(".wright").is_dir());
        assert!(
            compare_and_record_in(&run, &snap(120, &[("a", 100)]), CI_CACHE).is_none(),
            "no implicit previous-run comparison under CI"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Under CI `--save-baseline` writes only the named record and
    /// `--baseline` reads it; the previous-run record is never written.
    #[test]
    fn ci_save_and_compare_round_trips_the_named_baseline() {
        let dir = std::env::temp_dir().join(format!("wright-cache-ci2-{}", std::process::id()));
        let mut run = cost_run(&dir);
        run.save_baseline = Some("v1".to_string());
        assert!(compare_and_record_in(&run, &snap(100, &[("a", 100)]), CI_CACHE).is_none());
        let entries: Vec<_> = fs::read_dir(dir.join(".wright"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            entries,
            ["baseline-v1.json"],
            "CI writes no previous-run record"
        );

        let mut compare = cost_run(&dir);
        compare.baseline = Some("v1".to_string());
        let cmp = compare_and_record_in(&compare, &snap(120, &[("a", 80), ("b", 40)]), CI_CACHE)
            .expect("a usable baseline compares under CI");
        assert_eq!(cmp.reference, "baseline:v1");
        assert_eq!(cmp.total_delta, 20);
        assert!(baseline_exists_in(&compare, CI_CACHE));

        // Missing, corrupt, and differently keyed records all degrade
        // silently (decision 14 amends ADR-0027 decision 7 for CI).
        let mut unknown = cost_run(&dir);
        unknown.baseline = Some("nope".to_string());
        assert!(compare_and_record_in(&unknown, &snap(120, &[]), CI_CACHE).is_none());
        assert!(!baseline_exists_in(&unknown, CI_CACHE));

        fs::write(dir.join(".wright/baseline-bad.json"), "not json").unwrap();
        let mut bad = cost_run(&dir);
        bad.baseline = Some("bad".to_string());
        assert!(compare_and_record_in(&bad, &snap(120, &[]), CI_CACHE).is_none());
        assert!(!baseline_exists_in(&bad, CI_CACHE));

        let mut other_key = cost_run(&dir);
        other_key.input_path = Some(dir.join("other.ws"));
        other_key.baseline = Some("v1".to_string());
        assert!(
            compare_and_record_in(&other_key, &snap(120, &[]), CI_CACHE).is_none(),
            "a baseline keyed to a different input is unusable"
        );
        assert!(!baseline_exists_in(&other_key, CI_CACHE));
        let _ = fs::remove_dir_all(&dir);
    }

    /// Outside CI the same unusable baselines keep the caller-facing
    /// "unknown baseline" contract: `baseline_exists` is false for a
    /// missing, corrupt, or differently keyed record.
    #[test]
    fn unusable_baselines_are_absent_outside_ci_too() {
        let dir = std::env::temp_dir().join(format!("wright-cache-ci3-{}", std::process::id()));
        let mut save = cost_run(&dir);
        save.save_baseline = Some("v1".to_string());
        compare_and_record_in(&save, &snap(100, &[("a", 100)]), LOCAL_CACHE);

        let mut other_key = cost_run(&dir);
        other_key.input_path = Some(dir.join("other.ws"));
        other_key.baseline = Some("v1".to_string());
        assert!(
            compare_and_record_in(&other_key, &snap(120, &[]), LOCAL_CACHE).is_none(),
            "decision 12: differently keyed entries never compare"
        );
        assert!(!baseline_exists_in(&other_key, LOCAL_CACHE));
        let _ = fs::remove_dir_all(&dir);
    }
}
