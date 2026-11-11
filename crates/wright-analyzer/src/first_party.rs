//! The first-party lint package (`wrightkit/lint-rules`) embedded as the
//! default rule set, so a fresh install lints offline (#1081). The files are
//! a verbatim copy of the package's `rules/` at the commit recorded in
//! `lint-rules/UPSTREAM.md`; `scripts/sync-lint-rules.py` keeps the copy, the
//! commit, and this list in step.

/// `(file name, YAML)` for every embedded rule, in lexical file order — the
/// order a package directory loads in.
pub(crate) const RULES: &[(&str, &str)] = &[
    (
        "expensive-loop-check.yaml",
        include_str!("../../../lint-rules/rules/expensive-loop-check.yaml"),
    ),
    (
        "min-wait-loop.yaml",
        include_str!("../../../lint-rules/rules/min-wait-loop.yaml"),
    ),
    (
        "nested-binder-lambda.yaml",
        include_str!("../../../lint-rules/rules/nested-binder-lambda.yaml"),
    ),
];
