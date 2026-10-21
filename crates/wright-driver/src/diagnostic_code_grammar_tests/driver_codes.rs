//! The driver fixed-code census: every literal diagnostic code in a
//! production code position of the crates that feed the envelope, plus
//! every code position whose value cannot be classified.
//!
//! A code position is anywhere a value can reach a diagnostic `code`:
//!
//! - first arguments of `Diagnostic::error`/`warning`/`info`;
//! - `code:`/`*_code:` field values and `, code`/`*_code` shorthand
//!   fields in struct literals; in `wright-analyzer`, `id:` literal
//!   fields too — the registry's rule ids are the vocabulary
//!   `.id`/`.id()` passthroughs surface as `code`. Non-literal `id`
//!   values (`Symbol.id`, JSON-RPC ids) are entity ids, not diagnostic
//!   codes, and stay out;
//! - `"code"` JSON keys, `["code"(..)] =` subscript writes, and
//!   `insert("code", v)`-style key-then-value calls (the envelope's
//!   serialized face);
//! - `.code`/`.provider_code`/`.refusal_code` writes: `=`, `+=`,
//!   mutating methods (`push_str`/`push`/`insert`/`insert_str`/`extend`/
//!   `replace_range`/`clone_from` classify their operands; `truncate`/
//!   `clear`/case folds/`write_fmt` fail closed; reads pass; unknown
//!   methods fail closed), plus `mem::`/`ptr::`/`write!`/`writeln!`
//!   calls that take the field as a target;
//! - `(expr, Stage::…)` tuple arms — whitespace-, path-, and
//!   glob-import-tolerant;
//! - `=>` arms, `return` values, and tail expressions inside
//!   string-returning `fn <name>code(` bodies;
//! - arguments at the position of any `code`-typed (`&str`/`String`/
//!   `impl Into<String>`/generic `T: Into<String>`) parameter of an
//!   in-crate fn, or of a `let cb = |code: &str, …|` closure binding —
//!   this covers `refusal(`, `self.error(`, `unknown(`, and friends,
//!   and keeps coverage when a new `code`-param fn appears. In
//!   wright-analyzer a string-typed `id` parameter registers too.
//!
//! Every position's expression is then classified: a literal enters the
//! grammar-checked vocabulary; a bare `code` value resolves its binding
//! through the enclosing fn (string-typed parameters and match-arm/
//! closure patterns pass through, `let` bindings are classified, and
//! `code =`/`code +=`/`code.push_str(…)` mutations are classified as
//! code positions too); wrappers (`Some`, `.to_string()`, `unwrap_or*`,
//! `or*`, …) are peeled to the producing shape — literals and `format!`
//! literals inside `unwrap_or("…")`-style fallbacks are censused too,
//! since they are codes just the same. Remaining dynamic expressions
//! must match one of the verified producers (`diagnostic_code`,
//! `provider_failure_code`, `finding["code"]`, `UNKNOWN_WITHIN`, any
//! string-returning `fn` in the scanned crates, `format!` with a
//! grammar-clean literal, or an owner/provider passthrough read) —
//! anything else is reported unclassified and fails the census.
//!
//! `wright-cli`, `wright-analyzer`, and `wright-language` are scanned
//! alongside `wright-driver`: their Diagnostics, ErrorInfo/finding codes,
//! and SourceDiagnostics all reach the same envelope.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::classify::{
    CodeSite, Sites, check_shorthand_field, classify_arm_value, classify_rhs, classify_tuple_code,
    is_code_channel, is_entity_id, push_literal,
};
use super::format::check_write_fmt;
use super::patterns::in_pattern_position;
use super::resolve::{classify_code_method, plausibly_string};
use super::sinks::{AMBIGUOUS_SINKS, code_fns, collect_code_sinks};
use super::source::{
    as_str, blank_cfg_test_items, call_args, crate_src, enclosing_call, line_of, mask_rust,
    match_bracket, read_rs_tree, rhs_span, skeleton, stage_marker_at,
};

/// Collect every literal diagnostic code from the scanned crates, plus
/// every code position whose value cannot be classified.
pub(super) fn driver_code_census() -> (Vec<CodeSite>, Vec<String>) {
    let mut codes = Vec::new();
    let mut unclassified = Vec::new();
    let mut files = read_rs_tree(&crate_src());
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

    // Mask everything once: cfg(test) items never produce sinks or
    // positions.
    let masked_files: BTreeMap<PathBuf, (String, Vec<u8>)> = files
        .iter()
        .map(|(path, original)| {
            (
                path.clone(),
                (original.clone(), blank_cfg_test_items(&mask_rust(original))),
            )
        })
        .collect();
    let sinks = collect_code_sinks(&masked_files);
    let code_fns = code_fns(&masked_files);

    for (path, (original, masked)) in &masked_files {
        let text = as_str(masked, 0..masked.len()).to_string();
        let stage_glob = text.contains("Stage::*");
        let mut sites = Sites {
            codes: &mut codes,
            unclassified: &mut unclassified,
            original,
            masked,
            file: path,
            code_fns: &code_fns,
            stage_glob,
        };

        // Every callsite of an in-crate fn taking a string `code`
        // parameter: `error(`, `self.error(`, `crate::x::refusal(`,
        // `Diagnostic::error(` — the name boundary is checked so that
        // `is_refusal(`/`map_err(` stay out, while qualified and method
        // spellings stay in.
        for (name, index) in &sinks {
            for (at, _) in text.match_indices(format!("{name}(").as_str()) {
                if at > 0 && (masked[at - 1].is_ascii_alphanumeric() || masked[at - 1] == b'_') {
                    continue;
                }
                if text[..at].trim_end().ends_with("fn") {
                    continue; // the `fn name(` definition itself
                }
                let open = at + name.len();
                let Some(span) = call_args(masked, open).get(*index).copied() else {
                    continue;
                };
                // A `&str`/`String`/`Into<String>` code parameter can
                // never receive a `vec![…]`, `json!(…)`, array, struct
                // literal, or number — that callsite belongs to a
                // different fn of the same name (`refusal(vec![diag])`
                // in wright-driver vs `refusal(code: &str)` in
                // wright-cli).
                if !plausibly_string(&skeleton(&original[span.0..span.1])) {
                    continue;
                }
                // A name shared with a std collection/string method
                // (`contains`, `insert`, `get`, …) collides with
                // non-code receivers. Its callsites only classify an
                // argument that names a code channel — literal and
                // arbitrary-typed args stay out (`registry.contains(id)`
                // vs `set.contains(&"risks")`; the codes a real
                // `contains` sees are already enumerated at their own
                // sites).
                if AMBIGUOUS_SINKS.contains(&name.as_str())
                    && !is_code_channel(
                        &sites,
                        skeleton(&original[span.0..span.1]).trim_start_matches(['&', '*']),
                    )
                {
                    continue;
                }
                classify_rhs(&mut sites, span.0, span.1);
            }
        }

        // `name:` field values — `code` itself or any `.._code` suffix
        // (`provider_code:`, `refusal_code:`, `failure_code:`, …); in
        // wright-analyzer `id:` too (entity ids still filtered below).
        // `::` path separators and destructure patterns stay out.
        // Values run to the next top-level `,`/`}`/`)`/`]`; `<>` counts
        // when written tight (`Cow<'_, str>`) so a comma inside a type
        // can not truncate the value.
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
            if !is_code_channel(&sites, name) {
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
            // Type positions (`code: String` declarations, `code: &str`
            // params) and destructure patterns (`let Diagnostic {
            // code: c }`, match arms) are not code values.
            let value = text[vstart..vend].trim();
            if value.is_empty() || looks_like_type(value) || in_pattern_position(masked, at) {
                continue;
            }
            if name == "id" {
                if push_literal(sites.codes, original, path, (vstart, vend)) {
                    continue;
                }
                let shape = skeleton(&original[vstart..vend]);
                if is_entity_id(&shape) {
                    continue;
                }
            }
            classify_rhs(&mut sites, vstart, vend);
        }

        // `"code"` JSON keys, `["code"]`/`["code".to_string()]` subscript
        // writes, and `insert("code", v)`-style key-then-value calls —
        // scanned on `original` (masked has `""`) but only at real
        // literal sites (masked shows `"` + blanked content; comments
        // are fully blank).
        for (at, _) in original.match_indices("\"code\"") {
            if masked.get(at) != Some(&b'"') || masked.get(at + 1) != Some(&b' ') {
                continue; // inside a comment or a larger literal
            }
            let mut after = at + "\"code\"".len();
            // `.to_string()`/`.to_owned()`/`.as_str()`/`.as_mut_str()`
            // suffixes on the key literal do not change the key.
            loop {
                while after < masked.len() && masked[after].is_ascii_whitespace() {
                    after += 1;
                }
                let mut stripped = false;
                for suffix in [".to_string()", ".to_owned()", ".as_str()", ".as_mut_str()"] {
                    if masked[after..].starts_with(suffix.as_bytes()) {
                        after += suffix.len();
                        stripped = true;
                        break;
                    }
                }
                if !stripped {
                    break;
                }
            }
            // `["code"(..)] =` subscript write: `]` then `=` (not `==`).
            if masked.get(after) == Some(&b']') {
                let mut i = after + 1;
                while i < masked.len() && masked[i].is_ascii_whitespace() {
                    i += 1;
                }
                if masked.get(i) == Some(&b'=') && masked.get(i + 1) != Some(&b'=') {
                    let (vstart, vend) = rhs_span(masked, i, masked.len());
                    classify_rhs(&mut sites, vstart, vend);
                }
                continue;
            }
            // `"code": <value>` — the serialized envelope face.
            if masked.get(after) == Some(&b':') {
                let mut depth = 0i32;
                let mut vend = masked.len();
                for (i, &b) in masked.iter().enumerate().skip(after + 1) {
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
                let mut vstart = after + 1;
                while vstart < vend && masked[vstart].is_ascii_whitespace() {
                    vstart += 1;
                }
                while vend > vstart && masked[vend - 1].is_ascii_whitespace() {
                    vend -= 1;
                }
                classify_rhs(&mut sites, vstart, vend);
                continue;
            }
            // `insert("code", v)` / `("code", v)` — a `"code"` literal
            // followed by `,` keys the values after it; each is a code
            // position inside the enclosing `(…)`/`[…]`/`{…}`.
            if masked.get(after) == Some(&b',')
                && let Some((_, open)) = enclosing_call(masked, at)
                && let Some(close) = match_bracket(masked, open)
            {
                let mut depth = 0i32;
                let mut seg = after + 1;
                for (i, &b) in masked.iter().enumerate().take(close).skip(after + 1) {
                    match b {
                        b'(' | b'[' | b'{' => depth += 1,
                        b')' | b']' | b'}' => depth -= 1,
                        b',' if depth == 0 => {
                            classify_rhs(&mut sites, seg, i);
                            seg = i + 1;
                        }
                        _ => {}
                    }
                }
                classify_rhs(&mut sites, seg, close);
            }
        }

        // `.code`/`.provider_code`/`.refusal_code` writes: `=`, `+=`,
        // mutating methods, and `mem::`/`ptr::`/`write!` calls that take
        // the field as a target — the write side of the read passthrough
        // whitelist.
        for field in [".code", ".provider_code", ".refusal_code"] {
            for (at, _) in text.match_indices(field) {
                let mut i = at + field.len();
                while i < text.len() && masked[i].is_ascii_whitespace() {
                    i += 1;
                }
                match masked.get(i) {
                    Some(b'=')
                        if masked.get(i + 1) != Some(&b'=') && masked.get(i + 1) != Some(&b'>') =>
                    {
                        let (vstart, vend) = rhs_span(masked, i, masked.len());
                        classify_rhs(&mut sites, vstart, vend);
                    }
                    Some(b'+') if masked.get(i + 1) == Some(&b'=') => {
                        let (vstart, vend) = rhs_span(masked, i + 1, masked.len());
                        classify_rhs(&mut sites, vstart, vend);
                    }
                    Some(b'.') => classify_code_method(&mut sites, i + 1),
                    Some(b',') | Some(b')') => {
                        // `mem::replace(&mut d.code, v)`,
                        // `ptr::write(&mut d.code, v)`, `write!(d.code,
                        // "…")` — a `.code` field inside a known mutating
                        // call is a write position; every other arg is a
                        // code position.
                        let Some((callee, open)) = enclosing_call(masked, at) else {
                            continue;
                        };
                        let args = call_args(masked, open);
                        if callee == "write!" || callee == "writeln!" {
                            check_write_fmt(&mut sites, &args, at);
                            continue;
                        }
                        let replace_like = callee.ends_with("::replace")
                            || callee.ends_with("::write")
                            || callee.ends_with("::write_unaligned")
                            || callee.ends_with("::copy")
                            || callee.ends_with("::copy_nonoverlapping");
                        let take_like = callee.ends_with("::take")
                            || callee.ends_with("::swap")
                            || callee.ends_with("::read")
                            || callee.ends_with("::write_bytes")
                            || callee.ends_with("::drop_in_place");
                        if take_like {
                            sites.unclassified.push(format!(
                                "{}:{}: `{callee}` mutates a code outside the census",
                                path.display(),
                                line_of(original, at)
                            ));
                        } else if replace_like {
                            for arg in args {
                                if arg.0 <= at && at < arg.1 {
                                    continue;
                                }
                                classify_rhs(&mut sites, arg.0, arg.1);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // `(expr, Stage::…)` tuple arms: the expression before `,` + ws +
        // a `Stage::` path is the code position — whitespace-tolerant so
        // rustfmt multi-line tuples are covered, path-tolerant so
        // `crate::diag::Stage::Frontend` counts, and glob-tolerant so
        // `use Stage::*` + a bare `Frontend` counts.
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
            // Walk back over the whole expression tail: bracketed
            // calls/indexes (`to_string()`, `xs[0]`), masked string
            // literals (`"lit".to_string()`), and ident/path/method
            // segments (`x.y::z`, `format!`).
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
                        // Masked literal: interior is blanked — find the
                        // opening quote.
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
                continue; // a `Stage::X, Stage::Y` list, not a code tuple
            }
            classify_tuple_code(&mut sites, start, end);
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
            // Only functions returning a string produce diagnostic codes.
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
                    sites.unclassified.push(format!(
                        "{}:{}: `=> {{ … }}` arm inside a code-producing fn",
                        path.display(),
                        line_of(original, arm_at)
                    ));
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
                classify_arm_value(&mut sites, arm_at, expr_start, expr_start + expr_end);
            }
            // `return`/`tail` values inside a `*code` fn body are code
            // positions too — `fn wright_refusal_code() -> String {
            // "x".to_string() }` produces a code that never passes a
            // `=>` arm. `match`/block/`for`-style values stay out: their
            // arms are scanned above and they can not carry a bare tail.
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
                classify_rhs(&mut sites, rstart, rend);
            }
            // The body's tail expression: text after the last top-level
            // `;` (blocks descend until a non-block tail shows).
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
                        classify_rhs(&mut sites, ts, te);
                    }
                    break;
                }
            }
        }

        // Shorthand `code`/`id` fields inside any struct literal in
        // expression position — `Diagnostic { code, … }`, `ErrorInfo
        // { code, … }`, `Finding { id, … }`. Patterns (let/match/for) and
        // declarations (struct/impl/…) are excluded by the head scan;
        // match-arm patterns end with `=>` and are excluded below.
        for (at, _) in text.match_indices('{') {
            // Walk back over whitespace + the path head (`Name::Variant`).
            let mut h = at;
            while h > 0 && masked[h - 1].is_ascii_whitespace() {
                h -= 1;
            }
            while h > 0
                && (masked[h - 1].is_ascii_alphanumeric()
                    || masked[h - 1] == b'_'
                    || masked[h - 1] == b':')
            {
                h -= 1;
            }
            if h == at {
                continue; // bare `{` — a block, not a struct literal
            }
            let head_word_start = h;
            // The word before the path must not end in an ident char.
            if head_word_start > 0
                && (masked[head_word_start - 1].is_ascii_alphanumeric()
                    || masked[head_word_start - 1] == b'_')
            {
                continue;
            }
            // Statement head: text since the last `;`/`{`/`}` boundary.
            let mut stmt = head_word_start;
            while stmt > 0 && !b";{}".contains(&masked[stmt - 1]) {
                stmt -= 1;
            }
            let head = text[stmt..head_word_start].trim_start();
            // A top-level `=`/`=>`/`(`/`,`/`return`/`break`/`|`/`[`/… in
            // the head puts the literal in expression position. Only a
            // leading pattern/declaration keyword keeps it out — a match
            // scrutinee (`match D { … }`) and `return`/`break` values
            // are expressions and stay in.
            let mut in_expr = false;
            let mut head_depth = 0i32;
            for &b in masked[stmt..head_word_start].iter() {
                match b {
                    b'(' | b'[' => head_depth += 1,
                    b')' | b']' if head_depth > 0 => head_depth -= 1,
                    b'=' if head_depth == 0 => {
                        in_expr = true;
                        break;
                    }
                    _ => {}
                }
            }
            let last_word = head
                .rsplit(|c: char| c.is_ascii_whitespace() || c == ':' || c == '(' || c == ',')
                .next();
            if !in_expr
                && matches!(
                    last_word,
                    Some(
                        "let"
                            | "if"
                            | "for"
                            | "while"
                            | "loop"
                            | "impl"
                            | "struct"
                            | "enum"
                            | "union"
                            | "trait"
                            | "type"
                            | "fn"
                            | "mod"
                            | "use"
                            | "where"
                            | "const"
                            | "static"
                            | "else"
                            | "in"
                            | "unsafe"
                            | "async"
                            | "pub"
                            | "do"
                    )
                )
            {
                continue;
            }
            let Some(body_end) = match_bracket(masked, at) else {
                continue;
            };
            // `Name { … } =>` is a match-arm pattern, not a literal.
            let mut after_body = body_end + 1;
            while after_body < masked.len() && masked[after_body].is_ascii_whitespace() {
                after_body += 1;
            }
            if masked.get(after_body) == Some(&b'=') && masked.get(after_body + 1) == Some(&b'>') {
                continue;
            }
            // Top-level comma-separated fields: a bare `code`/`id` segment
            // is a shorthand binding to resolve (`code:` is a named field,
            // scanned above).
            let mut depth = 0i32;
            let mut field_start = at + 1;
            for (i, &b) in masked.iter().enumerate().take(body_end).skip(at + 1) {
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
    }
    (codes, unclassified)
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
