//! Shared predicates for the diagnostic-code census: what counts as a
//! code channel (field names, binding names, mutating methods,
//! code-channel-shaped arguments) and the single-literal evaluator
//! used by the dynamic-position report to recognise
//! `format!("lit")`/`concat!` expressions whose produced value is
//! exactly one literal.
//!
//! The literal census itself lives in `super::classify`; this module
//! keeps only the value/position predicates both passes need.

use std::path::PathBuf;

/// One literal diagnostic code with its source site.
pub(super) struct CodeSite {
    pub file: PathBuf,
    pub line: usize,
    pub code: String,
    pub scan: &'static str,
}

/// Peel fixed no-code-value wrappers once: `Some(…)`, `&`, `Ok(…)`,
/// `String::from(…)`, `str::to_string(…)`, and `.to_string()`/
/// `.to_owned()`/`.into()`/`.clone()`/`.as_str()`/`.as_mut_str()`/
/// `.trim()` suffixes. Returns the inner expression, or `None`.
fn peel_once(expr: &str) -> Option<&str> {
    let t = expr.trim();
    for (pre, suf) in [
        ("Some(", ")"),
        ("Ok(", ")"),
        ("Err(", ")"),
        ("String::from(", ")"),
        ("str::to_string(", ")"),
        ("Box::new(", ")"),
    ] {
        if let Some(inner) = t.strip_prefix(pre).and_then(|s| s.strip_suffix(suf)) {
            return Some(inner.trim());
        }
    }
    if let Some(inner) = t.strip_prefix('&') {
        return Some(inner.trim());
    }
    for suf in [
        ".to_string()",
        ".to_owned()",
        ".into()",
        ".clone()",
        ".as_str()",
        ".as_mut_str()",
        ".trim()",
        ".unwrap()",
        ".ok()",
    ] {
        if let Some(inner) = t.strip_suffix(suf) {
            return Some(inner.trim());
        }
    }
    // `.expect(msg)` — the payload is the code, the arg a panic message.
    if let Some(inner) = t.strip_suffix(')').and_then(|s| s.rsplit_once(".expect(")) {
        let (_, base) = inner;
        return Some(base.trim());
    }
    None
}

/// If `expr` is syntactically a single string literal — after peeling
/// wrappers, a `format!`/`concat!` of brace-free literals, or a plain
/// literal — its cooked content. `Some(None)` is "not a single literal".
pub(super) fn single_literal(expr: &str) -> Option<Option<String>> {
    let mut e = expr.trim();
    for _ in 0..10 {
        match peel_once(e) {
            Some(inner) => e = inner,
            None => break,
        }
    }
    // `format!("lit")` with no holes emits exactly the literal; a
    // `format!` whose template carries `{`/`}` holes is dynamic.
    if let Some(inner) = e
        .strip_prefix("format!")
        .and_then(|s| s.trim_start().strip_prefix('('))
        .and_then(|s| s.trim_end().strip_suffix(')'))
    {
        return match single_literal(inner.trim()) {
            Some(Some(lit)) if !lit.contains('{') && !lit.contains('}') => Some(Some(lit)),
            _ => Some(None),
        };
    }
    // `concat!("a", "b", …)` emits the joined literal.
    if let Some(inner) = e
        .strip_prefix("concat!")
        .and_then(|s| s.trim_start().strip_prefix('('))
        .and_then(|s| s.trim_end().strip_suffix(')'))
    {
        let mut out = String::new();
        for part in inner.split(',') {
            match single_literal(part.trim()) {
                Some(Some(lit)) => out.push_str(&lit),
                _ => return Some(None),
            }
        }
        return Some(Some(out));
    }
    // Raw strings: `r"…"` and `r#"…"#` carry no escapes.
    if let Some(rest) = e.strip_prefix('r') {
        let (hashes, rest) = rest.split_once('"')?;
        if !hashes.chars().all(|c| c == '#') || hashes.len() > 3 {
            return Some(None);
        }
        let close = format!("\"{}", "#".repeat(hashes.len()));
        let inner = rest.strip_suffix(close.as_str())?;
        return Some(Some(inner.to_string()));
    }
    let inner = e.strip_prefix('"')?.strip_suffix('"')?;
    // An escaped or interior quote means the span is not one literal.
    if inner.contains('\\') || inner.contains('"') {
        return Some(None);
    }
    Some(Some(inner.to_string()))
}

/// Whether the masked shape could still carry a string — used to skip
/// callsite arguments that obviously are not (`vec![…]`, `{ … }`,
/// array literals, plain numbers) for same-named sinks.
pub(super) fn plausibly_string(shape: &str) -> bool {
    let shape = shape.trim_start();
    !(shape.starts_with("vec!")
        || shape.starts_with('[')
        || shape.starts_with('{')
        || shape.chars().next().is_some_and(|c| c.is_ascii_digit()))
}

/// Whether a value expression names a code channel — used only to
/// disambiguate same-named std-method callsites
/// (`registry.contains(code)` vs `set.contains(&"risks")`). A literal or
/// a `format!` arg to an ambiguous name could be any string, so only an
/// identifier carrying a code-channel name qualifies.
pub(super) fn arg_names_code_channel(shape: &str) -> bool {
    let shape = shape.trim_start_matches(['&', '*']);
    let last = shape.rsplit(['.', ':']).next().unwrap_or(shape);
    !shape.starts_with('"')
        && !shape.contains('(')
        && (last == "code" || last == "id" || last.ends_with("_code"))
}

/// Whether `name` binds a code channel — `code`, `.._code` (but not
/// `by_code`, an entity channel), or the analyzer's rule-code `id`.
pub(super) fn is_code_field(name: &str, analyzer: bool) -> bool {
    if name == "code" || (name.ends_with("_code") && !name.ends_with("by_code")) {
        return true;
    }
    analyzer && name == "id"
}

/// Whether `name` declares a code channel as a binding. `code` is the
/// head noun when it ends the name — `code`, `provider_code`,
/// `ERR_CODE` — or leads only a piece qualifier — `CODE_PREFIX`,
/// `code_suffix`. A `code_`/`CODE_` prefix modifying another word
/// (`CODE_EXECUTION_CALLER`, `code_loading`) names something else, so
/// the binding defers to how its uses classify. `by_code` is an
/// entity channel (`is_code_field` agrees).
pub(super) fn code_binding_name(name: &str, analyzer: bool) -> bool {
    if analyzer && name == "id" {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    if lower == "by_code" || lower.ends_with("_by_code") || lower.starts_with("by_code_") {
        return false;
    }
    let segs: Vec<&str> = name.split('_').collect();
    segs.iter().enumerate().any(|(i, s)| {
        s.eq_ignore_ascii_case("code")
            && segs[i + 1..]
                .iter()
                .all(|s| s.eq_ignore_ascii_case("prefix") || s.eq_ignore_ascii_case("suffix"))
    })
}

/// The method names that mutate a `String` receiver's content —
/// `x.push_str(arg)`, `code.truncate(n)`, and friends. Readers and
/// predicates stay out.
pub(super) fn mutates_string(method: &str) -> bool {
    [
        "push",
        "push_str",
        "insert",
        "insert_str",
        "truncate",
        "clear",
        "replace_range",
        "clone_from",
        "pop",
        "remove",
        "extend",
        "write_str",
    ]
    .contains(&method)
}
