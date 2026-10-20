//! The driver fixed-code census: every literal diagnostic code in a
//! production code position of this crate's own source, plus every code
//! position whose value cannot be classified. Scans the shapes that
//! produce codes: `Diagnostic::error`/`warning`/`info` first arguments,
//! `code:` field values, `(code, Stage::…)` tuple arms, `=>` arms inside
//! string-returning `fn <name>code(` bodies, `refusal(` first arguments
//! (the `lookup` tool-error code), `.code =`/`code =` assignments, and
//! `code` shorthand fields inside `Diagnostic { … }` literals (resolved
//! through the binding's `let`/`fn` origin), and bare `code` ctor/`code:`
//! arguments resolved the same way. `wright-cli`, `wright-analyzer`, and
//! `wright-language` are scanned too: their Diagnostics, analyzer
//! ErrorInfo/finding codes, and SourceDiagnostics all reach the same
//! envelope, so their code positions are part of this contract surface.

use std::path::{Path, PathBuf};

use super::source::{
    as_str, blank_cfg_test_items, call_args, crate_src, enclosing_fn, enclosing_fn_body, line_of,
    mask_rust, match_arms_stage_ok, match_bracket, read_rs_tree, rhs_span, skeleton,
};

/// Wright-built producers whose variable parts are enumerated by runtime
/// tests in this file.
fn known_producer(normalized: &str) -> bool {
    normalized.contains("provider_failure_code(") // provider/refusal census
        || normalized.starts_with("diagnostic_code(") // workshop.<kind>.<name> census
        || normalized.starts_with("format!(\"unknown-") // unknown-<kind> census
        || normalized.starts_with("format!(\"workshop.")
        || normalized.starts_with("format!(\"{PROVIDER_REFUSAL_CODE_PREFIX}")
        || normalized.starts_with("finding[\"code\"]")
        || normalized == "UNKNOWN_WITHIN"
}

/// Whether a code-position expression is one of the enumerated dynamic
/// producers. Every rule is backed by a runtime test in this file that
/// drives that producer exhaustively, so an expression not listed here
/// fails the census instead of going unchecked.
fn dynamic_code_ok(expr: &str) -> bool {
    let mut normalized: String = expr.chars().filter(|c| !c.is_whitespace()).collect();
    // Peel pass-through wrappers — `Some(…)`, `.to_string()`/`.into()`/
    // `.clone()`/`.as_deref()`/`.as_str()`/`.unwrap_or_default()` suffixes,
    // and `unwrap_or*`/`or*` fallback tails — re-checking the producing
    // shape after every peel so `Some(format!("unknown-…"))` still hits
    // the prefix rules.
    loop {
        if known_producer(&normalized) {
            return true;
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
    // Passthroughs of already-enumerated or owner/provider-spelled codes:
    // `.code` reads, `.code()`/`.refusal_code()`/`.provider_code`
    // passthrough fields and methods, analyzer lint-registry `.id`/`id()`
    // rule codes (the analyzer owns that vocabulary verbatim). Bare `code`
    // reaches here only from the match-arm scans, whose patterns bind it;
    // every other code position resolves the binding via classify_rhs.
    let shape = skeleton(&normalized);
    shape == "code"
        || shape.ends_with(".code")
        || shape.ends_with(".code()")
        || shape.ends_with(".refusal_code()")
        || shape.ends_with(".provider_code")
        || shape.ends_with(".provider_code()")
        || shape.ends_with(".id")
        || shape.ends_with(".id()")
}

/// One literal found in a code position, with its site for diagnostics.
pub(super) struct CodeSite {
    pub(super) file: PathBuf,
    pub(super) line: usize,
    pub(super) code: String,
}

/// Extract the leading string literal of a code-position expression
/// (`"lit"`, `Some("lit")`, `"lit".to_string()`); returns whether one was
/// found and, when found, pushes it onto `codes`.
fn push_literal(
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

/// A non-literal code position must be one of the enumerated dynamic
/// producers; otherwise it joins the unclassified list and fails the census.
fn check_dynamic(
    unclassified: &mut Vec<String>,
    original: &str,
    file: &Path,
    expr: &str,
    pos: usize,
) {
    if !dynamic_code_ok(expr) {
        unclassified.push(format!(
            "{}:{}: {expr}",
            file.display(),
            line_of(original, pos),
            expr = expr.trim()
        ));
    }
}

/// Collect every literal diagnostic code from the crate's own source, plus
/// every code position whose value cannot be classified. Scans the
/// production shapes that produce codes: `Diagnostic::error/warning/info`
/// first arguments, `code:` field values, `(code, Stage::…)` tuple arms,
/// `=>` arms inside `fn code` bodies, and `refusal(` first arguments (the
/// `lookup` tool-error code).
pub(super) fn driver_code_census() -> (Vec<CodeSite>, Vec<String>) {
    let mut codes = Vec::new();
    let mut unclassified = Vec::new();
    let mut files = read_rs_tree(&crate_src());
    // `wright-cli`, `wright-analyzer`, and `wright-language` all feed the
    // envelope's code positions — cli Diagnostics push directly into
    // `diagnostics[]`, analyzer ErrorInfo/finding codes pass through
    // `error.code`/`finding["code"]`, and language SourceDiagnostics share
    // the service surface — so their code positions share this census.
    if let Some(crates) = crate_src().parent().and_then(Path::parent) {
        for member in ["wright-cli", "wright-analyzer", "wright-language"] {
            let dir = crates.join(member).join("src");
            if dir.is_dir() {
                files.extend(read_rs_tree(&dir));
            }
        }
    }
    // The census's own source — this module and its directory — is not a
    // code position.
    files.retain(|path, _| {
        let base = crate_src();
        !path.starts_with(base.join("diagnostic_code_grammar_tests"))
            && path != &base.join("diagnostic_code_grammar_tests.rs")
    });

    for (path, original) in &files {
        let masked = blank_cfg_test_items(&mask_rust(original));
        let text = as_str(&masked, 0..masked.len()).to_string();
        let mut sites = Sites {
            codes: &mut codes,
            unclassified: &mut unclassified,
            original,
            masked: &masked,
            file: path,
        };

        // `Diagnostic::error|warning|info(` first arguments.
        for ctor in [
            "Diagnostic::error(",
            "Diagnostic::warning(",
            "Diagnostic::info(",
        ] {
            for (at, _) in text.match_indices(ctor) {
                let open = at + ctor.len() - 1;
                let Some(span) = call_args(&masked, open).first().copied() else {
                    continue;
                };
                classify_rhs(&mut sites, span.0, span.1);
            }
        }

        // `code:` field values (word boundary, not `provider_code:` etc.).
        for (at, _) in text.match_indices("code:") {
            if at > 0
                && (text.as_bytes()[at - 1].is_ascii_alphanumeric()
                    || text.as_bytes()[at - 1] == b'_')
            {
                continue;
            }
            // The value runs to the next `,`, `}`, or — inside a
            // signature or call — the `)`/`]` closing the enclosing list.
            let mut depth = 0i32;
            let mut vend = masked.len();
            for (i, &b) in masked.iter().enumerate().skip(at + "code:".len()) {
                match b {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' if depth > 0 => depth -= 1,
                    b',' | b'}' | b')' | b']' => {
                        vend = i;
                        break;
                    }
                    _ => {}
                }
            }
            let vstart = at
                + "code:".len()
                + (text[at + "code:".len()..].len()
                    - text[at + "code:".len()..].trim_start().len());
            while vend > vstart && (text.as_bytes()[vend - 1] as char).is_ascii_whitespace() {
                vend -= 1;
            }
            let value = text[vstart..vend].trim();
            if value.is_empty() {
                continue;
            }
            // Type positions, not code values: `code: String` field
            // declarations and `code: &str`/`code: impl Into<String>`
            // parameter declarations.
            let type_like = {
                let first = value.split([' ', '<', '(']).next().unwrap_or("");
                let second = first.strip_prefix('&').unwrap_or(first);
                !first.is_empty()
                    && (second.chars().next().is_some_and(char::is_uppercase)
                        || second.starts_with("'")
                        || second == "str"
                        || first == "impl"
                        || [
                            "u8", "u16", "u32", "u64", "usize", "i8", "i16", "i32", "i64", "bool",
                        ]
                        .contains(&first))
                    && !value.contains('(')
                    && !value.contains('=')
            };
            if type_like {
                continue;
            }
            classify_rhs(&mut sites, vstart, vend);
        }

        // `(code, Stage::…)` tuple arms: the expression directly before
        // `, Stage::` is the code position.
        for (at, _) in text.match_indices(", Stage::") {
            let mut end = at;
            while end > 0 && (text.as_bytes()[end - 1] as char).is_ascii_whitespace() {
                end -= 1;
            }
            if end == 0 {
                continue;
            }
            let mut start = end;
            if masked[end - 1] == b')' {
                // Balanced backwards to the matching `(`.
                let mut depth = 0i32;
                while start > 0 {
                    start -= 1;
                    match masked[start] {
                        b')' => depth += 1,
                        b'(' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                // Include a `format!`/method/`.to_string()` prefix, and back
                // over any string literal the prefix attaches to.
                loop {
                    while start > 0
                        && (masked[start - 1].is_ascii_alphanumeric()
                            || masked[start - 1] == b'_'
                            || masked[start - 1] == b'!'
                            || masked[start - 1] == b':'
                            || masked[start - 1] == b'.')
                    {
                        start -= 1;
                    }
                    if start > 0 && masked[start - 1] == b'"' {
                        start -= 1;
                        while start > 0 && masked[start - 1] != b'"' {
                            start -= 1;
                        }
                        start = start.saturating_sub(1);
                    } else {
                        break;
                    }
                }
            } else if masked[end - 1] == b'"' {
                // Back over the closing quote, then to the opening quote.
                start = end - 1;
                while start > 0 && masked[start - 1] != b'"' {
                    start -= 1;
                }
                start = start.saturating_sub(1);
            } else {
                // A bare identifier or path expression (`code`, `info.code`,
                // `self.code()`-less members).
                while start > 0
                    && (masked[start - 1].is_ascii_alphanumeric()
                        || masked[start - 1] == b'_'
                        || masked[start - 1] == b':'
                        || masked[start - 1] == b'.')
                {
                    start -= 1;
                }
            }
            let expr = original[start..end].trim();
            if expr.starts_with("Stage::") {
                continue; // a `Stage::X, Stage::Y` list, not a code tuple
            }
            // A bare `code` here is bound by the match arm's own pattern
            // — a legitimate read, not a binding to resolve.
            if !push_literal(sites.codes, original, path, (start, end)) {
                check_dynamic(sites.unclassified, original, path, expr, start);
            }
        }

        // `=> "lit"` arms inside `fn <name>code(` bodies — `code`,
        // `provider_failure_code`, `diagnostic_code`, and friends.
        for (at, _) in text.match_indices("fn ") {
            let name_start = at + 3;
            let Some(name_len) =
                text[name_start..].find(|c: char| !c.is_alphanumeric() && c != '_')
            else {
                continue;
            };
            if !text[name_start..name_start + name_len].ends_with("code") {
                continue;
            }
            let Some(body) = text[name_start + name_len..]
                .find('{')
                .map(|n| name_start + name_len + n)
            else {
                continue;
            };
            // Only functions returning a string produce diagnostic codes;
            // `fn exit_code_from(…) -> i32`/`fn wright_refusal_code(…) ->
            // bool` map to exit codes and bools — different channels.
            let header = &text[name_start + name_len..body];
            let returns = header.rsplit("->").next().unwrap_or("");
            if !(returns.contains("str") || returns.contains("String")) {
                continue;
            }
            let Some(body_end) = match_bracket(&masked, body) else {
                continue;
            };
            for (arm, _) in text[body..body_end].match_indices("=>") {
                let arm_at = body + arm;
                let rest = &text[arm_at + 2..body_end];
                let rest = rest.trim_start();
                if rest.starts_with('{') {
                    sites.unclassified.push(format!(
                        "{}:{}: `=> {{ … }}` arm inside a code-producing fn",
                        path.display(),
                        line_of(original, arm_at)
                    ));
                    continue;
                }
                // The arm value runs to the top-level `,` or the match's
                // `}`; offsets are computed on the masked text, then the
                // expression is read from `original`.
                let expr_start = arm_at + 2 + (text[arm_at + 2..body_end].len() - rest.len());
                let mut depth = 0i32;
                let mut expr_end = rest.len();
                for (i, &b) in rest.as_bytes().iter().enumerate() {
                    match b {
                        b'(' | b'[' | b'{' => depth += 1,
                        b')' | b']' | b'}' if depth > 0 => depth -= 1,
                        b',' | b'}' if depth == 0 => {
                            expr_end = i;
                            break;
                        }
                        _ => {}
                    }
                }
                while expr_end > 0 && (rest.as_bytes()[expr_end - 1] as char).is_ascii_whitespace()
                {
                    expr_end -= 1;
                }
                let expr = original[expr_start..expr_start + expr_end].trim();
                if expr.is_empty() {
                    continue;
                }
                // A bare `code` arm value is bound by the arm's pattern —
                // passthrough, not a binding to resolve.
                if !push_literal(
                    sites.codes,
                    original,
                    path,
                    (expr_start, expr_start + expr_end),
                ) {
                    check_dynamic(sites.unclassified, original, path, expr, expr_start);
                }
            }
        }

        // `.code =` field assignments (not `.code ==` or `.code =>`): the
        // assigned value runs to the statement's `;`.
        for (at, _) in text.match_indices(".code") {
            let mut i = at + ".code".len();
            while i < text.len() && text.as_bytes()[i].is_ascii_whitespace() {
                i += 1;
            }
            if masked.get(i) != Some(&b'=')
                || masked.get(i + 1) == Some(&b'=')
                || masked.get(i + 1) == Some(&b'>')
            {
                continue;
            }
            let (vstart, vend) = rhs_span(&masked, i, masked.len());
            if vend <= vstart {
                continue;
            }
            classify_rhs(&mut sites, vstart, vend);
        }

        // `Diagnostic { …, code, … }` shorthand field inits: the field
        // binds whatever `code` names — resolve that binding in the
        // enclosing fn (a `code` parameter, a `let (mut )?code =` RHS, or
        // a tuple/pattern binding whose `, Stage::` arms are scanned).
        for (at, _) in text.match_indices("Diagnostic") {
            if at > 0
                && (text.as_bytes()[at - 1].is_ascii_alphanumeric()
                    || text.as_bytes()[at - 1] == b'_')
            {
                continue;
            }
            let mut brace = at + "Diagnostic".len();
            while brace < text.len() && text.as_bytes()[brace].is_ascii_whitespace() {
                brace += 1;
            }
            if masked.get(brace) != Some(&b'{') {
                continue;
            }
            // A top-level `=`/`=>` in the head puts `Diagnostic` in
            // expression position (`let d = Diagnostic {`); without one a
            // leading `let`/`if let`/`for` is a pattern and leading
            // `impl`/`struct`/… is a declaration — all skip. Match-arm
            // patterns `Diagnostic { … } =>` are excluded below.
            let mut stmt = at;
            while stmt > 0 && !b";{}".contains(&masked[stmt - 1]) {
                stmt -= 1;
            }
            let head = text[stmt..at].trim_start();
            let mut in_expr = false;
            let mut head_depth = 0i32;
            for &b in &masked[stmt..at] {
                match b {
                    b'(' | b'[' | b'{' => head_depth += 1,
                    b')' | b']' | b'}' if head_depth > 0 => head_depth -= 1,
                    b'=' if head_depth == 0 => {
                        in_expr = true;
                        break;
                    }
                    _ => {}
                }
            }
            let last_word = head
                .rsplit(|c: char| c.is_ascii_whitespace() || c == ':')
                .next();
            if !in_expr
                && (head.starts_with("let")
                    || head.ends_with(" let")
                    || head == "let"
                    || head.starts_with("for ")
                    || matches!(
                        last_word,
                        Some(
                            "impl"
                                | "struct"
                                | "enum"
                                | "union"
                                | "trait"
                                | "type"
                                | "fn"
                                | "mod"
                                | "use"
                        )
                    ))
            {
                continue;
            }
            let Some(body_end) = match_bracket(&masked, brace) else {
                continue;
            };
            // `Diagnostic { … } =>` is a match-arm pattern, not a literal.
            let mut after_body = body_end + 1;
            while after_body < text.len() && masked[after_body].is_ascii_whitespace() {
                after_body += 1;
            }
            if masked.get(after_body) == Some(&b'=') && masked.get(after_body + 1) == Some(&b'>') {
                continue;
            }
            // Top-level comma-separated fields: a bare `code` segment is a
            // shorthand binding (`code:` is scanned as a named field).
            let mut depth = 0i32;
            let mut field_start = brace + 1;
            for (i, &b) in masked.iter().enumerate().take(body_end).skip(brace + 1) {
                match b {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => depth -= 1,
                    b',' if depth == 0 => {
                        check_shorthand_field(&mut sites, field_start, i, at);
                        field_start = i + 1;
                    }
                    _ => {}
                }
            }
            check_shorthand_field(&mut sites, field_start, body_end, at);
        }

        // `refusal(` first arguments (lookup tool-error codes). Call
        // boundary: the char directly before `refusal` is not ident/`::`
        // — that keeps `return refusal(` — while `fn refusal(`, `is_*`
        // helpers, and `::refusal` qualifiers stay excluded.
        for (at, _) in text.match_indices("refusal(") {
            if (at > 0
                && (masked[at - 1].is_ascii_alphanumeric()
                    || masked[at - 1] == b'_'
                    || masked[at - 1] == b':'))
                || text[..at].ends_with("fn ")
            {
                continue;
            }
            let open = at + "refusal(".len() - 1;
            let Some(span) = call_args(&masked, open).first().copied() else {
                continue;
            };
            let arg = original[span.0..span.1].trim();
            if arg.starts_with("vec!") || arg.starts_with('[') {
                continue; // a diagnostics vector, not a code argument
            }
            classify_rhs(&mut sites, span.0, span.1);
        }
    }
    (codes, unclassified)
}

/// The census accumulators and the file under scan, shared by the
/// code-position classifiers.
struct Sites<'a> {
    codes: &'a mut Vec<CodeSite>,
    unclassified: &'a mut Vec<String>,
    original: &'a str,
    masked: &'a [u8],
    file: &'a Path,
}

/// One top-level field segment of a `Diagnostic { … }` literal. A segment
/// that trims to exactly `code` is a shorthand binding to resolve.
fn check_shorthand_field(sites: &mut Sites<'_>, start: usize, end: usize, at: usize) {
    if as_str(sites.masked, start..end).trim() == "code" {
        resolve_code_binding(sites, at);
    }
}

/// Literals nested inside `unwrap_or("…")`/`unwrap_or_else(|| "…"…)`
/// fallbacks of a code-position expression are themselves possible codes
/// — census them.
fn census_fallback_literals(sites: &mut Sites<'_>, start: usize, end: usize) {
    let expr = &sites.original[start..end];
    for marker in ["unwrap_or(", "||"] {
        for (off, _) in expr.match_indices(marker) {
            let lit_at = start + off + marker.len();
            let rest = expr[off + marker.len()..].trim_start();
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

/// Classify one code-position RHS: a literal enters the census, a bare
/// `code` value resolves through the enclosing fn's binding, and any
/// other dynamic expression must match a verified producer.
fn classify_rhs(sites: &mut Sites<'_>, start: usize, end: usize) {
    if start >= end {
        return;
    }
    if !push_literal(sites.codes, sites.original, sites.file, (start, end)) {
        census_fallback_literals(sites, start, end);
        let mut inner = skeleton(&sites.original[start..end]);
        while inner.starts_with("Some(") && inner.ends_with(')') {
            inner = inner["Some(".len()..inner.len() - 1].to_string();
        }
        if inner == "code" {
            resolve_code_binding(sites, start);
        } else {
            check_dynamic(
                sites.unclassified,
                sites.original,
                sites.file,
                &sites.original[start..end],
                start,
            );
        }
    }
}

/// Resolve what a bare `code` value (shorthand field or ctor/`code:`
/// argument) binds inside the enclosing fn. A `code` parameter or a
/// pattern destructuring a Diagnostic field passes through a value whose
/// producer is already covered; a `let (mut )?code =`/`code =` RHS is
/// classified like any other code position; a tuple/pattern
/// `let (code, …) = match …` binding is covered only when every arm feeds
/// the `, Stage::` scan. Anything else is unclassified.
fn resolve_code_binding(sites: &mut Sites<'_>, at: usize) {
    let unclassified_at = |sites: &mut Sites<'_>, why: &str| {
        sites.unclassified.push(format!(
            "{}:{}: `code` value {why}",
            sites.file.display(),
            line_of(sites.original, at)
        ));
    };
    let Some((_, params)) = enclosing_fn(sites.masked, at) else {
        unclassified_at(sites, "outside a fn");
        return;
    };
    if params.iter().any(|(param, _)| param == "code") {
        return;
    }
    let Some((body, body_end)) = enclosing_fn_body(sites.masked, at) else {
        unclassified_at(sites, "without an enclosing fn body");
        return;
    };
    let text = as_str(sites.masked, 0..sites.masked.len());
    let mut bound = false;
    for (let_at, _) in text[body..body_end].match_indices("let ") {
        let stmt = body + let_at + "let ".len();
        // The `let` pattern runs to a top-level `=` or `;`.
        let mut depth = 0i32;
        let mut pend = stmt;
        let mut rhs = None;
        for j in stmt..body_end {
            match sites.masked[j] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' if depth > 0 => depth -= 1,
                b'=' if depth == 0 && sites.masked.get(j + 1) != Some(&b'=') => {
                    pend = j;
                    rhs = Some(j);
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
        let Some(eq) = rhs else {
            continue;
        };
        // Find where the pattern binds `code` (if anywhere).
        let Some(code_at) = pattern.match_indices("code").map(|(i, _)| i).find(|&i| {
            let before = pattern[..i].chars().last();
            let after = pattern[i + "code".len()..].chars().next();
            !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                && !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        }) else {
            continue;
        };
        bound = true;
        // `code` inside `{…}`/`(…)` in the pattern destructures or reads an
        // already-produced value — except a `let` whose pattern is only
        // `mut code`/`code`/`code: Type` (a fresh binding to classify).
        let shallow = !pattern[..code_at].trim_end().contains(['{', '(']);
        let (rstart, rend) = rhs_span(sites.masked, eq, body_end);
        if shallow {
            // A `code`-to-`code` binding traces to an earlier binding this
            // scan already covers — classify only real expressions.
            if skeleton(&sites.original[rstart..rend]) != "code" {
                classify_rhs(sites, rstart, rend);
            }
        } else {
            // Destructuring binds an already-produced code — except when
            // the RHS is a `match` producing `(code, …)` tuples: those are
            // covered by the `, Stage::` arm scan only when every arm
            // carries a Stage marker.
            let rhs_text = &text[eq + 1..rend];
            if rhs_text.contains("match") && !match_arms_stage_ok(rhs_text) {
                unclassified_at(sites, "bound by a match arm the Stage:: scan misses");
            }
        }
    }
    // A bare `code =` rebinds an uninitialised `let code;` (or a binding an
    // outer block owns): classify the RHS like any other code position.
    if !bound {
        for (code_at, _) in text[body..body_end].match_indices("code") {
            let a = body + code_at;
            if a > 0
                && (sites.masked[a - 1].is_ascii_alphanumeric()
                    || sites.masked[a - 1] == b'_'
                    || sites.masked[a - 1] == b'.')
            {
                continue;
            }
            let mut i = a + "code".len();
            while i < body_end && sites.masked[i].is_ascii_whitespace() {
                i += 1;
            }
            if sites.masked.get(i) == Some(&b'=')
                && sites.masked.get(i + 1) != Some(&b'=')
                && sites.masked.get(i + 1) != Some(&b'>')
            {
                bound = true;
                let (rstart, rend) = rhs_span(sites.masked, i, body_end);
                classify_rhs(sites, rstart, rend);
            }
        }
    }
    if !bound {
        unclassified_at(sites, "has no `code` binding in its fn");
    }
}
