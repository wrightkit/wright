//! `format!` classification for `super::driver_codes::driver_code_census`:
//! the format literal itself is grammar-checked around `{…}` holes, and
//! every substituted value — positional arguments, `name = expr`
//! arguments, `{name}` bindings, and `{CONST}` fragments — must classify.

use super::classify::{Sites, is_entity_id, passthrough, producer, push_literal};
use super::patterns::{arm_pattern_binds, closure_param_binds, for_pattern_binds};
use super::resolve::resolve_code_binding;
use super::source::{as_str, enclosing_fn, enclosing_fn_body, line_of, match_bracket, skeleton};

/// A `format!(…)` in a code position: its format literal must consist
/// only of grammar-valid characters around `{…}` holes, and every value
/// the holes substitute — positional arguments, `name = expr` arguments,
/// and `{name}` bindings — must itself classify.
pub(super) fn check_format_literal(sites: &mut Sites<'_>, start: usize, end: usize) {
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
    let abs_open = start + open;
    let Some(close) = match_bracket(sites.masked, abs_open) else {
        return;
    };
    let args_end = split_args(sites.masked, abs_open + 1, close);
    finish_fmt(sites, rest, &args_end[1..], start, expr.trim());
}

/// `write!(buf, "…", args…)` / `writeln!(buf, "…", args…)` on a code
/// channel: `arg_spans` is the call's argument list — element 0 is the
/// buffer (the caller already proved it targets a code), element 1 is
/// the format literal, and the rest are the substituted values.
pub(super) fn check_write_fmt(sites: &mut Sites<'_>, arg_spans: &[(usize, usize)], at: usize) {
    let Some(&(ls, le)) = arg_spans.get(1) else {
        return;
    };
    let lit = sites.original[ls..le].trim();
    let Some(rest) = lit.strip_prefix('"') else {
        sites.unclassified.push(format!(
            "{}:{}: `write!`/`writeln!` code position without a literal format string",
            sites.file.display(),
            line_of(sites.original, at)
        ));
        return;
    };
    finish_fmt(sites, rest, &arg_spans[2..], at, lit);
}

/// Shared format-validation: `rest` is the literal text after its
/// opening quote (on `original`), `fmt_args` the argument spans whose
/// values substitute into holes, and `shown` the expression reported in
/// diagnostics.
fn finish_fmt(
    sites: &mut Sites<'_>,
    rest: &str,
    fmt_args: &[(usize, usize)],
    at: usize,
    shown: &str,
) {
    let mut i = 0;
    let bytes = rest.as_bytes();
    let mut bad = false;
    let mut named: Vec<String> = Vec::new();
    while i < bytes.len() {
        match bytes[i] {
            b'"' => break,
            b'\\' => i += 2, // keep escapes simple — `\n` is not a code char anyway
            // `{{`/`}}` emit literal braces into the produced code — a
            // brace is never a grammar-valid code character.
            b'{' if bytes.get(i + 1) == Some(&b'{') => {
                bad = true;
                break;
            }
            b'}' if bytes.get(i + 1) == Some(&b'}') => {
                bad = true;
                break;
            }
            b'{' => {
                // A format hole `{…}` — `{name}`/`{name:spec}` resolve a
                // binding, `{0}`/`{}` refer to positional args, and
                // `{p.field}`/`{v[0]}`/`{v.0}` resolve their base ident.
                i += 1;
                let hole_start = i;
                while i < bytes.len() && bytes[i] != b'}' {
                    i += 1;
                }
                let hole = &rest[hole_start..i.min(bytes.len())];
                let name = hole.split(':').next().unwrap_or("").trim();
                let base = name
                    .split(['.', '['])
                    .next()
                    .unwrap_or("")
                    .trim_start_matches(['&', '*']);
                if !base.is_empty()
                    && !base.chars().all(|c| c.is_ascii_digit())
                    && base.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && base
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                {
                    named.push(base.to_string());
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
            line_of(sites.original, at),
            shown
        ));
        return;
    }
    // `name = expr` arguments define `{name}` holes — record them before
    // resolving `named`.
    let mut arg_names: Vec<String> = Vec::new();
    for &(s, e) in fmt_args {
        if s < e && !sites.original[s..e].trim().is_empty() {
            if let Some(name) = format_arg_name(sites, s, e) {
                arg_names.push(name);
            }
            classify_format_arg(sites, s, e);
        }
    }
    named.retain(|name| !arg_names.iter().any(|n| n == name));
    // `{name}` holes resolve the binding that carries the substituted
    // segment — `const` fragments through literal char validation,
    // everything else through the full resolver (a `let seg = "bad"`
    // behind `{seg}` has its literal censused; an unbound hole fails).
    for name in named {
        if !const_hole_checked(sites, &name, at) {
            resolve_code_binding(sites, at, &name);
        }
    }
}

/// Top-level `,`-separated argument spans of the call whose `(` sits at
/// `abs_open` (on the masked text — literal interiors are blanked, so
/// no literal comma can split a segment). The closing bracket sits at
/// `close`.
fn split_args(masked: &[u8], abs_open: usize, close: usize) -> Vec<(usize, usize)> {
    let mut depth = 0i32;
    let mut seg_start = abs_open;
    let mut args_end = Vec::new();
    for (j, &b) in masked.iter().enumerate().take(close).skip(abs_open) {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                args_end.push((seg_start, j));
                seg_start = j + 1;
            }
            _ => {}
        }
    }
    args_end.push((seg_start, close));
    args_end
}

/// Whether `{name}` in a `format!` code names a `const … = "…"` whose
/// literal is grammar-valid fragment text — e.g. `provider-refusal-`
/// prefixes substituted into a code. A non-literal const fails closed;
/// returns false when no such const exists so the caller can fall back
/// to binding resolution.
fn const_hole_checked(sites: &mut Sites<'_>, name: &str, at: usize) -> bool {
    let text = as_str(sites.masked, 0..sites.masked.len());
    let needle = format!("const {name}");
    for (off, _) in text.match_indices(&needle) {
        // `const NAME` at a word boundary — `const NAMEX` must not match.
        if text
            .as_bytes()
            .get(off + needle.len())
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            continue;
        }
        // `const NAME [: ty] = "lit"` — the literal sits after `=`.
        let mut eq = None;
        for j in off + needle.len()..text.len() {
            match text.as_bytes()[j] {
                b'=' => {
                    eq = Some(j);
                    break;
                }
                b';' | b'{' | b'}' => break,
                _ => {}
            }
        }
        let Some(eq) = eq else {
            continue;
        };
        let mut q = eq + 1;
        while q < text.len() && text.as_bytes()[q].is_ascii_whitespace() {
            q += 1;
        }
        if text.as_bytes().get(q) != Some(&b'"') {
            sites.unclassified.push(format!(
                "{}:{}: `format!` hole `{{{name}}}` names a non-literal const",
                sites.file.display(),
                line_of(sites.original, at)
            ));
            return true;
        }
        let Some(close) = text[q + 1..].find('"').map(|p| q + 1 + p) else {
            continue;
        };
        let literal = &sites.original[q + 1..close];
        let valid = literal
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-');
        if !valid {
            sites.unclassified.push(format!(
                "{}:{}: `format!` hole `{{{name}}}` const literal produces a grammar-invalid code",
                sites.file.display(),
                line_of(sites.original, at)
            ));
        }
        return true;
    }
    false
}

/// One `format!` argument that lands inside a code: a literal is
/// censused, a producer/passthrough passes, `name = expr` recurses on
/// the value, and anything peelable to a bound ident (a method chain on
/// an enumerated producer, like `kind.replace(' ', "-")`) passes — the
/// rest fails closed.
fn classify_format_arg(sites: &mut Sites<'_>, start: usize, end: usize) {
    if push_literal(sites.codes, sites.original, sites.file, (start, end)) {
        return;
    }
    // `a + b` concatenation — every operand classifies on its own.
    {
        let mut depth = 0i32;
        let mut plus = None;
        for i in start..end {
            match sites.masked[i] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b'+' if depth == 0
                    && sites.masked.get(i + 1) != Some(&b'=')
                    && sites.masked.get(i + 1) != Some(&b'+')
                    && sites.masked.get(i.wrapping_sub(1)) != Some(&b'+') =>
                {
                    plus = Some(i);
                    break;
                }
                _ => {}
            }
        }
        if let Some(i) = plus {
            classify_plus_segment(sites, start, i);
            classify_plus_segment(sites, i + 1, end);
            return;
        }
    }
    let shape = skeleton(&sites.original[start..end]);
    if producer(&shape, sites.code_fns) || passthrough(&shape) || is_entity_id(&shape) {
        return;
    }
    // `name = expr` named argument — classify the value.
    if let Some(eq) = format_arg_eq(sites, start, end) {
        classify_format_arg(sites, eq + 1, end);
        return;
    }
    // Peel `&`/`*` to find the leading ident.
    let trimmed = shape.trim_start_matches(['&', '*']);
    let base_len = trimmed
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(trimmed.len());
    let base = &trimmed[..base_len];
    if !base.is_empty()
        && base
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
    {
        // A bare-ident argument IS the substituted value — resolve its
        // binding so `let bad = "BAD SEG"` behind `format!("{}", bad)`
        // has its literal censused. A method/field tail (`kind.replace`,
        // `e.kind`) transforms a bound value produced elsewhere.
        if base_len == trimmed.len() {
            resolve_code_binding(sites, start, base);
        } else if !bound_in_fn(sites, start, base) {
            sites.unclassified.push(format!(
                "{}:{}: `format!` argument `{base}` has no binding in its fn",
                sites.file.display(),
                line_of(sites.original, start)
            ));
        }
        return;
    }
    sites.unclassified.push(format!(
        "{}:{}: `format!` argument does not trace to a code producer: {}",
        sites.file.display(),
        line_of(sites.original, start),
        sites.original[start..end].trim()
    ));
}

/// Whether the `start..end` argument is `name = expr` — and if so the
/// name. The `=` must sit at depth 0 on the masked text (`==`/`=>` do
/// not count).
fn format_arg_name(sites: &Sites<'_>, start: usize, end: usize) -> Option<String> {
    let eq = format_arg_eq(sites, start, end)?;
    let name = as_str(sites.masked, start..eq).trim();
    (!name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_'))
    .then(|| name.to_string())
}

/// Position of the `=` inside a `name = expr` format argument (masked,
/// depth 0, `==`/`=>` excluded), or `None` for a positional argument.
fn format_arg_eq(sites: &Sites<'_>, start: usize, end: usize) -> Option<usize> {
    let mut depth = 0i32;
    for j in start..end {
        match sites.masked[j] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'=' if depth == 0 => {
                if sites.masked.get(j + 1) == Some(&b'=')
                    || sites.masked.get(j + 1) == Some(&b'>')
                    || (j > 0 && sites.masked.get(j - 1) == Some(&b'='))
                {
                    continue;
                }
                return Some(j);
            }
            _ => {}
        }
    }
    None
}

/// A `+`-concatenated format argument: a literal is a fragment — its
/// characters must be grammar-valid but it is not a whole code;
/// anything else classifies as an argument.
fn classify_plus_segment(sites: &mut Sites<'_>, start: usize, end: usize) {
    let text = sites.original[start..end]
        .trim()
        .trim_start_matches(['&', '*'])
        .trim_start();
    if let Some(rest) = text.strip_prefix('"')
        && let Some(close) = rest.find('"')
    {
        if !rest[..close]
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
        {
            sites.unclassified.push(format!(
                "{}:{}: `+` operand literal produces a grammar-invalid code: {}",
                sites.file.display(),
                line_of(sites.original, start),
                &rest[..close]
            ));
        }
        return;
    }
    classify_format_arg(sites, start, end);
}

/// Whether `name` is bound anywhere in scope at `at`: a `let` pattern in
/// the enclosing fn body, a parameter, a match-arm pattern, or a `|…|`
/// closure parameter.
fn bound_in_fn(sites: &Sites<'_>, at: usize, name: &str) -> bool {
    if arm_pattern_binds(sites.masked, at, name)
        || closure_param_binds(sites.masked, at, name)
        || for_pattern_binds(sites.masked, at, name)
    {
        return true;
    }
    if enclosing_fn(sites.masked, at)
        .is_some_and(|(_, params)| params.iter().any(|(p, _)| p == name))
    {
        return true;
    }
    let Some((body, body_end)) = enclosing_fn_body(sites.masked, at) else {
        return false;
    };
    let text = as_str(sites.masked, 0..sites.masked.len());
    text[body..body_end]
        .match_indices("let ")
        .any(|(let_at, _)| {
            // `let ` at a word boundary — `outlet `/`tablet ` can not
            // fake a binding.
            if let_at > 0 {
                let prev = sites.masked[body + let_at - 1];
                if prev.is_ascii_alphanumeric() || prev == b'_' {
                    return false;
                }
            }
            let stmt = body + let_at + "let ".len();
            let mut depth = 0i32;
            let mut pend = body_end;
            for j in stmt..body_end {
                match sites.masked[j] {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' if depth > 0 => depth -= 1,
                    b'=' | b';' if depth == 0 => {
                        pend = j;
                        break;
                    }
                    _ => {}
                }
            }
            let pattern = text[stmt..pend].trim_end();
            pattern.match_indices(name).any(|(i, _)| {
                let before = pattern[..i].chars().last();
                let after = pattern[i + name.len()..].chars().next();
                !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                    && !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            })
        })
}
