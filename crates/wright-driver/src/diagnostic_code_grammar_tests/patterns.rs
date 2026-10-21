//! Pattern and `match`-arm scanners shared by the contract censuses:
//! where a `let`/`for`/arm pattern binds a name, which source positions
//! sit inside a pattern, and whether every `match` arm carries a
//! `, Stage::` tuple marker.

use super::source::{as_str, match_bracket, stage_marker};

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

/// Whether every `=>` arm of every `match` inside `masked_rhs` produces a
/// `, Stage::` tuple — the shape the tuple-arm scan covers. A `match`-free
/// RHS defers to passthrough; a `match` with any unmarked arm fails
/// closed. An arm value ends at the next top-level `,`, the next arm's
/// `=>`, or the match's `}` — a comma-free block arm can not swallow the
/// arms after it.
pub(super) fn match_arms_stage_ok(masked_rhs: &str, stage_glob: bool) -> bool {
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
                    if !stage_marker(&bytes[vstart..vend], stage_glob) {
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
    // Only `=>`s inside real `match` bodies are arms — an `if let`
    // destructures through the `let` path instead, and macro shapes can
    // not fake an arm.
    let bodies = match_bodies(bytes, 0);
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'=' && bytes[i + 1] == b'>' {
            if !bodies.iter().any(|&(open, close)| i > open && i < close) {
                i += 2;
                continue;
            }
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
    // `=`/`;` that ends the nearest preceding `let` pattern — matched
    // at a word boundary so `outlet `/`tablet ` can not fake a `let `.
    if let Some(lt) = text[..pos].rfind("let ")
        && (lt == 0 || !masked[lt - 1].is_ascii_alphanumeric() && masked[lt - 1] != b'_')
    {
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
    // `for …` pattern: pos before a top-level ` in ` — same word
    // boundary (`before ` can not fake `for `).
    if let Some(ft) = text[..pos].rfind("for ")
        && (ft == 0 || !masked[ft - 1].is_ascii_alphanumeric() && masked[ft - 1] != b'_')
    {
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

/// Whether `name` is bound by the pattern of a `for` loop whose body
/// contains `pos` — `for (code, stage) in pairs { … }`.
pub(super) fn for_pattern_binds(masked: &[u8], pos: usize, name: &str) -> bool {
    let text = as_str(masked, 0..masked.len());
    for (at, _) in text[..pos.min(text.len())].match_indices("for ") {
        if at > 0 && (masked[at - 1].is_ascii_alphanumeric() || masked[at - 1] == b'_') {
            continue;
        }
        // The pattern runs to a top-level ` in `.
        let mut j = at + 4;
        let mut depth = 0i32;
        let mut in_at = None;
        while j + 4 < text.len() {
            match masked[j] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                _ => {}
            }
            if depth == 0 && &text[j..j + 4] == " in " {
                in_at = Some(j);
                break;
            }
            j += 1;
        }
        let Some(in_at) = in_at else {
            continue;
        };
        let pattern = text[at + 4..in_at].trim();
        // The loop body starts at the first `{` after the `in` expr.
        let mut k = in_at + 4;
        let mut d2 = 0i32;
        let mut brace = None;
        while k < masked.len() {
            match masked[k] {
                b'(' | b'[' => d2 += 1,
                b')' | b']' => d2 -= 1,
                b'{' if d2 == 0 => {
                    brace = Some(k);
                    break;
                }
                b'{' => d2 += 1,
                b'}' => d2 -= 1,
                _ => {}
            }
            k += 1;
        }
        let Some(brace) = brace else {
            continue;
        };
        let Some(close) = match_bracket(masked, brace) else {
            continue;
        };
        if pos > brace && pos < close {
            return pattern_contains_ident(pattern, name);
        }
    }
    false
}

/// Whether `pattern` binds `name` at a word boundary (`Some(code)`,
/// `(code, stage)`, `{ code: c }` — the `code:` there names a FIELD,
/// not a `code` binding, so `code:` is excluded).
fn pattern_contains_ident(pattern: &str, name: &str) -> bool {
    pattern.match_indices(name).any(|(i, _)| {
        let before = pattern[..i].chars().last();
        let after = pattern[i + name.len()..].chars().next();
        !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            && !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            && !pattern[i + name.len()..].trim_start().starts_with(':')
    })
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
