//! Bracket and statement-head helpers for the literal classifier:
//! finding enclosing brackets, reading the "head" that decides whether
//! a bracket is a pattern or an expression, and the
//! pattern/attribute membership tests.

use super::source::as_str;

/// The first non-whitespace byte index at or after `i`.
pub(super) fn next_nonws(masked: &[u8], mut i: usize) -> Option<usize> {
    while i < masked.len() {
        if !masked[i].is_ascii_whitespace() {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// The last non-whitespace byte index before `i`.
pub(super) fn prev_nonws(masked: &[u8], i: usize) -> Option<usize> {
    let mut j = i;
    while j > 0 {
        if !masked[j - 1].is_ascii_whitespace() {
            return Some(j - 1);
        }
        j -= 1;
    }
    None
}

/// The innermost unmatched `(`/`[`/`{` opening bracket before `pos`.
pub(super) fn enclosing_open(masked: &[u8], pos: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut j = pos;
    while j > 0 {
        j -= 1;
        match masked[j] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' => {
                if depth == 0 {
                    return Some(j);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// The byte range from the nearest statement boundary to `open` — the
/// "head" that decides whether a bracket is a pattern or an expression.
/// The scan stops at a same-level `;`, `,`, or enclosing bracket.
pub(super) fn bracket_head(masked: &[u8], open: usize) -> (usize, usize) {
    let mut depth = 0i32;
    let mut start = open;
    while start > 0 {
        match masked[start - 1] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            b';' | b',' if depth == 0 => break,
            _ => {}
        }
        start -= 1;
    }
    (start, open)
}

/// The byte range from the nearest `;`/unbalanced open bracket at this
/// level to `at`, skipping over balanced brackets — the statement head
/// used for assignment LHS and block-role reads.
pub(super) fn stmt_head(masked: &[u8], at: usize) -> (usize, usize) {
    let mut depth = 0i32;
    let mut start = at;
    while start > 0 {
        match masked[start - 1] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            b';' | b',' if depth == 0 => break,
            _ => {}
        }
        start -= 1;
    }
    (start, at)
}

/// The identifier/keyword ending just before `end` (whitespace trimmed).
pub(super) fn word_before(masked: &[u8], mut end: usize) -> (usize, usize) {
    while end > 0 && masked[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && (masked[start - 1].is_ascii_alphanumeric() || masked[start - 1] == b'_') {
        start -= 1;
    }
    (start, end)
}

/// Construct keywords that decide what a `{` is — checked at
/// top-level bracket depth of the statement head so a `if`/`match`
/// inside an earlier item's balanced body cannot leak into the
/// verdict. Visibility/qualifier words (`pub`, `const`, `let`, `mut`,
/// `unsafe`, `async`, `move`, `return`, …) are absent on purpose:
/// they qualify whatever follows and never head a construct whose
/// tail is discarded.
pub(super) const CONSTRUCT_KWS: &[&str] = &[
    "if",
    "else",
    "while",
    "for",
    "match",
    "loop",
    "fn",
    "impl",
    "trait",
    "mod",
    "struct",
    "enum",
    "union",
    "use",
    "type",
    "macro_rules",
    "asm",
    "trait_alias",
];

/// The head suffix that heads this bracket — the text after the last
/// top-level `}`. A complete `if c { … }`/`match { … }` earlier in the
/// head is a finished statement (`if c { } let x = {`), so only the
/// tail decides what the bracket being asked about is.
pub(super) fn head_tail(head: &str) -> &str {
    let mut depth = 0i32;
    let mut tail = 0;
    for (i, &b) in head.as_bytes().iter().enumerate() {
        match b {
            b'{' | b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    tail = i + 1;
                }
            }
            _ => {}
        }
    }
    &head[tail..]
}

/// The (offset, word) pairs sitting at top-level bracket depth in
/// `head`: `()[]{}` contents are skipped, so words inside earlier
/// items' balanced bodies never leak into a construct verdict —
/// statement heads can legitimately span several items when the last
/// `;` is far away.
pub(super) fn depth0_words(head: &str) -> Vec<(usize, &str)> {
    let bytes = head.as_bytes();
    let mut depth = 0i32;
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b if depth == 0
                && (b.is_ascii_alphanumeric() || b == b'_')
                && (i == 0
                    || (!(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')
                        && bytes[i - 1] != b'#')) =>
            {
                let mut j = i + 1;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    j += 1;
                }
                out.push((i, &head[i..j]));
                i = j;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// The last construct keyword at top-level bracket depth in `head`,
/// with its offset.
pub(super) fn last_construct_kw(head: &str) -> Option<(&str, usize)> {
    depth0_words(head)
        .iter()
        .rev()
        .find(|(_, w)| CONSTRUCT_KWS.contains(w))
        .map(|&(i, w)| (w, i))
}

/// The byte offset of the `if` opening the if-chain `head` ends in.
/// Scanning the head's depth-0 words backward, an `if` right after
/// `else` is an `else if` mid-chain and the walk continues past the
/// previous arm's balanced `{}`; the first `if` whose predecessor is
/// not `else` heads the chain. Complete `if` statements earlier in
/// the head are skipped the same way — their trailing `if` is the
/// last depth-0 word before the current block's condition.
pub(super) fn if_chain_start(head: &str) -> Option<usize> {
    let words = depth0_words(head);
    for (i, &(pos, w)) in words.iter().enumerate().rev() {
        if w == "if" && (i == 0 || words[i - 1].1 != "else") {
            return Some(pos);
        }
    }
    None
}

/// Whether `head` contains a top-level `=` (assignment, `==`, `=>`,
/// compound) — the marker that the bracket after it sits in expression
/// territory, never in a pattern.
pub(super) fn head_has_eq(masked: &[u8], range: (usize, usize)) -> bool {
    let mut depth = 0i32;
    for &b in &masked[range.0..range.1] {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'=' if depth == 0 => return true,
            _ => {}
        }
    }
    false
}

/// Whether `pos` sits inside a `match` body's pattern segment — after
/// the match `{` but before the arm's `=>` at the same level.
fn in_match_pattern_segment(masked: &[u8], match_open: usize, pos: usize) -> bool {
    let mut depth = 0i32;
    let mut pattern = true;
    for (i, &b) in masked.iter().enumerate().take(pos).skip(match_open + 1) {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'=' if depth == 0 && masked.get(i + 1) == Some(&b'>') => pattern = false,
            b',' if depth == 0 => pattern = true,
            _ => {}
        }
    }
    pattern
}

/// Whether the bracket at `open` is a match body — `match` is the
/// last construct keyword at depth 0 of its head (`match e {`,
/// `x = match e {`, `return match e {`). Depth-0 keeps a `match`
/// inside an earlier item's balanced body from leaking in.
pub(super) fn is_match_body(masked: &[u8], open: usize) -> bool {
    let (hs, he) = stmt_head(masked, open);
    let head = as_str(masked, hs..he);
    last_construct_kw(head_tail(head)).is_some_and(|(w, _)| w == "match")
}

/// Whether `pos` sits in pattern position — a `let`/`if let`/`while
/// let`/`for`/match-arm destructure. Every enclosing bracket is asked:
/// a bracket is a pattern when its head binds a pattern — `let`/`for`
/// at top-level bracket depth — *and no top-level `=` stands before
/// it* — `let x = T { code: v }` carries `=` before the `{`, so the
/// `code:` field is an expression value, while `let T { code: c } = x`
/// has none and stays a pattern. Depth-0 scanning keeps `let`/`for`
/// inside earlier items' balanced bodies from leaking in. A match
/// body defers to its arm segment: before the `=>` is pattern.
pub(super) fn in_pattern(masked: &[u8], pos: usize) -> bool {
    let mut at = pos;
    for _ in 0..32 {
        let Some(open) = enclosing_open(masked, at) else {
            return false;
        };
        let kind = masked[open];
        let (hs, he) = bracket_head(masked, open);
        if kind == b'{' && is_match_body(masked, open) {
            return in_match_pattern_segment(masked, open, at);
        }
        if head_has_eq(masked, (hs, he)) {
            return false;
        }
        let head = as_str(masked, hs..he);
        let words = depth0_words(head);
        // `let` at depth 0 heads a binding — `let (a, b)`, `if let`,
        // `while let`, `else if let` — pattern side until the `=`
        // (which `head_has_eq` already ruled out).
        if words.iter().any(|&(_, w)| w == "let") {
            return true;
        }
        // `for (a, b)`/`for x` — the pattern is the bracket's content.
        if kind == b'(' && words.iter().any(|&(_, w)| w == "for") {
            return true;
        }
        if kind == b'{' {
            // Any other `{` is a body (fn, loop, if, impl, block) —
            // expression territory, never a pattern.
            return false;
        }
        at = open;
    }
    false
}

/// Whether `pos` sits inside a `use` item — `use a::b`,
/// `pub use a::{b, c}`, nested `use a::{b::{c}}` trees included. A name
/// there is a path segment of a declaration, never a produced value.
/// The walk checks the statement head at `pos`'s level, then each
/// enclosing bracket's head — a nested use tree's inner `{` head is
/// `b::`, so the `use` lives one level out.
pub(super) fn in_use_item(masked: &[u8], pos: usize) -> bool {
    let mut at = pos;
    for _ in 0..32 {
        let (hs, he) = stmt_head(masked, at);
        let head = as_str(masked, hs..he);
        if depth0_words(head).iter().any(|&(_, w)| w == "use") {
            return true;
        }
        match enclosing_open(masked, at) {
            Some(open) => at = open,
            None => return false,
        }
    }
    false
}

/// Whether the `{` at `open` opens a struct/enum-variant literal —
/// `S {`, `a::b::T {`, `Self {`, `S<T> {` — rather than a block or a
/// `use` tree. The statement head decides: a construct keyword
/// (`if`/`match`/`fn`/`use`/…) heads a block or item, `=>`/`->` marks
/// an arm body or a return type, and qualifier words
/// (`unsafe`/`async`/`move`/`const`/…) head blocks; a head ending in an
/// identifier or `>` is a path — a struct literal. This is the check
/// `block_role`'s `ExprBlock` bucket misses, so produced-value
/// expansion must not absorb a struct literal's braces.
pub(super) fn is_struct_lit_brace(masked: &[u8], open: usize) -> bool {
    let (hs, he) = stmt_head(masked, open);
    let head = head_tail(as_str(masked, hs..he)).trim_end();
    if head.is_empty() || head.ends_with("=>") || head.contains("->") {
        return false;
    }
    let words = depth0_words(head);
    if words
        .iter()
        .any(|&(_, w)| CONSTRUCT_KWS.contains(&w) || w == "use")
    {
        return false;
    }
    if words.last().is_some_and(|&(_, w)| {
        [
            "unsafe", "async", "move", "do", "try", "const", "static", "let", "return", "pub",
        ]
        .contains(&w)
    }) {
        return false;
    }
    head.as_bytes()
        .last()
        .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'>')
}

/// Whether `pos` sits inside a `#[]`/`#![]` attribute — walk enclosing
/// brackets for a `[` preceded by `#`.
pub(super) fn in_attribute(masked: &[u8], pos: usize) -> bool {
    let mut at = pos;
    for _ in 0..32 {
        let Some(open) = enclosing_open(masked, at) else {
            return false;
        };
        if masked[open] == b'[' {
            let mut j = open;
            while j > 0 && masked[j - 1].is_ascii_whitespace() {
                j -= 1;
            }
            if j > 0 && masked[j - 1] == b'#' {
                return true;
            }
        }
        at = open;
    }
    false
}
