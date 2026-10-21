//! Source-reading machinery shared by the contract censuses: comment and
//! literal masking (so structural scans never match inside a literal),
//! bracket and argument matching, the enclosing-function lookup, and the
//! `#[cfg(test)]` blanking that keeps the census on production code.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

// ===== source machinery =====

/// The crate source root (`crates/wright-driver/src`).
pub(super) fn crate_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// The pinned `workshop-rs` source root, located through cargo so the census
/// reads the exact resolved revision, path or registry alike.
pub(super) fn workshop_rs_src() -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--offline"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata must run to enumerate engine error kinds");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata output is JSON");
    let manifest = metadata["packages"]
        .as_array()
        .expect("cargo metadata has a packages array")
        .iter()
        .find(|package| package["name"] == "workshop-rs")
        .and_then(|package| package["manifest_path"].as_str())
        .expect("the resolved graph contains workshop-rs");
    PathBuf::from(manifest)
        .parent()
        .expect("the package manifest has a parent directory")
        .join("src")
}

pub(super) fn read_rs_tree(dir: &Path) -> BTreeMap<PathBuf, String> {
    let mut files = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in
            std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.insert(
                    path.clone(),
                    std::fs::read_to_string(&path)
                        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display())),
                );
            }
        }
    }
    files
}

/// A byte copy of `src` with the contents of comments, string/char literals,
/// and raw strings replaced by spaces (delimiters kept, positions and
/// newlines preserved), so structural scanning never matches inside a
/// literal.
pub(super) fn mask_rust(src: &str) -> Vec<u8> {
    let bytes = src.as_bytes();
    let mut mask = bytes.to_vec();
    let mut blank = |range: std::ops::Range<usize>| {
        for pos in range {
            if mask[pos] != b'\n' {
                mask[pos] = b' ';
            }
        }
    };
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                let end = src[i..].find('\n').map(|n| i + n).unwrap_or(bytes.len());
                blank(i..end);
                i = end;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let mut end = i + 2;
                while end < bytes.len()
                    && !(bytes[end] == b'*' && bytes.get(end + 1) == Some(&b'/'))
                {
                    end += 1;
                }
                let end = (end + 2).min(bytes.len());
                blank(i..end);
                i = end;
            }
            b'r' => {
                // Possibly a raw string: r"..." or r#"..."#…
                let mut j = i + 1;
                while bytes.get(j) == Some(&b'#') {
                    j += 1;
                }
                if bytes.get(j) != Some(&b'"') {
                    i += 1;
                    continue;
                }
                let hashes = j - i - 1;
                let close = format!("\"{}", "#".repeat(hashes));
                let end = src[j + 1..]
                    .find(&close)
                    .map(|n| j + 1 + n + close.len())
                    .unwrap_or(bytes.len());
                blank(i..end);
                i = end;
            }
            b'"' => {
                let mut end = i + 1;
                while end < bytes.len() && bytes[end] != b'"' {
                    if bytes[end] == b'\\' {
                        end += 1;
                    }
                    end += 1;
                }
                blank(i + 1..end.min(bytes.len()));
                i = (end + 1).min(bytes.len());
            }
            b'\'' if bytes.get(i + 1) == Some(&b'\\') => {
                // An escaped char literal (`'\n'`, `'\''`, `'\u{1F600}'`):
                // scan to the closing quote. Lifetimes never carry `\`.
                let mut end = i + 2;
                if bytes.get(end) == Some(&b'\'') {
                    end += 1; // `'\''` — the escaped char is a quote itself
                }
                while end < bytes.len()
                    && bytes[end] != b'\''
                    && bytes[end] != b'\n'
                    && end - i <= 12
                {
                    end += 1;
                }
                if end < bytes.len() && bytes[end] == b'\'' && end > i + 2 {
                    blank(i + 1..end);
                    i = end + 1;
                } else {
                    i += 1;
                }
            }
            b'\'' => {
                // A bare char literal is exactly one char between quotes —
                // `'x'`, `'é'` — so a `'a`/`'static` lifetime, whose next
                // quote is never one char away, stays unmasked.
                let len = match bytes.get(i + 1) {
                    Some(b) if b.is_ascii() => 1,
                    Some(b) if (0xC0..=0xDFu8).contains(b) => 2,
                    Some(b) if (0xE0..=0xEFu8).contains(b) => 3,
                    Some(b) if (0xF0..=0xF7u8).contains(b) => 4,
                    _ => 0,
                };
                if len > 0 && bytes.get(i + 1 + len) == Some(&b'\'') {
                    blank(i + 1..i + 1 + len);
                    i += len + 2;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    mask
}

pub(super) fn as_str(mask: &[u8], range: std::ops::Range<usize>) -> &str {
    std::str::from_utf8(&mask[range]).expect("masked source stays valid UTF-8")
}

/// Blank every `#[cfg(test)]`-gated item (positions preserved) so only
/// production code is scanned.
pub(super) fn blank_cfg_test_items(mask: &[u8]) -> Vec<u8> {
    let mut out = mask.to_vec();
    let text = String::from_utf8_lossy(mask);
    let mut search_from = 0;
    while let Some(at) = text[search_from..]
        .find("#[cfg(test)]")
        .map(|n| search_from + n)
    {
        let mut i = at + "#[cfg(test)]".len();
        // The gated item may carry further attributes; skip `#[…]` groups
        // and whitespace to the item itself.
        loop {
            while i < mask.len() && (mask[i] as char).is_ascii_whitespace() {
                i += 1;
            }
            if mask[i..].starts_with(b"#[") {
                i = match_bracket(mask, i + 1)
                    .map(|end| end + 1)
                    .unwrap_or(i + 2);
            } else {
                break;
            }
        }
        // The item ends at the first top-level `;` or at the balanced `}`
        // closing its first `{`.
        let mut depth = 0i32;
        let mut end = i;
        while end < mask.len() {
            match mask[end] {
                b';' if depth == 0 => {
                    end += 1;
                    break;
                }
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end += 1;
                        break;
                    }
                }
                _ => {}
            }
            end += 1;
        }
        for byte in &mut out[at..end.min(mask.len())] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
        search_from = end;
    }
    out
}

/// The index of the bracket matching the `(`, `[`, or `{` at `open`.
pub(super) fn match_bracket(mask: &[u8], open: usize) -> Option<usize> {
    let (open_ch, close_ch) = match mask.get(open) {
        Some(b'(') => (b'(', b')'),
        Some(b'[') => (b'[', b']'),
        Some(b'{') => (b'{', b'}'),
        _ => return None,
    };
    let mut depth = 0i32;
    for (i, &b) in mask.iter().enumerate().skip(open) {
        if b == open_ch {
            depth += 1;
        } else if b == close_ch {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// The (start, end) byte ranges of the top-level arguments of the call
/// whose `(` sits at `open`.
pub(super) fn call_args(mask: &[u8], open: usize) -> Vec<(usize, usize)> {
    let mut args = Vec::new();
    let mut depth = 0i32;
    let mut arg_start = open + 1;
    for (i, &b) in mask.iter().enumerate().skip(open + 1) {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    args.push((arg_start, i));
                    return args;
                }
                depth -= 1;
            }
            b',' if depth == 0 => {
                args.push((arg_start, i));
                arg_start = i + 1;
            }
            _ => {}
        }
    }
    args
}

/// The (name, params) of the nearest `fn` whose body encloses `pos`; params
/// are (name, type) pairs with the receiver omitted.
pub(super) fn enclosing_fn(mask: &[u8], pos: usize) -> Option<(String, Vec<(String, String)>)> {
    let text = String::from_utf8_lossy(mask);
    let mut search_end = pos;
    loop {
        let fn_at = text[..search_end].rfind("fn ")?;
        let name_start = fn_at + 3;
        let name_end = text[name_start..]
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .map(|n| name_start + n)?;
        let paren = text[name_end..].find('(').map(|n| name_end + n)?;
        let params_end = match_bracket(mask, paren)?;
        let body = text[params_end..].find('{').map(|n| params_end + n)?;
        let body_end = match_bracket(mask, body)?;
        if pos > body && pos < body_end {
            let params = call_args(mask, paren)
                .iter()
                .filter_map(|(s, e)| {
                    let param = text[*s..*e].trim();
                    if param.is_empty()
                        || param == "self"
                        || param == "&self"
                        || param == "&mut self"
                    {
                        return None;
                    }
                    let (name, ty) = param.split_once(':')?;
                    Some((
                        name.trim().trim_start_matches("mut ").to_string(),
                        ty.trim().to_string(),
                    ))
                })
                .collect();
            return Some((text[name_start..name_end].to_string(), params));
        }
        search_end = fn_at;
    }
}

/// The `(start, end)` byte range of the `{…}` body of the `fn` enclosing
/// `pos` — the same walk `enclosing_fn` performs, returning the span.
pub(super) fn enclosing_fn_body(mask: &[u8], pos: usize) -> Option<(usize, usize)> {
    let text = String::from_utf8_lossy(mask);
    let mut search_end = pos;
    loop {
        let fn_at = text[..search_end].rfind("fn ")?;
        let name_start = fn_at + 3;
        let name_end = text[name_start..]
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .map(|n| name_start + n)?;
        let paren = text[name_end..].find('(').map(|n| name_end + n)?;
        let params_end = match_bracket(mask, paren)?;
        let body = text[params_end..].find('{').map(|n| params_end + n)?;
        let body_end = match_bracket(mask, body)?;
        if pos > body && pos < body_end {
            return Some((body, body_end));
        }
        search_end = fn_at;
    }
}

pub(super) fn line_of(src: &str, pos: usize) -> usize {
    src[..pos].matches('\n').count() + 1
}

/// `expr` with all whitespace and string-literal contents removed — the
/// shape of a code-position expression, for the dynamic-producer rules.
pub(super) fn skeleton(expr: &str) -> String {
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

/// The expression span following an `=` at `eq`, up to the statement's
/// top-level `;` or `limit`.
pub(super) fn rhs_span(masked: &[u8], eq: usize, limit: usize) -> (usize, usize) {
    let mut depth = 0i32;
    let mut end = limit;
    for (j, &b) in masked.iter().enumerate().skip(eq + 1) {
        if j >= limit {
            break;
        }
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' if depth > 0 => depth -= 1,
            b';' if depth == 0 => {
                end = j;
                break;
            }
            _ => {}
        }
    }
    let mut start = eq + 1;
    while start < end && masked[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && masked[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    (start, end)
}

/// The span `(open_brace, close)` of each `match` body inside `masked`,
/// searched from `from`. The scrutinee expression may itself contain
/// braces (closures, struct literals), so the body brace is the first
/// `{` at depth 0 after the `match` keyword.
fn match_bodies(masked: &[u8], from: usize) -> Vec<(usize, usize)> {
    let mut bodies = Vec::new();
    let mut i = from;
    while i + 5 <= masked.len() {
        if &masked[i..i + 5] == b"match"
            && (i == 0 || !(masked[i - 1].is_ascii_alphanumeric() || masked[i - 1] == b'_'))
            && masked
                .get(i + 5)
                .is_none_or(|b| !(b.is_ascii_alphanumeric() || *b == b'_'))
        {
            let mut j = i + 5;
            let mut depth = 0i32;
            let mut brace = None;
            while j < masked.len() {
                match masked[j] {
                    b'{' if depth == 0 => {
                        brace = Some(j);
                        break;
                    }
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => {
                        depth -= 1;
                        if depth < 0 {
                            break;
                        }
                    }
                    b';' if depth == 0 => break,
                    _ => {}
                }
                j += 1;
            }
            if let Some(open) = brace
                && let Some(close) = match_bracket(masked, open)
            {
                bodies.push((open, close));
                // Continue inside the body so nested `match`es in the
                // scrutinee or arm values are collected too.
                i = open + 1;
                continue;
            }
        }
        i += 1;
    }
    bodies
}

/// Whether `, <ws>Stage::` appears in `bytes` — whitespace-tolerant, so
/// rustfmt multi-line tuples (`"…",\n    Stage::Frontend`) count too.
pub(super) fn stage_marker(bytes: &[u8]) -> bool {
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b',' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if bytes[j..].starts_with(b"Stage::") {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Whether every `=>` arm of every `match` inside `masked_rhs` produces a
/// `, Stage::` tuple — the shape the tuple-arm scan covers. A `match`-free
/// RHS defers to passthrough; a `match` with any unmarked arm fails
/// closed. An arm value ends at the next top-level `,`, the next arm's
/// `=>`, or the match's `}` — a comma-free block arm can not swallow the
/// arms after it.
pub(super) fn match_arms_stage_ok(masked_rhs: &str) -> bool {
    let bytes = masked_rhs.as_bytes();
    for (open, close) in match_bodies(bytes, 0) {
        let mut depth = 1i32;
        let mut i = open + 1;
        while i < close {
            match bytes[i] {
                b'{' | b'(' | b'[' => depth += 1,
                b'}' | b')' | b']' => depth -= 1,
                b'=' if depth == 1 && bytes.get(i + 1) == Some(&b'>') => {
                    let vstart = i + 2;
                    let mut vend = close;
                    let mut arm_depth = 0i32;
                    for (j, &b) in bytes.iter().enumerate().take(close).skip(vstart) {
                        match b {
                            b'{' | b'(' | b'[' => arm_depth += 1,
                            b'}' | b')' | b']' => {
                                if arm_depth == 0 {
                                    vend = j;
                                    break;
                                }
                                arm_depth -= 1;
                            }
                            b',' if arm_depth == 0 => {
                                vend = j;
                                break;
                            }
                            b'=' if arm_depth == 0 && bytes.get(j + 1) == Some(&b'>') => {
                                vend = j;
                                break;
                            }
                            _ => {}
                        }
                    }
                    if !stage_marker(&bytes[vstart..vend]) {
                        return false;
                    }
                    i = vend;
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
    }
    true
}

/// Whether `name` is bound by the pattern of ANY match arm containing
/// `pos` — e.g. `Some(code) if pred(code) => (code, …)` binds `code` in
/// the pattern, the guard, and the value; an outer arm's binding is still
/// in scope for positions inside a nested arm that does not rebind it.
pub(super) fn arm_pattern_binds(masked: &[u8], pos: usize, name: &str) -> bool {
    let bytes = masked;
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'=' && bytes[i + 1] == b'>' {
            // The arm's level is the bracket depth just before `=>`.
            let mut depth = 0i32;
            for &b in &bytes[..i] {
                match b {
                    b'{' | b'(' | b'[' => depth += 1,
                    b'}' | b')' | b']' => depth -= 1,
                    _ => {}
                }
            }
            // Arm value: from after `=>` to the next `,` or `=>` at the
            // same depth, or the `}` that closes the match — a comma-free
            // block arm can not swallow later arms.
            let mut vend = bytes.len();
            let mut d = depth;
            for (j, &b) in bytes.iter().enumerate().skip(i + 2) {
                match b {
                    b'{' | b'(' | b'[' => d += 1,
                    b'}' | b')' | b']' => {
                        if d == depth {
                            vend = j;
                            break;
                        }
                        d -= 1;
                    }
                    b',' if d == depth => {
                        vend = j;
                        break;
                    }
                    b'=' if d == depth && bytes.get(j + 1) == Some(&b'>') => {
                        vend = j;
                        break;
                    }
                    _ => {}
                }
            }
            // Pattern start: the `,`/`{` at `depth` before the arrow.
            let mut pstart = 0;
            {
                let mut pd = depth;
                for j in (0..i).rev() {
                    match bytes[j] {
                        b'}' | b')' | b']' => pd += 1,
                        b'{' | b'(' | b'[' => {
                            if pd == depth {
                                pstart = j + 1;
                                break;
                            }
                            pd -= 1;
                        }
                        b',' if pd == depth => {
                            pstart = j + 1;
                            break;
                        }
                        _ => {}
                    }
                }
            }
            // The arm binds `name` across pattern, guard, and value.
            if pos >= pstart && pos <= vend {
                let pattern = as_str(masked, pstart..i);
                let binds = pattern.match_indices(name).any(|(idx, _)| {
                    let before = pattern[..idx].chars().last();
                    let after = pattern[idx + name.len()..].chars().next();
                    !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                        && !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                });
                if binds {
                    return true;
                }
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    false
}

/// Whether `pos` sits inside a destructuring pattern — a `let`/`if let`/
/// `while let` pattern (terminated by a top-level `=`/`;`), a `for`
/// pattern (terminated by ` in `), or a match arm's pattern region
/// (before its `=>`). Field scans must skip patterns: `code:`/`code`
/// there names a binding, not a produced code.
pub(super) fn in_pattern_position(masked: &[u8], pos: usize) -> bool {
    let text = as_str(masked, 0..masked.len());
    // `let …`/`if let …`/`while let …` pattern: pos before the top-level
    // `=`/`;` that ends the nearest preceding `let` pattern.
    if let Some(lt) = text[..pos].rfind("let ") {
        let mut depth = 0i32;
        let mut in_pattern = true;
        for &b in masked.iter().take(pos).skip(lt + 4) {
            match b {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b'=' | b';' if depth == 0 => {
                    in_pattern = false;
                    break;
                }
                _ => {}
            }
        }
        if in_pattern {
            return true;
        }
    }
    // `for …` pattern: pos before a top-level ` in `.
    if let Some(ft) = text[..pos].rfind("for ") {
        let mut depth = 0i32;
        let mut in_pattern = true;
        let mut j = ft + 4;
        while j < pos {
            match masked[j] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                _ => {}
            }
            if depth == 0 && masked[j..].starts_with(b" in ") {
                in_pattern = false;
                break;
            }
            j += 1;
        }
        if in_pattern {
            return true;
        }
    }
    // Match arm pattern: inside a match body, before the arm's `=>` —
    // scanning forward, a `=>` at body depth precedes any `,`/`}`.
    for (open, close) in match_bodies(masked, 0) {
        if pos <= open || pos >= close {
            continue;
        }
        let mut depth = 0i32;
        for j in pos..close {
            match masked[j] {
                b'{' | b'(' | b'[' => depth += 1,
                b'}' | b')' | b']' => {
                    if depth == 0 {
                        break; // end of the match body — `pos` was a value
                    }
                    depth -= 1;
                }
                b',' if depth == 0 => break, // value boundary
                b'=' if depth == 0 && masked.get(j + 1) == Some(&b'>') => return true,
                _ => {}
            }
        }
    }
    false
}

/// Whether `name` is bound by a `|…|` closure parameter list whose body
/// contains `pos` — e.g. `|code: &str| { …code… }` inside a fn.
pub(super) fn closure_param_binds(masked: &[u8], pos: usize, name: &str) -> bool {
    let mut i = 0;
    while i < pos && i < masked.len() {
        if masked[i] != b'|' || masked.get(i + 1) == Some(&b'|') {
            i += 1;
            continue;
        }
        // Find the closing `|` of the param list (params are idents,
        // `mut`, types, commas — no `|` and no operators).
        let mut j = i + 1;
        let mut close = None;
        while j < masked.len() && masked[j] != b'\n' {
            match masked[j] {
                b'|' => {
                    close = Some(j);
                    break;
                }
                b'(' | b'[' | b'{' | b')' | b']' | b'}' | b';' | b'=' | b'<' | b'>' | b'&'
                | b'*' => {
                    // Not a param list — or a `&`/`Type<…>` we can still
                    // parse; `(`/`{`/`;`/`=` end it.
                    if matches!(
                        masked[j],
                        b'(' | b'[' | b'{' | b')' | b']' | b'}' | b';' | b'='
                    ) {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        let Some(close) = close else {
            i += 1;
            continue;
        };
        // The closure body: `{…}` to its match, else a single expression
        // to the next top-level `,`/`;`/`)`/`]`/`}` at the closure's depth.
        let mut k = close + 1;
        while k < masked.len() && masked[k].is_ascii_whitespace() {
            k += 1;
        }
        let body_end = if masked.get(k) == Some(&b'{') {
            match_bracket(masked, k).unwrap_or(masked.len())
        } else {
            // Expression: it ends at a separator at the depth the closure
            // sits at — approximate with the depth at `i`.
            let mut depth = 0i32;
            for &b in &masked[..i] {
                match b {
                    b'{' | b'(' | b'[' => depth += 1,
                    b'}' | b')' | b']' => depth -= 1,
                    _ => {}
                }
            }
            let mut end = masked.len();
            let mut d = depth;
            for (n, &b) in masked.iter().enumerate().skip(close + 1) {
                match b {
                    b'{' | b'(' | b'[' => d += 1,
                    b'}' | b')' | b']' => {
                        if d == depth {
                            end = n;
                            break;
                        }
                        d -= 1;
                    }
                    b',' | b';' if d == depth => {
                        end = n;
                        break;
                    }
                    _ => {}
                }
            }
            end
        };
        if pos > close && pos < body_end {
            let param_text = as_str(masked, i + 1..close);
            if param_text.split(',').any(|p| {
                p.trim()
                    .split(':')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .trim_start_matches("mut ")
                    .trim()
                    == name
            }) {
                return true;
            }
        }
        i = close + 1;
    }
    false
}
