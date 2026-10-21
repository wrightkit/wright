//! Code-position handling for the census: every string literal inside a
//! code position enters the grammar-checked vocabulary — except literals
//! that are `format!`-style templates (`{`/`}` holes), whose produced
//! value the runtime assertions cover instead. The residual expression
//! shape is *reported* as a dynamic position, never classified.
//!
//! Dynamic positions are not silent: `debug_assert!` code-token checks
//! inside the production sinks (`Diagnostic::error`, `refusal`,
//! `provider_failure_code`, the LSP error writer) validate every code
//! they produce on any exercised path, regardless of how the value was
//! computed, and the engine-kind tests drive the `kind`-built shapes
//! through their producers.

use std::path::PathBuf;

use super::source::{line_of, match_bracket};

/// One literal diagnostic code with its source site.
pub(super) struct CodeSite {
    pub file: PathBuf,
    pub line: usize,
    pub code: String,
    pub scan: &'static str,
}

/// Per-file census state.
pub(super) struct Sites<'a> {
    pub codes: &'a mut Vec<CodeSite>,
    pub dynamic: &'a mut Vec<String>,
    pub original: &'a str,
    pub masked: &'a [u8],
    pub file: &'a PathBuf,
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
    ] {
        if let Some(inner) = t.strip_suffix(suf) {
            return Some(inner.trim());
        }
    }
    None
}

/// If `expr` is syntactically a single string literal — after peeling
/// wrappers, a `format!`/`concat!` of brace-free literals, or a plain
/// literal — its cooked content. `Some(None)` is "not a single literal".
fn single_literal(expr: &str) -> Option<Option<String>> {
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

/// Whether the `:`-scan hit at `at` sits in a destructure/pattern head —
/// `let { code: c }`, `for (k, v)`, `if let X { code: c } =`, a match-arm
/// pattern — rather than an expression position. Approximated by the
/// statement head: text from the last top-level `;`/`{`/`}`.
pub(super) fn in_pattern_position(masked: &[u8], at: usize) -> bool {
    let mut stmt = at;
    let mut depth = 0i32;
    while stmt > 0 {
        match masked[stmt - 1] {
            b'(' | b'[' => depth += 1,
            b')' | b']' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            b';' | b'{' | b'}' if depth == 0 => break,
            _ => {}
        }
        stmt -= 1;
    }
    let head = masked[stmt..at]
        .iter()
        .map(|&b| b as char)
        .collect::<String>();
    let head = head.trim_start();
    head.starts_with("let ") || head.starts_with("for ") || head.contains("=>")
}

/// A `name:`/`.._code:`/`id:` field name decides whether a `:`-scan hit is
/// a code position. `by_code`-style fields ("indexed by code") are entity
/// channels, not diagnostic codes.
pub(super) fn is_code_field(name: &str, analyzer: bool) -> bool {
    if name == "code" || (name.ends_with("_code") && !name.ends_with("by_code")) {
        return true;
    }
    analyzer && name == "id"
}

/// Literal spans reachable through transparent structure — wrappers
/// that return the value whole (`Some`, `Ok`, `&`, `.to_string()`), or
/// alternates that can *become* the produced value (`.unwrap_or*`/
/// `.or*`/`.then*` fallbacks, `if`/`else` arms, `match` arm bodies,
/// closures `|| "lit"`). Literals inside transforms (`format!` templates
/// and their args, `replace`, `+` concat pieces, push_str, index args)
/// are not standalone codes and stay out — the produced values are
/// reported dynamic.
fn literal_leaves(masked: &[u8], s0: usize, e0: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut stack = vec![(s0, e0)];
    while let Some((mut s, mut e)) = stack.pop() {
        for _ in 0..16 {
            while s < e && masked[s].is_ascii_whitespace() {
                s += 1;
            }
            while e > s && masked[e - 1].is_ascii_whitespace() {
                e -= 1;
            }
            // A trailing comma inside a wrapper (`Some("x",)`).
            while e > s && masked[e - 1] == b',' {
                e -= 1;
                while e > s && masked[e - 1].is_ascii_whitespace() {
                    e -= 1;
                }
            }
            if s >= e {
                break;
            }
            // `Some(`/…`(` call wrapper: peel when the call's `)`
            // closes the whole span and the callee is a transparent
            // wrapper — a bare ident or `::`-path.
            if masked[s].is_ascii_alphabetic() || masked[s] == b'_' {
                let mut h = s;
                while h < e
                    && (masked[h].is_ascii_alphanumeric() || masked[h] == b'_' || masked[h] == b':')
                {
                    h += 1;
                }
                if masked.get(h) == Some(&b'(') && match_bracket(masked, h) == Some(e - 1) {
                    let callee = masked[s..h].iter().map(|&b| b as char).collect::<String>();
                    if matches!(
                        callee.as_str(),
                        "Some"
                            | "Ok"
                            | "String::from"
                            | "str::to_string"
                            | "Box::new"
                            | "Cow::Borrowed"
                            | "Cow::Owned"
                    ) {
                        s = h + 1;
                        e -= 1;
                        continue;
                    }
                    break;
                }
            }
            if masked[s] == b'&' {
                s += 1;
                continue;
            }
            // Transparent method suffixes — the produced value is the
            // base unchanged.
            let mut stripped = false;
            for suf in [
                ".to_string()",
                ".to_owned()",
                ".into()",
                ".clone()",
                ".as_str()",
                ".as_mut_str()",
                ".trim()",
            ] {
                let su = suf.as_bytes();
                if e - s >= su.len() && masked[e - su.len()..e] == *su {
                    e -= su.len();
                    stripped = true;
                    break;
                }
            }
            if stripped {
                continue;
            }
            break;
        }
        // Span fully peeled — classify what remains.
        if s < e && masked[s] == b'"' && masked[e - 1] == b'"' {
            out.push((s, e));
            continue;
        }
        // `.method(arg)` chains: combinator args are produced values.
        for ae in combinator_fallback_args(masked, s, e) {
            stack.push(ae);
        }
        // `if c { a } else { b }` and `match x { p => a, _ => b }` —
        // every arm's body is a produced value.
        if masked[s..e].starts_with(b"if ") || masked[s..e].starts_with(b"if\t") {
            for span in if_arms(masked, s, e) {
                stack.push(span);
            }
            continue;
        }
        if masked[s..e].starts_with(b"match ") {
            for span in match_arm_bodies(masked, s, e) {
                stack.push(span);
            }
            continue;
        }
        // `[a, b]` / `(a, b)` containers — elements flow to the
        // bound/sink as produced values.
        if masked[s] == b'[' || masked[s] == b'(' {
            let close = match_bracket(masked, s).unwrap_or(e);
            for (a, b) in super::source::call_args(masked, s) {
                stack.push((a, b.min(close)));
            }
            continue;
        }
        // `vec![a, b]` — same for macro arrays.
        if masked[s].is_ascii_alphabetic() {
            let mut h = s;
            while h < e
                && (masked[h].is_ascii_alphanumeric() || masked[h] == b'_' || masked[h] == b'!')
            {
                h += 1;
            }
            if masked[h.saturating_sub(1)] == b'!'
                && masked.get(h) == Some(&b'[')
                && let Some(close) = match_bracket(masked, h)
            {
                for (a, b) in super::source::call_args(masked, h) {
                    stack.push((a, b.min(close)));
                }
                continue;
            }
        }
        // Closure `|| expr` / `|x| expr`.
        if masked[s] == b'|' {
            let mut i = s + 1;
            while i < e && masked[i] != b'|' {
                i += 1;
            }
            if i < e {
                stack.push((i + 1, e));
            }
            continue;
        }
        // Everything else — `format!`, `replace`, `+`, transforms — is
        // dynamic; no literal inside it is a standalone code.
    }
    out
}

/// Every `.combinator(arg)` in `masked[s..e]` whose argument is a
/// produced value (`unwrap_or`, `or_else`, `then`, …).
fn combinator_fallback_args(masked: &[u8], s: usize, e: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = s;
    while i < e {
        if masked[i] == b'.' && i + 1 < e && masked[i + 1].is_ascii_alphabetic() {
            let mut h = i + 1;
            while h < e && (masked[h].is_ascii_alphanumeric() || masked[h] == b'_') {
                h += 1;
            }
            if masked.get(h) == Some(&b'(')
                && let Some(close) = match_bracket(masked, h)
            {
                let method = masked[i + 1..h]
                    .iter()
                    .map(|&b| b as char)
                    .collect::<String>();
                if matches!(
                    method.as_str(),
                    "unwrap_or"
                        | "unwrap_or_else"
                        | "or"
                        | "or_else"
                        | "then_some"
                        | "then"
                        | "ok_or"
                        | "ok_or_else"
                ) {
                    let args = super::source::call_args(masked, h);
                    if let Some(&(a, b)) = args.first() {
                        out.push((a, b.min(close)));
                    }
                }
                i = close + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// `if … { a } else { b }`: the spans of the arm bodies (`if`/`else
/// if`/`else`).
fn if_arms(masked: &[u8], s: usize, e: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = s;
    // Skip the `if` head to the first `{`.
    while i < e && masked[i] != b'{' {
        i += 1;
    }
    while i < e {
        let Some(close) = match_bracket(masked, i) else {
            break;
        };
        out.push((i + 1, close));
        i = close + 1;
        while i < e && masked[i].is_ascii_whitespace() {
            i += 1;
        }
        // `else {` or `else if … {`
        if masked[i..e].starts_with(b"else") {
            i += 4;
            while i < e && masked[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < e && masked[i] == b'{' {
                continue;
            }
            // `else if` — skip to its `{`.
            while i < e && masked[i] != b'{' {
                i += 1;
            }
            continue;
        }
        break;
    }
    out
}

/// `match x { p => a, _ => b }`: the arm body spans.
fn match_arm_bodies(masked: &[u8], s: usize, e: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = s;
    while i < e && masked[i] != b'{' {
        i += 1;
    }
    let Some(body_close) = match_bracket(masked, i).filter(|&c| c < e) else {
        return out;
    };
    let mut a = i + 1;
    while a < body_close {
        // Find `=>` at depth 0 from `a`.
        let mut depth = 0i32;
        let mut arrow = None;
        let mut j = a;
        while j < body_close {
            match masked[j] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b'=' if depth == 0 && masked.get(j + 1) == Some(&b'>') => {
                    arrow = Some(j + 2);
                    break;
                }
                b',' if depth == 0 => break,
                _ => {}
            }
            j += 1;
        }
        let Some(rhs_start) = arrow else {
            break;
        };
        // Arm body runs to the next top-level `,` (or the block close).
        let mut depth = 0i32;
        let mut rhs_end = body_close;
        let mut j = rhs_start;
        while j < body_close {
            match masked[j] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b',' if depth == 0 => {
                    rhs_end = j;
                    break;
                }
                _ => {}
            }
            j += 1;
        }
        out.push((rhs_start, rhs_end));
        a = rhs_end + 1;
    }
    out
}

/// Record a code position. Literals reachable through transparent
/// structure enter the vocabulary; the residual expression is reported
/// as a dynamic position whose produced value the sink assertions check
/// at runtime.
pub(super) fn take_position(sites: &mut Sites, start: usize, end: usize) {
    let expr = sites.original[start..end].trim();
    if expr.is_empty() {
        return;
    }
    match single_literal(expr) {
        Some(Some(code)) => sites.codes.push(CodeSite {
            file: sites.file.clone(),
            line: line_of(sites.original, start),
            code,
            scan: sites.scan,
        }),
        _ => {
            for (ls, le) in literal_leaves(sites.masked, start, end) {
                let content = &sites.original[ls + 1..le - 1];
                if !content.contains('{') && !content.contains('}') {
                    sites.codes.push(CodeSite {
                        file: sites.file.clone(),
                        line: line_of(sites.original, ls),
                        code: content.to_string(),
                        scan: sites.scan,
                    });
                }
            }
            sites.dynamic.push(format!(
                "{}:{}: {expr}",
                sites.file.display(),
                line_of(sites.original, start)
            ));
        }
    }
}

/// The `…(args)` call whose `(` is at `open`: take every argument that
/// could carry a string (contains a literal or is plausibly string
/// shaped) as a code position. Used by the `.code`-method and bare
/// `code.`-method scans.
pub(super) fn take_string_args(sites: &mut Sites, masked: &[u8], open: usize, close: usize) {
    for (s, e) in super::source::call_args(masked, open) {
        let shape = skeleton_arg(sites.original, s, e);
        if masked[s..e.min(close)].contains(&b'"')
            || plausibly_string(&shape) && !shape.starts_with('\'')
        {
            take_position(sites, s, e);
        }
    }
}

fn skeleton_arg(original: &str, s: usize, e: usize) -> String {
    super::source::skeleton(&original[s..e])
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
