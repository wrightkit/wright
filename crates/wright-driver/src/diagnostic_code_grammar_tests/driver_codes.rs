//! The driver fixed-code census: every literal diagnostic code in a
//! production code position of the crates that feed the envelope, plus
//! every code position whose value cannot be classified.
//!
//! A code position is anywhere a value can reach a diagnostic `code`:
//!
//! - first arguments of `Diagnostic::error`/`warning`/`info`;
//! - `code:` field values and `, code` shorthand fields in struct
//!   literals; in `wright-analyzer`, `id:` literal fields too — the
//!   registry's rule ids are the vocabulary `.id`/`.id()` passthroughs
//!   surface as `code`. Non-literal `id` values (`Symbol.id`, JSON-RPC
//!   ids) are entity ids, not diagnostic codes, and stay out;
//! - `"code"` JSON keys and `["code"]` subscript writes (the envelope's
//!   serialized face);
//! - `.code` writes: `=`, `+=`, `push_str`/`push`/`insert`/`extend`/
//!   `replace_range`;
//! - `(expr, Stage::…)` tuple arms;
//! - `=>` arms inside string-returning `fn <name>code(` bodies;
//! - arguments at the position of any `code`-typed (`&str`/`String`/
//!   `impl Into<String>`) parameter of an in-crate fn — this covers
//!   `refusal(`, `self.error(`, `unknown(`, and friends, and keeps
//!   coverage when a new `code`-param fn appears.
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
    plausibly_string, push_literal,
};
use super::source::{
    as_str, blank_cfg_test_items, call_args, crate_src, line_of, mask_rust, match_bracket,
    read_rs_tree, rhs_span, skeleton,
};

/// `fn` names whose parameter list carries a string-typed `code`
/// parameter, with that parameter's index (receiver excluded). Any
/// callsite's argument at that index is a code position.
type CodeSinks = BTreeMap<String, usize>;

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
        let analyzer = path.to_string_lossy().contains("wright-analyzer");
        let mut sites = Sites {
            codes: &mut codes,
            unclassified: &mut unclassified,
            original,
            masked,
            file: path,
            code_names: &["code"],
            code_fns: &code_fns,
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
                classify_rhs(&mut sites, span.0, span.1);
            }
        }

        // `code:`/`id:` field values (word boundary on the name, so
        // `provider_code:`/`grid:` stay out). Values run to the next
        // top-level `,`/`}`/`)`/`]`.
        for field in sites
            .code_names
            .iter()
            .map(|name| format!("{name}:"))
            .collect::<Vec<_>>()
        {
            let field = field.as_str();
            for (at, _) in text.match_indices(field) {
                if at > 0
                    && (text.as_bytes()[at - 1].is_ascii_alphanumeric()
                        || text.as_bytes()[at - 1] == b'_')
                {
                    continue;
                }
                let mut depth = 0i32;
                let mut vend = masked.len();
                for (i, &b) in masked.iter().enumerate().skip(at + field.len()) {
                    match b {
                        b'(' | b'[' | b'{' => depth += 1,
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
                let vstart = at
                    + field.len()
                    + (text[at + field.len()..].len()
                        - text[at + field.len()..].trim_start().len());
                while vend > vstart && (text.as_bytes()[vend - 1] as char).is_ascii_whitespace() {
                    vend -= 1;
                }
                // Type positions (`code: String` declarations, `code: &str`
                // params) are not code values.
                let value = text[vstart..vend].trim();
                if value.is_empty() || looks_like_type(value) {
                    continue;
                }
                classify_rhs(&mut sites, vstart, vend);
            }
        }

        // Analyzer rule-code vocabulary: `id: "lint-…"` literals in the
        // registry are the codes `.id`/`.id()` passthroughs surface. Only
        // literal values are censused — `Symbol.id`/`RuleDefinition.id`
        // and JSON-RPC ids are entity ids, not diagnostic codes.
        if analyzer {
            for (at, _) in text.match_indices("id:") {
                if at > 0
                    && (text.as_bytes()[at - 1].is_ascii_alphanumeric()
                        || text.as_bytes()[at - 1] == b'_')
                {
                    continue;
                }
                let mut depth = 0i32;
                let mut vend = masked.len();
                for (i, &b) in masked.iter().enumerate().skip(at + "id:".len()) {
                    match b {
                        b'(' | b'[' | b'{' => depth += 1,
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
                let vstart = at
                    + "id:".len()
                    + (text[at + "id:".len()..].len()
                        - text[at + "id:".len()..].trim_start().len());
                while vend > vstart && (text.as_bytes()[vend - 1] as char).is_ascii_whitespace() {
                    vend -= 1;
                }
                push_literal(sites.codes, original, path, (vstart, vend));
            }
        }

        // `"code"` JSON keys and `["code"]` subscript writes — scanned on
        // `original` (masked has `""`) but only at real literal sites
        // (masked shows `"` + blanked content; comments are fully blank).
        for (at, _) in original.match_indices("\"code\"") {
            if masked.get(at) != Some(&b'"') || masked.get(at + 1) != Some(&b' ') {
                continue; // inside a comment or a larger literal
            }
            let mut after = at + "\"code\"".len();
            while after < masked.len() && masked[after].is_ascii_whitespace() {
                after += 1;
            }
            // `["code"] =` subscript write: `]` then `=` (not `==`).
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
            }
        }

        // `.code` writes: `=`, `+=`, and mutating methods whose operand is
        // a code value.
        for (at, _) in text.match_indices(".code") {
            let mut i = at + ".code".len();
            while i < text.len() && masked[i].is_ascii_whitespace() {
                i += 1;
            }
            match masked.get(i) {
                Some(b'=') | Some(b'+') => {
                    if masked.get(i + 1) == Some(&b'=') || masked.get(i + 1) == Some(&b'>') {
                        continue;
                    }
                    let (vstart, vend) = rhs_span(masked, i, masked.len());
                    classify_rhs(&mut sites, vstart, vend);
                }
                Some(b'.') => {
                    let after_dot = i + 1;
                    for method in ["push_str(", "push(", "insert(", "extend(", "replace_range("] {
                        if masked[after_dot..].starts_with(method.as_bytes()) {
                            let open = after_dot + method.len() - 1;
                            for arg in call_args(masked, open) {
                                classify_rhs(&mut sites, arg.0, arg.1);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // `(expr, Stage::…)` tuple arms: the expression directly before
        // `, Stage::` is the code position.
        for (at, _) in text.match_indices(", Stage::") {
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
            if expr.starts_with("Stage::") {
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
            // A top-level `=`/`=>`/`(`/`,`/`return`/`. ` in the head puts
            // the literal in expression position. Without one a leading
            // `let`/`if let`/`for`/`match`/… keyword is a pattern or
            // declaration — skip.
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
                && !matches!(last_word, Some("return" | "=>" | "(" | "," | "."))
                && !head.ends_with("=>")
                && !head.ends_with('(')
                && !head.ends_with(',')
                && !head.is_empty()
            {
                continue;
            }
            if !in_expr
                && matches!(
                    last_word,
                    Some(
                        "let"
                            | "if"
                            | "for"
                            | "while"
                            | "match"
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
                            | "return"
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

/// Collect `fn` names whose parameter list carries a string-typed `code`
/// parameter (`code: &str`, `code: String`, `code: impl Into<String>`),
/// with the parameter's index (the receiver does not count). Built from
/// the masked sources so `#[cfg(test)]` helpers never register.
fn collect_code_sinks(files: &BTreeMap<PathBuf, (String, Vec<u8>)>) -> CodeSinks {
    let mut sinks = BTreeMap::new();
    for (original, masked) in files.values() {
        let text = as_str(masked, 0..masked.len()).to_string();
        for (at, _) in text.match_indices("fn ") {
            let name_start = at + 3;
            let name_end = text[name_start..]
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .map(|n| name_start + n);
            let Some(name_end) = name_end else {
                continue;
            };
            let name = original[name_start..name_end].to_string();
            // Skip generics `<'a>` / `<'a, T>` before the param list.
            let mut p = name_end;
            while p < masked.len() && masked[p].is_ascii_whitespace() {
                p += 1;
            }
            if masked.get(p) == Some(&b'<') {
                let mut depth = 0i32;
                let mut q = p;
                while q < masked.len() {
                    match masked[q] {
                        b'<' => depth += 1,
                        b'>' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    q += 1;
                }
                p = q + 1;
                while p < masked.len() && masked[p].is_ascii_whitespace() {
                    p += 1;
                }
            }
            if masked.get(p) != Some(&b'(') {
                continue;
            }
            let params = call_args(masked, p);
            // The receiver (`self`/`&self`/…) carries no `:` and does not
            // consume a callsite argument — index counts only real params.
            let mut index = 0usize;
            for (ps, pe) in params {
                let param = as_str(masked, ps..pe).trim();
                let Some((pname, ptype)) = param.split_once(':') else {
                    continue;
                };
                let pname = pname.trim().trim_start_matches("mut ").trim();
                let ptype = ptype.trim();
                let string_typed =
                    ptype.contains("str") || ptype.contains("String") || ptype.contains("Into");
                if pname == "code" && string_typed {
                    sinks.entry(name.clone()).or_insert(index);
                }
                index += 1;
            }
        }
    }
    sinks
}

/// Names of string-returning `fn <name>code(` — the fns whose `=>` arms
/// the census already scans. An outermost call to one in a code position
/// produces a code through that scanned body.
fn code_fns(files: &BTreeMap<PathBuf, (String, Vec<u8>)>) -> std::collections::BTreeSet<String> {
    let mut fns = std::collections::BTreeSet::new();
    for (original, masked) in files.values() {
        let text = as_str(masked, 0..masked.len()).to_string();
        for (at, _) in text.match_indices("fn ") {
            let name_start = at + 3;
            let Some(name_end) = text[name_start..]
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .map(|n| name_start + n)
            else {
                continue;
            };
            let name = &original[name_start..name_end];
            if !name.ends_with("code") {
                continue;
            }
            let Some(brace) = text[name_end..].find('{').map(|n| name_end + n) else {
                continue;
            };
            let returns = text[name_end..brace].rsplit("->").next().unwrap_or("");
            if returns.contains("str") || returns.contains("String") {
                fns.insert(name.to_string());
            }
        }
    }
    fns
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
