//! The driver fixed-code census: every literal diagnostic code in a
//! production code position of this crate's own source, plus every code
//! position whose value cannot be classified. Scans the shapes that
//! produce codes: `Diagnostic::error`/`warning`/`info` first arguments,
//! `code:` field values, `(code, Stage::…)` tuple arms, `=>` arms inside
//! string-returning `fn <name>code(` bodies, and `refusal(` first
//! arguments (the `lookup` tool-error code).

use std::path::{Path, PathBuf};

use super::source::{
    as_str, blank_cfg_test_items, call_args, crate_src, line_of, mask_rust, match_bracket,
    read_rs_tree,
};

/// `expr` with all whitespace and string-literal contents removed — the
/// shape of a code-position expression, for the dynamic-producer rules.
fn skeleton(expr: &str) -> String {
    let mut out = String::with_capacity(expr.len());
    let mut chars = expr.chars();
    while let Some(c) = chars.next() {
        if c == '"' {
            out.push('"');
            out.push('"');
            let mut escaped = false;
            for inner in chars.by_ref() {
                if escaped {
                    escaped = false;
                } else if inner == '\\' {
                    escaped = true;
                } else if inner == '"' {
                    break;
                }
            }
        } else if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

/// Whether a code-position expression is one of the enumerated dynamic
/// producers. Every rule is backed by a runtime test in this file that
/// drives that producer exhaustively, so an expression not listed here
/// fails the census instead of going unchecked.
fn dynamic_code_ok(expr: &str) -> bool {
    let normalized: String = expr.chars().filter(|c| !c.is_whitespace()).collect();
    // Wright-built shapes whose variable parts are enumerated by runtime
    // tests in this file.
    if normalized.ends_with("provider_failure_code(&error)") // provider/refusal census
        || normalized.starts_with("diagnostic_code(") // workshop.<kind>.<name> census
        || normalized.starts_with("format!(\"unknown-") // unknown-<kind> census
        || normalized.starts_with("format!(\"workshop.")
        || normalized.starts_with("format!(\"{PROVIDER_REFUSAL_CODE_PREFIX}")
        || normalized.starts_with("finding[\"code\"]")
        || normalized == "UNKNOWN_WITHIN"
    {
        return true;
    }
    // Passthroughs of already-enumerated or owner/provider-spelled codes:
    // `.code` reads, `code` parameters, and `.code()` methods.
    let shape = skeleton(expr);
    shape == "code"
        || shape == "code.to_string()"
        || shape == "code.into()"
        || shape.ends_with(".code")
        || shape.ends_with(".code()")
        || shape.ends_with(".code.to_string()")
        || shape.ends_with(".code().to_string()")
        || shape.ends_with(".code.unwrap_or_else(||\"\".to_string())")
        || shape == "error.refusal_code().unwrap_or_else(||error.code())"
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
                if !push_literal(&mut codes, original, path, span) {
                    check_dynamic(
                        &mut unclassified,
                        original,
                        path,
                        &original[span.0..span.1],
                        span.0,
                    );
                }
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
            // `|| "lit"` fallbacks inside a passthrough (e.g.
            // `d.code.unwrap_or_else(|| "provider-diagnostic".to_string())`)
            // still enumerate a literal.
            for (off, _) in text[vstart..vend].match_indices("||") {
                let lit_at = vstart + off + 2;
                let after = text[lit_at..].trim_start();
                let Some(rest) = after.strip_prefix('"') else {
                    continue;
                };
                if let Some(end) = rest.find('"') {
                    // `rest` is masked text — only its byte offsets are
                    // meaningful; the literal itself is in `original`.
                    let open = lit_at + (after.len() - rest.len()) + 1;
                    codes.push(CodeSite {
                        file: path.clone(),
                        line: line_of(original, lit_at),
                        code: original[open..open + end].to_string(),
                    });
                }
            }
            if !push_literal(&mut codes, original, path, (vstart, vend)) {
                check_dynamic(
                    &mut unclassified,
                    original,
                    path,
                    &original[vstart..vend],
                    vstart,
                );
            }
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
            if !push_literal(&mut codes, original, path, (start, end)) {
                check_dynamic(&mut unclassified, original, path, expr, start);
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
                    unclassified.push(format!(
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
                if !push_literal(
                    &mut codes,
                    original,
                    path,
                    (expr_start, expr_start + expr_end),
                ) {
                    check_dynamic(&mut unclassified, original, path, expr, expr_start);
                }
            }
        }

        // `refusal(` first arguments (lookup tool-error codes).
        for (at, _) in text.match_indices("refusal(") {
            let before = text[..at].trim_end();
            if before.ends_with("fn")
                || before.ends_with("is_")
                || before.ends_with('_')
                || before
                    .chars()
                    .last()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == ':')
            {
                continue; // is_refusal( / refusal_code( / fn refusal
            }
            let open = at + "refusal(".len() - 1;
            let Some(span) = call_args(&masked, open).first().copied() else {
                continue;
            };
            let arg = original[span.0..span.1].trim();
            if arg.starts_with("vec!") || arg.starts_with('[') {
                continue; // a diagnostics vector, not a code argument
            }
            if !push_literal(&mut codes, original, path, span) {
                check_dynamic(&mut unclassified, original, path, arg, span.0);
            }
        }
    }
    (codes, unclassified)
}
