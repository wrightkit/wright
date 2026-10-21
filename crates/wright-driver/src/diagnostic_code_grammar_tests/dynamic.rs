//! The dynamic-position report: every source position where a value
//! can reach a diagnostic `code` without being a single source
//! literal — format-produced codes, variables, function returns,
//! `if`/`match` produced values, field writes.
//!
//! The scans mirror the position evidence the literal census uses, but
//! their job is the report: each position's expression is evaluated,
//! and a span that reduces to exactly one literal joins the vocabulary
//! while anything else is listed as `scan`-provenanced dynamic — values
//! the runtime chokes (`Diagnostic::error`'s and `refusal`'s
//! `debug_assert!` code-token assertions, plus the engine-kind tests)
//! check when they execute.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::brackets::in_pattern;
use super::positions::{
    CodeSite, arg_names_code_channel, code_binding_name, is_code_field, mutates_string,
    plausibly_string, single_literal,
};
use super::sinks::{AMBIGUOUS_SINKS, CodeSinks};
use super::source::{
    as_str, call_args, enclosing_call, line_of, match_bracket, rhs_span, skeleton, stage_marker_at,
};

/// One dynamic code position — a non-literal value that can reach a
/// diagnostic `code`.
pub(super) struct DynSite {
    pub file: PathBuf,
    pub line: usize,
    pub ctx: String,
    pub scan: &'static str,
}

/// Evaluate the position `vs..ve`: a span that reduces to exactly one
/// literal joins the vocabulary; anything else is a dynamic entry.
fn note_position(
    vocabulary: &mut Vec<CodeSite>,
    dynamic: &mut Vec<DynSite>,
    original: &str,
    file: &Path,
    scan: &'static str,
    vs: usize,
    ve: usize,
) {
    let shape = skeleton(&original[vs..ve]);
    if shape.is_empty() || looks_like_type(original[vs..ve].trim()) {
        return;
    }
    if let Some(Some(code)) = single_literal(&original[vs..ve]) {
        vocabulary.push(CodeSite {
            file: file.to_path_buf(),
            line: line_of(original, vs),
            code,
            scan,
        });
        return;
    }
    let mut ctx = shape;
    if ctx.len() > 96 {
        ctx.truncate(96);
        ctx.push('…');
    }
    dynamic.push(DynSite {
        file: file.to_path_buf(),
        line: line_of(original, vs),
        ctx,
        scan,
    });
}

/// `write!`/`writeln!` args past the writer.
fn note_write_args(
    vocabulary: &mut Vec<CodeSite>,
    dynamic: &mut Vec<DynSite>,
    original: &str,
    masked: &[u8],
    file: &Path,
    open: usize,
) {
    for arg in call_args(masked, open).iter().skip(1) {
        note_position(vocabulary, dynamic, original, file, "write", arg.0, arg.1);
    }
}

/// Every string arg of the call at `open` — for mutating methods on a
/// code channel.
fn note_string_args(
    vocabulary: &mut Vec<CodeSite>,
    dynamic: &mut Vec<DynSite>,
    original: &str,
    masked: &[u8],
    file: &Path,
    open: usize,
    close: usize,
) {
    for (s, e) in call_args(masked, open) {
        if s < close {
            note_position(vocabulary, dynamic, original, file, "mutator", s, e);
        }
    }
}

/// Enumerate the dynamic code positions of every scanned file — the
/// same evidence the literal census uses, evaluated into
/// literal-vs-dynamic entries.
pub(super) fn enumerate_dynamic(
    files: &BTreeMap<PathBuf, (String, Vec<u8>)>,
    sinks: &CodeSinks,
    vocabulary: &mut Vec<CodeSite>,
    dynamic: &mut Vec<DynSite>,
) {
    for (path, (original, masked)) in files {
        let text = as_str(masked, 0..masked.len()).to_string();
        let stage_glob = text.contains("Stage::*");
        let analyzer = path.to_string_lossy().contains("wright-analyzer");

        // Sink callsites: every arg at a code-parameter index.
        for (name, indices) in sinks {
            let mut needles: Vec<(usize, usize)> = text
                .match_indices(format!("{name}(").as_str())
                .map(|(a, _)| (a, a + name.len()))
                .collect();
            for (a, _) in text.match_indices(format!("{name}::<").as_str()) {
                let mut j = a + name.len() + 2;
                let mut depth = 1i32;
                while j < masked.len() && depth > 0 {
                    match masked[j] {
                        b'<' => depth += 1,
                        b'>' => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                if masked.get(j) == Some(&b'(') {
                    needles.push((a, j));
                }
            }
            for (at, open) in needles {
                if at > 0 && (masked[at - 1].is_ascii_alphanumeric() || masked[at - 1] == b'_') {
                    continue;
                }
                if text[..at].trim_end().ends_with("fn") {
                    continue; // the `fn name(` definition itself
                }
                for index in indices {
                    let Some(span) = call_args(masked, open).get(*index).copied() else {
                        continue;
                    };
                    if !plausibly_string(&skeleton(&original[span.0..span.1])) {
                        continue;
                    }
                    if AMBIGUOUS_SINKS.contains(&name.as_str())
                        && !arg_names_code_channel(
                            skeleton(&original[span.0..span.1]).trim_start_matches(['&', '*']),
                        )
                    {
                        continue;
                    }
                    note_position(vocabulary, dynamic, original, path, "sink", span.0, span.1);
                }
            }
        }

        // `code:`/`.._code:`/`id:`(analyzer) field values.
        for (at, _) in text.match_indices(':') {
            if masked.get(at + 1) == Some(&b':') || (at > 0 && masked[at - 1] == b':') {
                continue;
            }
            let mut nstart = at;
            while nstart > 0
                && (masked[nstart - 1].is_ascii_alphanumeric() || masked[nstart - 1] == b'_')
            {
                nstart -= 1;
            }
            let name = &text[nstart..at];
            if !is_code_field(name, analyzer) {
                continue;
            }
            let mut depth = 0i32;
            let mut vend = masked.len();
            for (i, &b) in masked.iter().enumerate().skip(at + 1) {
                match b {
                    b'(' | b'[' | b'{' => depth += 1,
                    b'<' if i > 0
                        && (masked[i - 1].is_ascii_alphanumeric()
                            || masked[i - 1] == b'>'
                            || masked[i - 1] == b'_'
                            || masked[i - 1] == b':') =>
                    {
                        depth += 1;
                    }
                    b'>' if depth > 0 => depth -= 1,
                    b')' | b']' | b'}' => {
                        if depth == 0 {
                            vend = i;
                            break;
                        }
                        depth -= 1;
                    }
                    b',' if depth == 0 => {
                        vend = i;
                        break;
                    }
                    _ => {}
                }
            }
            let vstart = at + 1 + (text[at + 1..].len() - text[at + 1..].trim_start().len());
            while vend > vstart && (text.as_bytes()[vend - 1] as char).is_ascii_whitespace() {
                vend -= 1;
            }
            let value = text[vstart..vend].trim();
            if value.is_empty() || looks_like_type(value) || in_pattern(masked, at) {
                continue;
            }
            note_position(vocabulary, dynamic, original, path, "field", vstart, vend);
        }

        // `.code`-family writes: `=`, `+=`, mutating methods, and
        // `mem::`/`ptr::`/`write!` calls taking the field as target.
        for (at, _) in text.match_indices('.') {
            let fname_end = at
                + 1
                + text[at + 1..]
                    .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .unwrap_or(0);
            let fname = &text[at + 1..fname_end];
            if !is_code_field(fname, analyzer) {
                continue;
            }
            let mut i = at + 1 + fname.len();
            while i < text.len() && masked[i].is_ascii_whitespace() {
                i += 1;
            }
            match masked.get(i) {
                Some(b'=')
                    if masked.get(i + 1) != Some(&b'=') && masked.get(i + 1) != Some(&b'>') =>
                {
                    let (vstart, vend) = rhs_span(masked, i, masked.len());
                    note_position(
                        vocabulary,
                        dynamic,
                        original,
                        path,
                        "fieldwrite",
                        vstart,
                        vend,
                    );
                }
                Some(b'+') if masked.get(i + 1) == Some(&b'=') => {
                    let (vstart, vend) = rhs_span(masked, i + 1, masked.len());
                    note_position(
                        vocabulary,
                        dynamic,
                        original,
                        path,
                        "fieldwrite",
                        vstart,
                        vend,
                    );
                }
                Some(b'.') => {
                    let mut j = i + 1;
                    while j < masked.len()
                        && (masked[j].is_ascii_alphanumeric() || masked[j] == b'_')
                    {
                        j += 1;
                    }
                    let method = &text[i + 1..j];
                    if !mutates_string(method) {
                        continue;
                    }
                    if masked.get(j) == Some(&b'(')
                        && let Some(close) = match_bracket(masked, j)
                    {
                        note_string_args(vocabulary, dynamic, original, masked, path, j, close);
                    }
                }
                Some(b',') | Some(b')') => {
                    let Some((callee, open)) = enclosing_call(masked, at) else {
                        continue;
                    };
                    let args = call_args(masked, open);
                    if callee == "write!" || callee == "writeln!" {
                        note_write_args(vocabulary, dynamic, original, masked, path, open);
                        continue;
                    }
                    let bare = callee.rsplit([':', '.']).next().unwrap_or("");
                    let mutation_like = [
                        "replace",
                        "write",
                        "write_unaligned",
                        "copy",
                        "copy_nonoverlapping",
                        "take",
                        "swap",
                        "read",
                        "write_bytes",
                        "drop_in_place",
                        "clone_from",
                    ]
                    .contains(&bare);
                    if mutation_like {
                        for arg in args {
                            if arg.0 <= at && at < arg.1 {
                                continue;
                            }
                            note_position(
                                vocabulary,
                                dynamic,
                                original,
                                path,
                                "fieldwrite-call",
                                arg.0,
                                arg.1,
                            );
                        }
                    }
                }
                _ => {}
            }
        }

        // Bare `code`/`id` writes: `code = …`, `code += …`,
        // `code.push_str(…)`, `write!(code, …)`.
        for name in ["code", "id"] {
            for (at, _) in text.match_indices(name) {
                if at > 0
                    && (masked[at - 1].is_ascii_alphanumeric()
                        || masked[at - 1] == b'_'
                        || masked[at - 1] == b'.'
                        || masked[at - 1] == b':')
                {
                    continue;
                }
                let mut i = at + name.len();
                while i < masked.len() && masked[i].is_ascii_whitespace() {
                    i += 1;
                }
                match masked.get(i) {
                    Some(b'=')
                        if masked.get(i + 1) != Some(&b'=') && masked.get(i + 1) != Some(&b'>') =>
                    {
                        let (vstart, vend) = rhs_span(masked, i, masked.len());
                        note_position(
                            vocabulary,
                            dynamic,
                            original,
                            path,
                            "barewrite",
                            vstart,
                            vend,
                        );
                    }
                    Some(b'+') if masked.get(i + 1) == Some(&b'=') => {
                        let (vstart, vend) = rhs_span(masked, i + 1, masked.len());
                        note_position(
                            vocabulary,
                            dynamic,
                            original,
                            path,
                            "barewrite",
                            vstart,
                            vend,
                        );
                    }
                    Some(b'.') => {
                        let mut j = i + 1;
                        while j < masked.len()
                            && (masked[j].is_ascii_alphanumeric() || masked[j] == b'_')
                        {
                            j += 1;
                        }
                        let method = &text[i + 1..j];
                        if !mutates_string(method) {
                            continue;
                        }
                        if masked.get(j) == Some(&b'(')
                            && let Some(close) = match_bracket(masked, j)
                        {
                            note_string_args(vocabulary, dynamic, original, masked, path, j, close);
                        }
                    }
                    Some(b',') | Some(b')') => {
                        let Some((callee, open)) = enclosing_call(masked, at) else {
                            continue;
                        };
                        if callee == "write!" || callee == "writeln!" {
                            note_write_args(vocabulary, dynamic, original, masked, path, open);
                        }
                    }
                    _ => {}
                }
            }
        }

        // `(expr, Stage::…)` tuples: the expression before `, Stage::`.
        for (at, _) in text.match_indices(',') {
            if !stage_marker_at(masked, at, stage_glob) {
                continue;
            }
            let mut end = at;
            while end > 0 && (text.as_bytes()[end - 1] as char).is_ascii_whitespace() {
                end -= 1;
            }
            if end == 0 {
                continue;
            }
            let mut start = end;
            loop {
                let prev = start.checked_sub(1).map(|i| masked[i]);
                match prev {
                    Some(b')') | Some(b']') | Some(b'}') => {
                        let mut depth = 0i32;
                        let mut j = start;
                        while j > 0 {
                            j -= 1;
                            match masked[j] {
                                b')' | b']' | b'}' => depth += 1,
                                b'(' | b'[' | b'{' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        start = j;
                    }
                    Some(b'"') => {
                        let mut j = start - 1;
                        while j > 0 && masked[j - 1] != b'"' {
                            j -= 1;
                        }
                        if j == 0 {
                            break;
                        }
                        start = j - 1;
                    }
                    Some(c)
                        if c.is_ascii_alphanumeric()
                            || c == b'_'
                            || c == b'!'
                            || c == b':'
                            || c == b'.' =>
                    {
                        let mut j = start;
                        while j > 0
                            && (masked[j - 1].is_ascii_alphanumeric()
                                || masked[j - 1] == b'_'
                                || masked[j - 1] == b'!'
                                || masked[j - 1] == b':'
                                || masked[j - 1] == b'.')
                        {
                            j -= 1;
                        }
                        start = j;
                    }
                    _ => break,
                }
                if start == 0 {
                    break;
                }
            }
            let expr = original[start..end].trim();
            if expr.starts_with("Stage::")
                || expr.contains("::Stage::")
                || (stage_glob
                    && !expr.is_empty()
                    && expr.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && expr.chars().next().is_some_and(char::is_uppercase))
            {
                continue;
            }
            note_position(
                vocabulary,
                dynamic,
                original,
                path,
                "stagetuple",
                start,
                end,
            );
        }

        // `=> "lit"` arms, `return` values, and tail expressions inside
        // `fn <name>code(` bodies — the same name convention
        // `slot::return_tail` applies: a `code` `_`-segment anywhere in
        // the name (`code`, `provider_failure_code`, `code_for_kind`)
        // makes the produced value a code, with `by_code` excluded as
        // the entity channel. Other string-returning fns are not code
        // positions — the literal census defers their returns under the
        // fn name and lets the callsites decide.
        for (at, _) in text.match_indices("fn ") {
            let name_start = at + 3;
            let Some(name_len) =
                text[name_start..].find(|c: char| !c.is_alphanumeric() && c != '_')
            else {
                continue;
            };
            let name = &text[name_start..name_start + name_len];
            if name.contains("by_code") || !name.split('_').any(|s| s.eq_ignore_ascii_case("code"))
            {
                continue;
            }
            let Some(body) = text[name_start + name_len..]
                .find('{')
                .map(|n| name_start + name_len + n)
            else {
                continue;
            };
            let header = &text[name_start + name_len..body];
            let returns = header.rsplit("->").next().unwrap_or("");
            if !(returns.contains("str") || returns.contains("String")) {
                continue;
            }
            let Some(body_end) = match_bracket(masked, body) else {
                continue;
            };
            for (arm, _) in text[body..body_end].match_indices("=>") {
                let arm_at = body + arm;
                let rest = &text[arm_at + 2..body_end];
                let rest = rest.trim_start();
                if rest.starts_with('{') {
                    dynamic.push(DynSite {
                        file: path.clone(),
                        line: line_of(original, arm_at),
                        ctx: "=> { … } arm in a code-producing fn".to_string(),
                        scan: "codefn",
                    });
                    continue;
                }
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
                if original[expr_start..expr_start + expr_end]
                    .trim()
                    .is_empty()
                {
                    continue;
                }
                note_position(
                    vocabulary,
                    dynamic,
                    original,
                    path,
                    "codefn-arm",
                    expr_start,
                    expr_start + expr_end,
                );
            }
            for (r, _) in text[body..body_end].match_indices("return") {
                let ra = body + r;
                if ra > 0 && (masked[ra - 1].is_ascii_alphanumeric() || masked[ra - 1] == b'_') {
                    continue;
                }
                let mut vs = ra + "return".len();
                while vs < body_end && masked[vs].is_ascii_whitespace() {
                    vs += 1;
                }
                if vs >= body_end || masked[vs] == b';' {
                    continue;
                }
                let (rstart, rend) = rhs_span(masked, vs - 1, body_end);
                let shape = skeleton(&original[rstart..rend]);
                if ["match", "{", "for", "while", "loop", "unsafe"]
                    .iter()
                    .any(|k| shape.starts_with(k))
                {
                    continue;
                }
                note_position(
                    vocabulary,
                    dynamic,
                    original,
                    path,
                    "codefn-return",
                    rstart,
                    rend,
                );
            }
            // The body's tail expression.
            {
                let mut depth = 0i32;
                let mut tail_start = body + 1;
                for (i, &b) in masked.iter().enumerate().take(body_end).skip(body + 1) {
                    match b {
                        b'(' | b'[' | b'{' => depth += 1,
                        b')' | b']' | b'}' => depth -= 1,
                        b';' if depth == 0 => tail_start = i + 1,
                        _ => {}
                    }
                }
                loop {
                    let mut ts = tail_start;
                    while ts < body_end && masked[ts].is_ascii_whitespace() {
                        ts += 1;
                    }
                    let mut te = body_end;
                    while te > ts && masked[te - 1].is_ascii_whitespace() {
                        te -= 1;
                    }
                    if ts >= te {
                        break;
                    }
                    if masked[ts] == b'{'
                        && let Some(inner_end) = match_bracket(masked, ts)
                    {
                        depth = 0;
                        tail_start = ts + 1;
                        for (i, &b) in masked.iter().enumerate().take(inner_end).skip(ts + 1) {
                            match b {
                                b'(' | b'[' | b'{' => depth += 1,
                                b')' | b']' | b'}' => depth -= 1,
                                b';' if depth == 0 => tail_start = i + 1,
                                _ => {}
                            }
                        }
                        continue;
                    }
                    let shape = skeleton(&original[ts..te]);
                    if !shape.is_empty()
                        && !["match", "for", "while", "loop", "unsafe"]
                            .iter()
                            .any(|k| shape.starts_with(k))
                    {
                        note_position(vocabulary, dynamic, original, path, "codefn-tail", ts, te);
                    }
                    break;
                }
            }
        }

        // `for <code-ish> in <iterable>`: the iterable's positions.
        for (at, _) in text.match_indices("for ") {
            let head = &text[at + 4..];
            let Some(in_at) = head.find(" in ") else {
                continue;
            };
            let pat = head[..in_at]
                .trim_start_matches(['m', 'u', 't', ' ', 'r', 'e', 'f', '&'])
                .trim_matches(['(', ')'])
                .trim();
            // The pattern's words must bind a code channel the same way
            // the literal census's `code_binding_name` decides — a
            // `code_execution` loop variable is not a code channel.
            let code_named = pat
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .any(|w| code_binding_name(w, analyzer));
            if !code_named {
                continue;
            }
            let iter_start = at + 4 + in_at + 4;
            let mut depth = 0i32;
            let mut iter_end = masked.len();
            for (i, &b) in masked.iter().enumerate().skip(iter_start) {
                match b {
                    b'(' | b'[' => depth += 1,
                    b')' | b']' => depth -= 1,
                    b'{' if depth == 0 => {
                        iter_end = i;
                        break;
                    }
                    _ => {}
                }
            }
            let mut it_start = iter_start;
            while it_start < iter_end && masked[it_start].is_ascii_whitespace() {
                it_start += 1;
            }
            while iter_end > it_start && masked[iter_end - 1].is_ascii_whitespace() {
                iter_end -= 1;
            }
            if it_start < iter_end {
                note_position(
                    vocabulary, dynamic, original, path, "foriter", it_start, iter_end,
                );
            }
        }
    }
}

/// Whether a `code:`/`id:` "value" is actually a type annotation.
fn looks_like_type(value: &str) -> bool {
    let first = value.split([' ', '<', '(']).next().unwrap_or("");
    let second = first.strip_prefix('&').unwrap_or(first);
    !first.is_empty()
        && (second.chars().next().is_some_and(char::is_uppercase)
            || second.starts_with('\'')
            || second == "str"
            || first == "impl"
            || [
                "u8", "u16", "u32", "u64", "usize", "i8", "i16", "i32", "i64", "bool",
            ]
            .contains(&first))
        && !value.contains('(')
        && !value.contains('=')
}
