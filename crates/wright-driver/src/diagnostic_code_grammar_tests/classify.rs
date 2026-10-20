//! The classifiers behind `super::driver_codes::driver_code_census`: every code
//! position the scans surface resolves to a literal (censused), a
//! verified producer/passthrough, or an unclassified site that fails the
//! test — enumerate-or-fail, never silent.

use std::path::{Path, PathBuf};

use super::source::{
    arm_pattern_binds, as_str, call_args, closure_param_binds, enclosing_fn, enclosing_fn_body,
    line_of, match_arms_stage_ok, match_bracket, rhs_span, skeleton,
};

/// One literal found in a code position, with its site for diagnostics.
pub(super) struct CodeSite {
    pub(super) file: PathBuf,
    pub(super) line: usize,
    pub(super) code: String,
}

/// The census accumulators and the file under scan, shared by the
/// code-position classifiers.
pub(super) struct Sites<'a> {
    pub(super) codes: &'a mut Vec<CodeSite>,
    pub(super) unclassified: &'a mut Vec<String>,
    pub(super) original: &'a str,
    pub(super) masked: &'a [u8],
    pub(super) file: &'a Path,
    /// Field/variable names that carry diagnostic codes. Only `code`:
    /// `id` is overloaded — analyzer rule codes live in `id:` literals
    /// (censused separately) while `Symbol.id`/`RuleDefinition.id` and
    /// JSON-RPC ids are entity ids, not diagnostic codes.
    pub(super) code_names: &'a [&'a str],
    /// `fn`s returning `&str`/`String` — outermost calls to them in a code
    /// position produce codes through their scanned bodies.
    pub(super) code_fns: &'a std::collections::BTreeSet<String>,
}

/// A code position inside a `(expr, Stage::…)` tuple: a bare `code`/`id`
/// is read from the match arm's own pattern (passthrough); anything else
/// is classified like any code position.
pub(super) fn classify_tuple_code(sites: &mut Sites<'_>, start: usize, end: usize) {
    let shape = skeleton(&sites.original[start..end]);
    if sites.code_names.contains(&shape.as_str()) && arm_pattern_binds(sites.masked, start, &shape)
    {
        return;
    }
    classify_rhs(sites, start, end);
}

/// A code position in a `fn <name>code` `=>` arm value: same arm-pattern
/// rule as tuple positions.
pub(super) fn classify_arm_value(sites: &mut Sites<'_>, arm_at: usize, start: usize, end: usize) {
    let shape = skeleton(&sites.original[start..end]);
    if sites.code_names.contains(&shape.as_str()) && arm_pattern_binds(sites.masked, arm_at, &shape)
    {
        return;
    }
    classify_rhs(sites, start, end);
}

/// One top-level field segment of a struct literal. A segment that trims
/// to exactly a code name is a shorthand binding to resolve.
pub(super) fn check_shorthand_field(sites: &mut Sites<'_>, start: usize, end: usize, at: usize) {
    let field = as_str(sites.masked, start..end).trim();
    if sites.code_names.contains(&field) {
        resolve_code_binding(sites, at, field);
    }
}

/// Extract the leading string literal of a code-position expression
/// (`"lit"`, `Some("lit")`, `"lit".to_string()`); returns whether one was
/// found and, when found, pushes it onto `codes`.
pub(super) fn push_literal(
    codes: &mut Vec<CodeSite>,
    original: &str,
    file: &Path,
    span: (usize, usize),
) -> bool {
    let text = original[span.0..span.1].trim();
    let inner = text.strip_prefix("Some(").unwrap_or(text).trim_start();
    let Some(rest) = inner.strip_prefix('"') else {
        return false;
    };
    let Some(end) = rest.find('"') else {
        return false;
    };
    codes.push(CodeSite {
        file: file.to_path_buf(),
        line: line_of(original, span.0),
        code: rest[..end].to_string(),
    });
    true
}

/// Literals nested inside `unwrap_or("…")`/`unwrap_or_else(|| "…"…)`/
/// `or("…")`/`or_else(|| "…")` fallbacks of a code-position expression are
/// themselves possible codes — census them.
fn census_fallback_literals(sites: &mut Sites<'_>, start: usize, end: usize) {
    let expr = &sites.original[start..end];
    for marker in ["unwrap_or(", "||", "or(", "or_else("] {
        for (off, _) in expr.match_indices(marker) {
            let lit_at = start + off + marker.len();
            let rest = expr[off + marker.len()..].trim_start();
            // `|| format!("…")` — the closure body is a format literal.
            if let Some(fmt) = rest.strip_prefix("format!") {
                let fmt_start = lit_at + (expr[off + marker.len()..].len() - rest.len());
                if fmt.starts_with('(')
                    && let Some(open) = sites.masked[fmt_start..]
                        .iter()
                        .position(|&b| b == b'(')
                        .map(|p| fmt_start + p)
                    && let Some(close) = match_bracket(sites.masked, open)
                {
                    check_format_literal(sites, fmt_start, close + 1);
                }
                continue;
            }
            let Some(inner) = rest.strip_prefix('"') else {
                continue;
            };
            let Some(width) = inner.find('"') else {
                continue;
            };
            let literal = &inner[..width];
            if literal.is_empty() {
                continue;
            }
            sites.codes.push(CodeSite {
                file: sites.file.to_path_buf(),
                line: line_of(sites.original, lit_at),
                code: literal.to_string(),
            });
        }
    }
}

/// Classify one code-position RHS: a literal enters the census; a bare
/// `code`/`id` (after peeling wrappers) resolves through the enclosing
/// fn's binding; `format!` literals are grammar-checked around their
/// holes; remaining shapes must match a verified producer or passthrough.
pub(super) fn classify_rhs(sites: &mut Sites<'_>, start: usize, end: usize) {
    if start >= end {
        return;
    }
    if push_literal(sites.codes, sites.original, sites.file, (start, end)) {
        return;
    }
    census_fallback_literals(sites, start, end);
    let expr = &sites.original[start..end];
    // Peel wrappers — `Some(…)`, `.to_string()`/`.into()`/`.clone()`/
    // `.as_deref()`/`.as_str()`/`.unwrap_or_default()` suffixes, and
    // `unwrap_or*`/`or*`/`or_else` fallback tails — checking the peeled
    // shape after every step so `Some(code)` and `code.to_string()` both
    // reach binding resolution instead of a shape whitelist.
    let mut normalized: String = expr.chars().filter(|c| !c.is_whitespace()).collect();
    loop {
        while let Some(rest) = normalized
            .strip_prefix('&')
            .or_else(|| normalized.strip_prefix('*'))
        {
            normalized = rest.to_string();
        }
        if normalized.starts_with("format!(") && normalized.ends_with(')') {
            check_format_literal(sites, start, end);
            return;
        }
        let shape = skeleton(&normalized);
        if sites.code_names.contains(&shape.as_str()) {
            resolve_code_binding(sites, start, &shape);
            return;
        }
        if producer(&normalized, sites.code_fns) || passthrough(&shape) {
            return;
        }
        if normalized.starts_with("Some(") && normalized.ends_with(')') {
            normalized = normalized["Some(".len()..normalized.len() - 1].to_string();
            continue;
        }
        let mut peeled = false;
        for suffix in [
            ".to_string()",
            ".into()",
            ".clone()",
            ".as_deref()",
            ".as_str()",
            ".unwrap_or_default()",
        ] {
            if let Some(rest) = normalized.strip_suffix(suffix) {
                normalized = rest.to_string();
                peeled = true;
                break;
            }
        }
        if !peeled && normalized.ends_with(')') {
            for cut in [".unwrap_or_else(", ".unwrap_or(", ".or_else(", ".or("] {
                if let Some(index) = normalized.rfind(cut) {
                    normalized.truncate(index);
                    peeled = true;
                    break;
                }
            }
        }
        if !peeled {
            break;
        }
    }
    sites.unclassified.push(format!(
        "{}:{}: {}",
        sites.file.display(),
        line_of(sites.original, start),
        expr.trim()
    ));
}

/// Verified code producers — the whole peeled expression must be the
/// call (`X::provider_failure_code(…)`, `diagnostic_code(…)`, or any
/// `fn` returning `&str`/`String` in the scanned crates) or an enumerated
/// constant/passthrough read. A producer nested inside a wrapper that
/// changes its output (e.g. inside `format!` args) never matches —
/// `format!` goes through `check_format_literal` instead.
fn producer(normalized: &str, code_fns: &std::collections::BTreeSet<String>) -> bool {
    if normalized == "UNKNOWN_WITHIN" || normalized == "finding[\"code\"]" {
        return true;
    }
    // Outermost call `path::name(args)` — args may be anything; the call
    // must span the whole expression.
    let Some(open) = normalized.find('(') else {
        return false;
    };
    if !normalized.ends_with(')') {
        return false;
    }
    let callee = &normalized[..open];
    let qualifier_ok = callee
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':' || c == '!');
    if !qualifier_ok {
        return false;
    }
    let basename = callee.rsplit("::").next().unwrap_or(callee);
    basename == "provider_failure_code"
        || basename == "diagnostic_code"
        || code_fns.contains(basename)
}

/// Passthrough reads of already-enumerated or owner/provider-spelled
/// codes. Bare `code`/`id` never reach here — `classify_rhs` resolves
/// their bindings first.
fn passthrough(shape: &str) -> bool {
    shape.ends_with(".code")
        || shape.ends_with(".code()")
        || shape.ends_with(".refusal_code()")
        || shape.ends_with(".provider_code")
        || shape.ends_with(".provider_code()")
        || shape.ends_with(".id")
        || shape.ends_with(".id()")
        || shape == "finding[\"\"]" // skeleton of `finding["code"]`
}

/// A `format!(…)` in a code position: its format literal must consist
/// only of grammar-valid characters around `{…}` holes — a space,
/// uppercase letter, or stray punctuation in the literal produces a
/// malformed code no producer whitelist may hide.
fn check_format_literal(sites: &mut Sites<'_>, start: usize, end: usize) {
    let expr = &sites.original[start..end];
    let Some(open) = expr.find('(') else {
        return;
    };
    let args = expr[open + 1..].trim_start();
    let Some(rest) = args.strip_prefix('"') else {
        sites.unclassified.push(format!(
            "{}:{}: `format!` code position without a literal format string",
            sites.file.display(),
            line_of(sites.original, start)
        ));
        return;
    };
    let mut i = 0;
    let bytes = rest.as_bytes();
    let mut bad = false;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => break,
            b'\\' => i += 2, // keep escapes simple — `\n` is not a code char anyway
            b'{' => {
                // Skip the format hole `{…}`.
                i += 1;
                while i < bytes.len() && bytes[i] != b'}' {
                    i += 1;
                }
                i += 1;
            }
            b => {
                if !(b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-') {
                    bad = true;
                    break;
                }
                i += 1;
            }
        }
    }
    if bad {
        sites.unclassified.push(format!(
            "{}:{}: `format!` literal produces a grammar-invalid code: {}",
            sites.file.display(),
            line_of(sites.original, start),
            expr.trim()
        ));
    }
}

/// Resolve what a bare `code`/`id` value (shorthand field, ctor argument,
/// or `code:`/`id:` field) binds inside the enclosing fn. Every `let`
/// binding and bare `name =` rebind in the fn body is classified — a
/// shadowing inner `let` can not hide behind an outer parameter. With no
/// binding in the body, a same-named parameter passes through a value
/// whose producer is covered at the caller, a match-arm pattern binding
/// defers to the `, Stage::`/arm scans, and anything else is
/// unclassified.
fn resolve_code_binding(sites: &mut Sites<'_>, at: usize, name: &str) {
    let unclassified_at = |sites: &mut Sites<'_>, why: &str| {
        sites.unclassified.push(format!(
            "{}:{}: `{name}` value {why}",
            sites.file.display(),
            line_of(sites.original, at)
        ));
    };
    let Some((body, body_end)) = enclosing_fn_body(sites.masked, at) else {
        unclassified_at(sites, "outside a fn");
        return;
    };
    let text = as_str(sites.masked, 0..sites.masked.len());
    let mut bound = false;
    for (let_at, _) in text[body..body_end].match_indices("let ") {
        let stmt = body + let_at + "let ".len();
        // The `let` pattern runs to a top-level `=` or `;`.
        let mut depth = 0i32;
        let mut pend = stmt;
        let mut eq = None;
        for j in stmt..body_end {
            match sites.masked[j] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' if depth > 0 => depth -= 1,
                b'=' if depth == 0 && sites.masked.get(j + 1) != Some(&b'=') => {
                    pend = j;
                    eq = Some(j);
                    break;
                }
                b';' if depth == 0 => {
                    pend = j;
                    break;
                }
                _ => {}
            }
        }
        let pattern = text[stmt..pend].trim_end_matches(|c: char| c.is_ascii_whitespace());
        let Some(eq) = eq else {
            continue;
        };
        // Find where the pattern binds the name (word boundary).
        let Some(name_at) = pattern.match_indices(name).map(|(i, _)| i).find(|&i| {
            let before = pattern[..i].chars().last();
            let after = pattern[i + name.len()..].chars().next();
            !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                && !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        }) else {
            continue;
        };
        bound = true;
        // `name` inside `{…}`/`(…)` in the pattern destructures an
        // already-produced value — classify the RHS unless it is a match
        // feeding the `, Stage::` arm scan, in which case every arm must
        // carry a Stage marker.
        let shallow = !pattern[..name_at].trim_end().contains(['{', '(']);
        let (rstart, rend) = rhs_span(sites.masked, eq, body_end);
        if shallow {
            // A `name`-to-`name` binding traces to an earlier binding this
            // scan already classifies.
            if skeleton(&sites.original[rstart..rend]) != name {
                classify_rhs(sites, rstart, rend);
            }
        } else {
            // Destructuring binds a value produced elsewhere:
            // - a `match` RHS feeds the `, Stage::` arm scan — every arm
            //   must carry the marker;
            // - a tuple RHS with `, Stage::` is covered by the tuple scan;
            // - a `Some(`/enum/tuple constructor RHS produces the value
            //   right here — classify it so `Some("bad code")` is
            //   censused;
            // - anything else (param, call, field read) was produced at
            //   its own `code:`/`id:`/shorthand write site.
            let rhs_text = &text[eq + 1..rend];
            if rhs_text.contains("match") {
                if !match_arms_stage_ok(rhs_text) {
                    unclassified_at(sites, "bound by a match arm the Stage:: scan misses");
                }
            } else if !rhs_text.contains(", Stage::") {
                let skel = skeleton(&sites.original[rstart..rend]);
                if skel.starts_with("Some(")
                    || skel.starts_with("Ok(")
                    || skel.starts_with("Err(")
                    || skel.starts_with('(')
                {
                    classify_rhs(sites, rstart, rend);
                }
            }
        }
    }
    // Whether `name` is bound as a code channel in this fn: a `let`
    // binding (classified above), a string-typed parameter (`code: i64`
    // in a JSON-RPC helper is not a code channel), a match-arm pattern
    // binding, or a `|…|` closure parameter.
    let param = enclosing_fn(sites.masked, at).and_then(|(_, params)| {
        params
            .iter()
            .find(|(p, _)| p == name)
            .map(|(_, t)| t.clone())
    });
    let param_is_string = param
        .as_deref()
        .is_some_and(|t| t.contains("str") || t.contains("String") || t.contains("Into"));
    let bound_elsewhere =
        arm_pattern_binds(sites.masked, at, name) || closure_param_binds(sites.masked, at, name);
    let code_channel = bound || param_is_string || bound_elsewhere;

    // Mutating a code binding produces codes: `code = …`, `code += …`,
    // `code.push_str(…)`, `code.push(…)`, `code.insert(…)`, `code.extend(…)`,
    // `code.replace_range(…)` — every operand is a code position. A
    // `name` inside a `let` pattern is a binding site, not a mutation.
    if code_channel {
        for (name_at, _) in text[body..body_end].match_indices(name) {
            let a = body + name_at;
            if a > 0
                && (sites.masked[a - 1].is_ascii_alphanumeric()
                    || sites.masked[a - 1] == b'_'
                    || sites.masked[a - 1] == b'.')
            {
                continue;
            }
            // Inside a `let` pattern? The pattern runs to a top-level
            // `=`/`;`; anything before that binds rather than mutates.
            if let Some(lt) = text[body..a].rfind("let ") {
                let lt = body + lt;
                let mut d = 0i32;
                let mut in_pattern = true;
                for j in lt + 4..a {
                    match sites.masked[j] {
                        b'(' | b'[' | b'{' => d += 1,
                        b')' | b']' | b'}' => d -= 1,
                        b'=' | b';' if d == 0 => {
                            in_pattern = false;
                            break;
                        }
                        _ => {}
                    }
                }
                if in_pattern {
                    continue;
                }
            }
            let mut i = a + name.len();
            while i < body_end && sites.masked[i].is_ascii_whitespace() {
                i += 1;
            }
            match masked_get(sites.masked, i) {
                Some(b'=')
                    if masked_get(sites.masked, i + 1) != Some(b'=')
                        && masked_get(sites.masked, i + 1) != Some(b'>') =>
                {
                    let (rstart, rend) = rhs_span(sites.masked, i, body_end);
                    if plausibly_string(&skeleton(&sites.original[rstart..rend])) {
                        classify_rhs(sites, rstart, rend);
                    }
                }
                Some(b'+') if masked_get(sites.masked, i + 1) == Some(b'=') => {
                    let (rstart, rend) = rhs_span(sites.masked, i + 1, body_end);
                    if plausibly_string(&skeleton(&sites.original[rstart..rend])) {
                        classify_rhs(sites, rstart, rend);
                    }
                }
                Some(b'.') => {
                    for method in ["push_str(", "push(", "insert(", "extend(", "replace_range("] {
                        let j = i + 1;
                        if sites.masked[j..].starts_with(method.as_bytes()) {
                            let open = j + method.len() - 1;
                            for arg in call_args(sites.masked, open) {
                                if plausibly_string(&skeleton(&sites.original[arg.0..arg.1])) {
                                    classify_rhs(sites, arg.0, arg.1);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    if bound || param.is_some() || bound_elsewhere {
        return;
    }
    unclassified_at(sites, &format!("has no `{name}` binding in its fn"));
}

/// Whether a callsite argument can possibly be a `&str`/`String`/
/// `Into<String>` value — literals, idents, paths, `format!`, method
/// calls, `&`-references all can; `vec![…]`, `json!(…)`, `[…]`, `{…}`,
/// and numbers cannot. Used to disambiguate same-named fns whose `code`
/// slot is not a string channel, and to skip non-string operands of
/// `code` mutations.
pub(super) fn plausibly_string(shape: &str) -> bool {
    !shape.starts_with("vec!")
        && !shape.starts_with("json!")
        && !shape.starts_with('[')
        && !shape.starts_with('{')
        && !shape
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit() || (c == '-' && shape.len() > 1))
}

fn masked_get(masked: &[u8], i: usize) -> Option<u8> {
    masked.get(i).copied()
}
