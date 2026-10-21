//! The driver-code literal census and dynamic-position report.
//!
//! The census is literal-first: the mask records every string, raw
//! string, byte string, and char literal in every production `.rs` file
//! of the five scanned crates, and each literal is classified as
//! `Vocab` (a whole diagnostic code — grammar-checked), `Fragment` (a
//! code piece — charset-checked), `Other` (positive non-code evidence),
//! `Deferred` (a name binding resolved by use), or `Unresolved` — which
//! fails the test and prints file, line, and literal. Nothing position-
//! driven decides what to look at, so a `code:` literal inside any
//! syntax — a `let`-RHS struct expression included — cannot slip past.
//!
//! Non-literal values at code positions go to the dynamic report
//! (`dynamic.rs`); their runtime checks are the `debug_assert!`
//! code-token assertions in `Diagnostic::error` and `refusal`, plus the
//! engine-kind tests for `kind`-built shapes.
//!
//! `wright-cli`, `wright-analyzer`, `wright-language`, and `wright-lsp`
//! are scanned alongside `wright-driver`.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use super::classify::{FileCtx, Outcome, Scope, classify, resolve_deferred};
use super::dynamic::{DynSite, enumerate_dynamic};
use super::positions::CodeSite;
use super::sinks::{collect_aliases, collect_code_sinks};
use super::source::{Lit, blank_cfg_test_items, crate_src, line_of, mask_rust_full, read_rs_tree};

/// One literal the census could not classify — printed by the test.
pub(super) struct LitSite {
    pub file: PathBuf,
    pub line: usize,
    pub text: String,
    pub reason: &'static str,
}

/// The census result: literal vocabulary, code fragments, the dynamic
/// report, and anything that failed to classify.
#[derive(Default)]
pub(super) struct Census {
    /// Whole diagnostic-code literals — the grammar test checks these.
    pub vocabulary: Vec<CodeSite>,
    /// Code pieces (`PREFIX` constants, `+=` appends) — charset-checked.
    pub fragments: Vec<CodeSite>,
    /// Non-literal code positions — printed with `--nocapture`, and
    /// asserted nonempty.
    pub dynamic: Vec<DynSite>,
    /// Literals with no evidence either way — the test fails on these.
    pub unresolved: Vec<LitSite>,
    /// Counts for the report.
    pub literals_total: usize,
    pub other: usize,
    pub other_reasons: BTreeMap<&'static str, usize>,
    pub deferred_other: usize,
    pub deferred_vocab: usize,
    /// Literals per kind — `String`/`Raw`/`Char` — for the report.
    pub kinds: [usize; 3],
}

/// Collect every literal diagnostic code from the scanned crates, plus
/// the dynamic-position report, plus the unresolved census failures.
pub(super) fn driver_code_census() -> Census {
    let mut files = read_rs_tree(&crate_src());
    if let Some(crates) = crate_src().parent().and_then(Path::parent) {
        for member in [
            "wright-cli",
            "wright-analyzer",
            "wright-language",
            "wright-lsp",
        ] {
            let dir = crates.join(member).join("src");
            if dir.is_dir() {
                files.extend(read_rs_tree(&dir));
            }
        }
    }
    // The census's own source is not a code position.
    let base = crate_src();
    files.retain(|path, _| {
        !path.starts_with(base.join("diagnostic_code_grammar_tests"))
            && path != &base.join("diagnostic_code_grammar_tests.rs")
    });

    // Mask every file once, keeping the literal spans; `#[cfg(test)]`
    // items stay blanked so test code is never scanned.
    let mut masked_files: BTreeMap<PathBuf, (String, Vec<u8>)> = BTreeMap::new();
    let mut file_lits: BTreeMap<PathBuf, Vec<Lit>> = BTreeMap::new();
    for (path, original) in &files {
        let (masked, lits) = mask_rust_full(original);
        masked_files.insert(
            path.clone(),
            (original.clone(), blank_cfg_test_items(&masked)),
        );
        file_lits.insert(path.clone(), lits);
    }
    let mut sinks = collect_code_sinks(&masked_files);
    collect_aliases(&masked_files, &mut sinks);

    let mut census = Census::default();
    let mut deferred: Vec<(Lit, PathBuf, Vec<String>, Scope)> = Vec::new();
    for (path, (original, masked)) in &masked_files {
        let text = super::source::as_str(masked, 0..masked.len()).to_string();
        let ctx = FileCtx {
            original,
            masked,
            analyzer: path.to_string_lossy().contains("wright-analyzer"),
            stage_glob: text.contains("Stage::*") || original.contains("use crate::diag::Stage::*"),
            sinks: &sinks,
        };
        for lit in &file_lits[path] {
            // A literal whose span was blanked sits in `#[cfg(test)]`
            // code — never scanned.
            if lit.span.0 >= masked.len() || masked[lit.span.0].is_ascii_whitespace() {
                continue;
            }
            census.literals_total += 1;
            census.kinds[match lit.kind {
                super::source::LitKind::Str => 0,
                super::source::LitKind::Raw => 1,
                super::source::LitKind::Char => 2,
            }] += 1;
            let line = line_of(original, lit.span.0);
            let text_of = |l: &Lit| original[l.content.0..l.content.1].to_string();
            match classify(&ctx, lit) {
                Outcome::Vocab(scan) => census.vocabulary.push(CodeSite {
                    file: path.clone(),
                    line,
                    code: text_of(lit),
                    scan,
                }),
                Outcome::Fragment(scan) => census.fragments.push(CodeSite {
                    file: path.clone(),
                    line,
                    code: text_of(lit),
                    scan,
                }),
                Outcome::Other(reason) => {
                    census.other += 1;
                    *census.other_reasons.entry(reason).or_insert(0) += 1;
                }
                Outcome::Deferred { names, scope } => {
                    deferred.push((*lit, path.clone(), names, scope));
                }
                Outcome::Unresolved(reason) => census.unresolved.push(LitSite {
                    file: path.clone(),
                    line,
                    text: text_of(lit),
                    reason,
                }),
            }
        }
    }
    // Deferred names resolve by use: any use at a code position makes
    // the bound literal vocabulary.
    for (lit, path, names, scope) in deferred {
        if resolve_deferred(
            &names,
            &path,
            &scope,
            &masked_files,
            &sinks,
            &mut HashSet::new(),
            0,
        ) {
            let (original, _) = &masked_files[&path];
            census.deferred_vocab += 1;
            census.vocabulary.push(CodeSite {
                file: path,
                line: line_of(original, lit.span.0),
                code: original[lit.content.0..lit.content.1].to_string(),
                scan: "deferred-use",
            });
        } else {
            census.deferred_other += 1;
        }
    }
    enumerate_dynamic(
        &masked_files,
        &sinks,
        &mut census.vocabulary,
        &mut census.dynamic,
    );
    census
}
