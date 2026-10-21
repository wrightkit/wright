//! The driver fixed-code census: every literal diagnostic code in a
//! production code position of the crates that feed the envelope, plus
//! an explicit report of the dynamic (non-literal) positions.
//!
//! A code position is anywhere a value can reach a diagnostic `code`:
//! `Diagnostic::error`/`warning`/`refusal(` calls and any in-crate fn or
//! closure whose `code` parameter is string-typed (analyzer adds `id`);
//! `code:`/`.._code:` field values; `"code"` JSON keys and key-value
//! call/subscript/tuple forms; `.code` writes (`=`, `+=`, mutating
//! methods, `mem::`/`ptr::`/`write!` targets); bare `code`/`id` binding
//! mutations; `(expr, Stage::…)` tuples; `=>`/`return`/tail expressions
//! inside string-returning `fn <name>code(` bodies; `for <code-ish>`
//! iterables.
//!
//! Literals reachable through transparent wrappers (`Some`, fallbacks,
//! `if`/`match` arms) enter the vocabulary; the residual shape is
//! reported dynamic, never classified — produced values are checked at
//! runtime by the `debug_assert!` code-token assertions inside
//! `Diagnostic::error`, `refusal`, `provider_failure_code`, and the LSP
//! error writer, and by the engine-kind tests for `kind`-built shapes.
//!
//! `wright-cli`, `wright-analyzer`, `wright-language`, and `wright-lsp`
//! are scanned alongside `wright-driver`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::positions::{
    CodeSite, Sites, arg_names_code_channel, in_pattern_position, is_code_field, mutates_string,
    plausibly_string, take_position, take_string_args,
};
use super::sinks::{AMBIGUOUS_SINKS, collect_code_sinks};
use super::source::{
    as_str, blank_cfg_test_items, call_args, crate_src, enclosing_call, line_of, mask_rust,
    match_bracket, read_rs_tree, rhs_span, skeleton, stage_marker_at,
};

/// Collect every literal diagnostic code from the scanned crates, plus
/// the dynamic-position report.
pub(super) fn driver_code_census() -> (Vec<CodeSite>, Vec<String>) {
    let mut codes = Vec::new();
    let mut dynamic = Vec::new();
    let mut files = read_rs_tree(&crate_src());
    if let Some(crates) = crate_src().parent().and_then(Path::parent) {
        for member in [
            "wright-cli",
            "wright-analyzer",
            "wright-language",
            "wright-lsp",
        ] {
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

    for (path, (original, masked)) in &masked_files {
        let text = as_str(masked, 0..masked.len()).to_string();
        let stage_glob = text.contains("Stage::*");
        let analyzer = path.to_string_lossy().contains("wright-analyzer");
        let mut sites = Sites {
            codes: &mut codes,
            dynamic: &mut dynamic,
            original,
            masked,
            file: path,
            scan: "census",
        };

        // Every callsite of an in-crate fn taking a string `code`
        // parameter: `error(`, `self.error(`, `crate::x::refusal(`,
        // `Diagnostic::error(` — the name boundary is checked so that
        // `is_refusal(`/`map_err(` stay out, while qualified and method
        // spellings stay in.
        sites.scan = "sinks";
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
                take_position(&mut sites, span.0, span.1);
            }
        }

        // `name:` field values — `code` itself or any `.._code` suffix
        // (`provider_code:`, `refusal_code:`, `failure_code:`, …); in
        // wright-analyzer `id:` too. `::` path separators and destructure
        // patterns stay out.
        sites.scan = "field";
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
            if value.is_empty() || looks_like_type(value) || in_pattern_position(masked, at) {
                continue;
            }
            take_position(&mut sites, vstart, vend);
        }

        // `"code"` JSON keys, `["code"(..)] =` subscript writes, and
        // `insert("code", v)`-style key-then-value calls — scanned on
        // `original` (masked has `""`) but only at real literal sites.
        sites.scan = "jsonkey";
        for (at, _) in original.match_indices("\"code\"") {
            if masked.get(at) != Some(&b'"') || masked.get(at + 1) != Some(&b' ') {
                continue; // inside a comment or a larger literal
            }
            let mut after = at + "\"code\"".len();
            // `.to_string()`/`.to_owned()`/`.as_str()`/`.into()` suffixes
            // on the key literal do not change the key.
            loop {
                while after < masked.len() && masked[after].is_ascii_whitespace() {
                    after += 1;
                }
                let mut stripped = false;
                for suffix in [
                    ".to_string()",
                    ".to_owned()",
                    ".as_str()",
                    ".as_mut_str()",
                    ".into()",
                    ".clone()",
                ] {
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
                    take_position(&mut sites, vstart, vend);
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
                take_position(&mut sites, vstart, vend);
                continue;
            }
            // `insert("code", v)` / `("code", v)` / `["code", v]` — a
            // `"code"` literal followed by `,` keys the values after it
            // inside the enclosing `(…)`/`[…]`/`{…}`.
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
                            take_position(&mut sites, seg, i);
                            seg = i + 1;
                        }
                        _ => {}
                    }
                }
                take_position(&mut sites, seg, close);
                continue;
            }
            // `entry("code").or_insert(v)`/`or_insert_with(|| v)` — the
            // `"code"` arg closes the key call; the value is the next
            // method's argument.
            if masked.get(after) == Some(&b')')
                && let Some((callee, _)) = enclosing_call(masked, at)
                && (callee.ends_with("::entry") || callee == "entry")
            {
                let mut i = after + 1;
                while i < masked.len()
                    && masked[i] != b'('
                    && masked[i] != b';'
                    && masked[i] != b'{'
                {
                    i += 1;
                }
                if masked.get(i) == Some(&b'(')
                    && let Some(close) = match_bracket(masked, i)
                {
                    for (s, e) in call_args(masked, i) {
                        let _ = close;
                        take_position(&mut sites, s, e);
                    }
                }
            }
        }

        // `.code`/`.provider_code`/`.refusal_code` writes (analyzer adds
        // `.id`): `=`, `+=`, mutating methods, and `mem::`/`ptr::`/`write!`
        // calls that take the field as a target.
        sites.scan = "fieldwrite";
        for field in [".code", ".provider_code", ".refusal_code", ".id"] {
            if field == ".id" && !analyzer {
                continue;
            }
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
                        take_position(&mut sites, vstart, vend);
                    }
                    Some(b'+') if masked.get(i + 1) == Some(&b'=') => {
                        let (vstart, vend) = rhs_span(masked, i + 1, masked.len());
                        take_position(&mut sites, vstart, vend);
                    }
                    Some(b'.') => {
                        // Only mutating methods write into the code —
                        // predicates and readers (`starts_with`, `len`)
                        // stay out.
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
                            take_string_args(&mut sites, masked, j, close);
                        }
                    }
                    Some(b',') | Some(b')') => {
                        sites.scan = Box::leak(format!("fieldwrite@{at}").into_boxed_str());
                        // `mem::replace(&mut d.code, v)`,
                        // `ptr::write(&mut d.code, v)`,
                        // `write!(d.code, "…")` — other args are code
                        // positions.
                        let Some((callee, open)) = enclosing_call(masked, at) else {
                            continue;
                        };
                        let args = call_args(masked, open);
                        if callee == "write!" || callee == "writeln!" {
                            // arg[0] is the writer (the `.code` field);
                            // the format literal is a code position when
                            // it has no holes, dynamic when it does —
                            // `take_position` handles both.
                            for arg in args.iter().skip(1) {
                                take_position(&mut sites, arg.0, arg.1);
                            }
                            continue;
                        }
                        // Only field-mutating calls make the sibling
                        // args code positions; `.code` inside any other
                        // call is a read.
                        let mutation_like = callee.ends_with("::replace")
                            || callee.ends_with("::write")
                            || callee.ends_with("::write_unaligned")
                            || callee.ends_with("::copy")
                            || callee.ends_with("::copy_nonoverlapping")
                            || callee.ends_with("::take")
                            || callee.ends_with("::swap")
                            || callee.ends_with("::read")
                            || callee.ends_with("::write_bytes")
                            || callee.ends_with("::drop_in_place")
                            || callee.ends_with("::clone_from");
                        if mutation_like {
                            for arg in args {
                                if arg.0 <= at && at < arg.1 {
                                    continue;
                                }
                                take_position(&mut sites, arg.0, arg.1);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Bare `code`/`id` binding writes with no `.`: `code = …`,
        // `code += …`, `code.push_str(…)`-style methods, and
        // `write!(code, …)`/`writeln!(code, …)` into a bound code var.
        sites.scan = "baremut";
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
                    // `code = …` / `code += …` (not `==`, not `code:`).
                    Some(b'=')
                        if masked.get(i + 1) != Some(&b'=') && masked.get(i + 1) != Some(&b'>') =>
                    {
                        let (vstart, vend) = rhs_span(masked, i, masked.len());
                        take_position(&mut sites, vstart, vend);
                    }
                    Some(b'+') if masked.get(i + 1) == Some(&b'=') => {
                        let (vstart, vend) = rhs_span(masked, i + 1, masked.len());
                        take_position(&mut sites, vstart, vend);
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
                            take_string_args(&mut sites, masked, j, close);
                        }
                    }
                    Some(b',') | Some(b')') => {
                        // `write!(code, "…")` — the bound var is the
                        // writer; the format arg is a code position.
                        let Some((callee, open)) = enclosing_call(masked, at) else {
                            continue;
                        };
                        if callee == "write!" || callee == "writeln!" {
                            for arg in call_args(masked, open).iter().skip(1) {
                                take_position(&mut sites, arg.0, arg.1);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // `(expr, Stage::…)` tuple positions: the expression before `,` +
        // ws + a `Stage::` path is the code position — whitespace-,
        // path-, and glob-tolerant. A call whose argument precedes
        // `, Stage::` is also taken; that over-includes (the arg may be
        // a message), which only adds literals to the vocabulary —
        // reported sites stay honest.
        sites.scan = "stagetuple";
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
            // calls/indexes, masked string literals, and
            // ident/path/method segments.
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
                continue; // a `Stage::X, Stage::Y` list, not a code tuple
            }
            take_position(&mut sites, start, end);
        }

        // `=> "lit"` arms, `return` values, and tail expressions inside
        // `fn <name>code(` bodies — `code`, `provider_failure_code`,
        // `diagnostic_code`, and friends.
        sites.scan = "codefn";
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
                    sites.dynamic.push(format!(
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
                take_position(&mut sites, expr_start, expr_start + expr_end);
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
                take_position(&mut sites, rstart, rend);
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
                        take_position(&mut sites, ts, te);
                    }
                    break;
                }
            }
        }

        // `for <code-ish pattern> in <iterable>`: the iterable's literals
        // are values the binding can carry into a sink.
        sites.scan = "foriter";
        for (at, _) in text.match_indices("for ") {
            let head = &text[at + 4..];
            let Some(in_at) = head.find(" in ") else {
                continue;
            };
            let pat = head[..in_at].trim();
            if !(pat == "code"
                || pat == "id"
                || pat.ends_with("_code")
                || pat.contains("code,")
                || pat.contains(", code"))
            {
                continue;
            }
            let iter_start = at + 4 + in_at + 4;
            // The iterable runs to the loop body's `{` at depth 0.
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
                take_position(&mut sites, it_start, iter_end);
            }
        }
    }
    (codes, dynamic)
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
