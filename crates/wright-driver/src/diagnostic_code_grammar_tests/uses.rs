//! Use-side helpers for the literal classifier: the minimal local
//! type resolution (`param : Type`, `let x = expr`, `const X: Type`,
//! `fn f() -> Type`, `impl Type`) that proves what a receiver or a
//! path leaf names, the `+`-concatenation / `.join()`-produced-string
//! span helpers, `Stage`-tuple element detection, and `format!`-family
//! template-hole parsing.
//!
//! These are deliberately shallow: a type the tables cannot name is
//! `None`, and `slot.rs`/`dynamic.rs` turn `None` into `Unresolved` —
//! never a silent `Other`.

use super::brackets::{depth0_words, enclosing_open, next_nonws, prev_nonws, stmt_head};
use super::classify::FileCtx;
use super::expand::{expr_start_before, is_operand_end};
use super::source::{as_str, enclosing_fn, enclosing_fn_span, match_bracket};

/// Type leaves the local tables never define — receivers of these
/// types prove a qualified ambiguous callsite is *not* the registered
/// sink's owner (`Vec::contains`, `str::contains`, `BTreeSet::contains`).
const BUILTIN_TYPES: &[&str] = &[
    "str",
    "String",
    "char",
    "bool",
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "usize",
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "isize",
    "f32",
    "f64",
    "slice",
    "tuple",
    "Vec",
    "VecDeque",
    "Option",
    "Result",
    "Box",
    "Rc",
    "Arc",
    "Cell",
    "RefCell",
    "Mutex",
    "RwLock",
    "HashMap",
    "BTreeMap",
    "HashSet",
    "BTreeSet",
    "PathBuf",
    "Path",
    "OsString",
    "OsStr",
    "Cow",
    "Duration",
    "Instant",
    "Arguments",
    "Range",
    "RangeInclusive",
];

/// The leaf type name of a type spelling — `&'a mut Vec<T>` → `Vec`,
/// `[T; N]` → `slice`, `(A, B)` → `tuple`, `impl Into<String>` →
/// `Into`, `Option<T>` → `Option`. `""` when the spelling carries no
/// name.
pub(super) fn ty_leaf(ty: &str) -> String {
    let t = ty
        .trim()
        .trim_start_matches('&')
        .trim_start_matches('\'')
        .trim_start_matches(|c: char| {
            c.is_ascii_alphanumeric() && c.is_ascii_lowercase() && c == '\''
        })
        .trim_start_matches("mut ")
        .trim_start_matches('*')
        .trim();
    let t = t
        .strip_prefix("impl ")
        .or_else(|| t.strip_prefix("dyn "))
        .unwrap_or(t)
        .trim();
    if t.starts_with('[') {
        return "slice".to_string();
    }
    if t.starts_with('(') {
        return "tuple".to_string();
    }
    let mut out = String::new();
    for c in t.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            break;
        }
    }
    out
}

/// The `impl` block type enclosing `pos` — `impl X {`, `impl X<T> {`,
/// `impl Trait for X {` all own `X`.
pub(super) fn impl_owner(masked: &[u8], pos: usize) -> Option<String> {
    let mut at = pos;
    for _ in 0..32 {
        let open = enclosing_open(masked, at)?;
        if masked[open] != b'{' {
            at = open;
            continue;
        }
        let (hs, he) = stmt_head(masked, open);
        let head = as_str(masked, hs..he);
        let Some(&(kat, _)) = depth0_words(head).iter().rev().find(|&(_, w)| *w == "impl") else {
            at = open;
            continue;
        };
        let rest = head[kat + 4..].trim();
        // `impl Trait for X` — the owner is the type after `for`.
        let owner = rest.rfind(" for ").map_or(rest, |fat| &rest[fat + 5..]);
        let name: String = owner
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        return (!name.is_empty()).then_some(name);
    }
    None
}

/// What a receiver-type check proved about a qualified ambiguous
/// callsite.
pub(super) enum OwnerCheck {
    /// The receiver type is the sink declaration's `impl` owner.
    Owner,
    /// The receiver type is provably something else — a std type or a
    /// locally defined type the declaration does not own.
    Other,
    /// Nothing proves the receiver type — generic parameters, member
    /// expressions, unknown callees — and the callsite fails closed.
    Unknown,
}

/// The leaf type name the expression `rs..re` produces — `input` is
/// `str`, `Vec::new()` is `Vec`, `KINDS` is its `const` type.
/// `None` when nothing in the local tables proves it.
pub(super) fn expr_ty(ctx: &FileCtx, rs: usize, re: usize) -> Option<String> {
    expr_ty_text(ctx, as_str(ctx.masked, rs..re).trim(), rs)
}

fn expr_ty_text(ctx: &FileCtx, t: &str, pos: usize) -> Option<String> {
    let t = t.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(rest) = t.strip_prefix('&') {
        return expr_ty_text(ctx, rest.trim_start_matches("mut ").trim(), pos);
    }
    if let Some(rest) = t.strip_prefix('*') {
        return expr_ty_text(ctx, rest.trim(), pos);
    }
    // `(expr)` — a single-element group.
    if t.starts_with('(') && t.ends_with(')') {
        return expr_ty_text(ctx, &t[1..t.len() - 1], pos);
    }
    if t.starts_with('"') || t.starts_with("r\"") || t.starts_with("b\"") {
        return Some("str".to_string());
    }
    if t.starts_with('\'') {
        return Some("char".to_string());
    }
    if t.starts_with('[') {
        return Some("slice".to_string());
    }
    if t.starts_with("vec!") {
        return Some("Vec".to_string());
    }
    if t.starts_with("format!") {
        return Some("String".to_string());
    }
    if t.starts_with("format_args") {
        return Some("Arguments".to_string());
    }
    if t.starts_with("concat!") {
        return Some("str".to_string());
    }
    if t == "self" || t == "Self" {
        return impl_owner(ctx.masked, pos);
    }
    if let Some(op) = t.find('(') {
        let callee = t[..op].trim_end();
        if let Some(dot) = callee.rfind('.') {
            // `recv.method(…)` — string-producing methods name
            // themselves; `clone`/`unwrap`/`or`-family keep the
            // receiver's type; everything else is unproven.
            let method = &callee[dot + 1..];
            if [
                "to_string",
                "to_owned",
                "to_string_lossy",
                "format",
                "join",
                "concat",
                "collect",
                "repeat",
                "replace",
                "to_uppercase",
                "to_lowercase",
            ]
            .contains(&method)
            {
                return Some("String".to_string());
            }
            if method == "as_str"
                || method == "to_str"
                || method.starts_with("trim")
                || method.starts_with("strip")
            {
                return Some("str".to_string());
            }
            if [
                "clone",
                "copied",
                "unwrap",
                "expect",
                "unwrap_or",
                "unwrap_or_else",
                "or",
                "or_else",
            ]
            .contains(&method)
            {
                return expr_ty_text(ctx, callee[..dot].trim(), pos);
            }
            return None;
        }
        // `Type::assoc(…)` produces the type; `m::f(…)` and `f(…)`
        // resolve through the `fn` return table.
        let mut segs = callee.rsplit("::");
        let leaf = segs.next().unwrap_or(callee).trim();
        if let Some(penult) = segs.next() {
            let penult = penult
                .trim()
                .trim_end_matches('>')
                .rsplit('<')
                .next()
                .unwrap_or("")
                .trim();
            if penult == "Self" {
                return impl_owner(ctx.masked, pos);
            }
            if penult.chars().next().is_some_and(char::is_uppercase) {
                return Some(ty_leaf(penult));
            }
        }
        if let Some(ret) = ctx.tables.rets.get(leaf) {
            return Some(ret.clone());
        }
        return None;
    }
    // Bare ident, `m::NAME`, or `a.b` member.
    let tail = t.rsplit([':', '.']).next().unwrap_or(t).trim();
    if tail.is_empty() || !tail.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    if t.contains(':') || t.contains('.') {
        // `m::NAME` resolves through the const table; `a.b` member
        // paths stay unproven.
        return ctx.tables.consts.get(tail).cloned();
    }
    ident_ty(ctx, tail, pos)
}

/// The declared type of ident `name` visible at `pos` — `fn` params,
/// then the last `let name`/`let name: T`/`let name = expr` before
/// `pos` in the enclosing fn, then `const`/`static` tables.
fn ident_ty(ctx: &FileCtx, name: &str, pos: usize) -> Option<String> {
    if let Some((_, params)) = enclosing_fn(ctx.masked, pos)
        && let Some((_, pt)) = params.iter().find(|(pn, _)| pn == name)
    {
        return Some(ty_leaf(pt));
    }
    if let Some(ty) = let_ty(ctx, name, pos) {
        return Some(ty);
    }
    ctx.tables.consts.get(name).cloned()
}

/// The last `let name`/`let name: T`/`let name = expr` binding's type
/// before `pos` inside the enclosing fn body — shadowing resolves to
/// the nearest prior declaration.
fn let_ty(ctx: &FileCtx, name: &str, pos: usize) -> Option<String> {
    let (_, body_open, body_end) = enclosing_fn_span(ctx.masked, pos)?;
    let text = as_str(ctx.masked, body_open..body_end).to_string();
    let mut found = None;
    for (at, _) in text.match_indices("let ") {
        let a = body_open + at;
        if a >= pos {
            break;
        }
        if a > 0 && (ctx.masked[a - 1].is_ascii_alphanumeric() || ctx.masked[a - 1] == b'_') {
            continue;
        }
        let mut j = a + 4;
        while j < body_end && ctx.masked[j].is_ascii_whitespace() {
            j += 1;
        }
        if ctx.masked[j..].starts_with(b"mut ") {
            j += 4;
            while j < body_end && ctx.masked[j].is_ascii_whitespace() {
                j += 1;
            }
        }
        let id_start = j;
        while j < body_end && (ctx.masked[j].is_ascii_alphanumeric() || ctx.masked[j] == b'_') {
            j += 1;
        }
        if as_str(ctx.masked, id_start..j) != name {
            continue;
        }
        while j < body_end && ctx.masked[j].is_ascii_whitespace() {
            j += 1;
        }
        if ctx.masked.get(j) == Some(&b':') && ctx.masked.get(j + 1) != Some(&b':') {
            // `let name: Type = …` — the annotation is the type.
            let ty_start = j + 1;
            let mut ty_end = ty_start;
            let mut depth = 0i32;
            while ty_end < body_end {
                match ctx.masked[ty_end] {
                    b'<' | b'(' | b'[' | b'{' => depth += 1,
                    b'>' | b')' | b']' | b'}' => depth -= 1,
                    b'=' | b';' if depth <= 0 => break,
                    _ => {}
                }
                ty_end += 1;
            }
            let leaf = ty_leaf(as_str(ctx.masked, ty_start..ty_end).trim());
            if !leaf.is_empty() {
                found = Some(leaf);
            }
        } else if ctx.masked.get(j) == Some(&b'=') && ctx.masked.get(j + 1) != Some(&b'=') {
            // `let name = expr` — the produced type of `expr`.
            let rhs_start = j + 1;
            let mut rhs_end = rhs_start;
            let mut depth = 0i32;
            while rhs_end < body_end {
                match ctx.masked[rhs_end] {
                    b'(' | b'[' | b'{' | b'<' => depth += 1,
                    b')' | b']' | b'}' | b'>' => depth -= 1,
                    b';' if depth <= 0 => break,
                    _ => {}
                }
                rhs_end += 1;
            }
            if let Some(ty) = expr_ty_text(ctx, as_str(ctx.masked, rhs_start..rhs_end), rhs_start) {
                found = Some(ty);
            }
        }
    }
    found
}

/// Whether the receiver `rs..re` of a qualified ambiguous callsite
/// proves the declaration's owner — `recv: &Recv` on `impl Recv`'s
/// method is `Owner`, `Vec`/`BTreeSet` are `Other`, anything unproven
/// is `Unknown`.
pub(super) fn receiver_owner(ctx: &FileCtx, rs: usize, re: usize, owner: &str) -> OwnerCheck {
    match expr_ty(ctx, rs, re) {
        Some(ty) if ty == owner => OwnerCheck::Owner,
        Some(ty) if BUILTIN_TYPES.contains(&ty.as_str()) || ctx.tables.defined.contains(&ty) => {
            OwnerCheck::Other
        }
        Some(_) | None => OwnerCheck::Unknown,
    }
}

/// The named `{…}` hole root-identifiers in cooked `format!` template
/// text — `{name}`/`{name:spec}`/`{name.field?}` all report `name`;
/// positional `{}`/`{0}` and `{{`/`}}` escapes report nothing. The
/// second element is whether the whole template is exactly one bare
/// `{ident}` hole — only then does a named hole's value pass through
/// as the produced string whole.
pub(super) fn template_holes(cooked: &str) -> (Vec<String>, bool) {
    let bytes = cooked.as_bytes();
    let mut holes = Vec::new();
    let mut covers = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' if bytes.get(i + 1) == Some(&b'{') => i += 2,
            b'}' if bytes.get(i + 1) == Some(&b'}') => i += 2,
            b'{' => {
                let Some(close) = bytes[i..].iter().position(|&b| b == b'}').map(|n| i + n) else {
                    break;
                };
                let inner = &cooked[i + 1..close];
                let name = inner
                    .split([':', '.', '?', '!', '}'])
                    .next()
                    .unwrap_or("")
                    .trim();
                if !name.is_empty()
                    && name
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    covers |= i == 0 && close == cooked.len() - 1 && inner.trim() == name;
                    holes.push(name.to_string());
                }
                i = close + 1;
            }
            _ => i += 1,
        }
    }
    (holes, covers)
}

/// The cooked text of `original[a..b]` when it is a `"…"`/`r"…"`/
/// `r#"…"#` literal — the format-template content without quotes.
pub(super) fn template_text(original: &str, a: usize, b: usize) -> Option<String> {
    let t = original[a..b].trim();
    if let Some(rest) = t.strip_prefix('r') {
        let (hashes, rest) = rest.split_once('"')?;
        if !hashes.chars().all(|c| c == '#') {
            return None;
        }
        let close = format!("\"{}", "#".repeat(hashes.len()));
        return rest
            .strip_suffix(close.as_str())
            .map(|inner| inner.to_string());
    }
    t.strip_prefix('"')?
        .strip_suffix('"')
        .map(|inner| inner.to_string())
}

/// The end of the operand starting at or after `i` — prefix ops, a
/// literal, a bracketed expression, or `ident(::ident)*` with
/// `.method(…)`/`[idx]`/`?`/`!` suffixes. The forward mirror of
/// [`expr_start_before`].
pub(super) fn operand_end(masked: &[u8], mut i: usize) -> usize {
    while i < masked.len() && masked[i].is_ascii_whitespace() {
        i += 1;
    }
    while i < masked.len() && matches!(masked[i], b'&' | b'*' | b'+' | b'-' | b'!' | b'~') {
        i += 1;
        while i < masked.len() && masked[i].is_ascii_whitespace() {
            i += 1;
        }
    }
    if i >= masked.len() {
        return i;
    }
    match masked[i] {
        b'"' | b'\'' => {
            let q = masked[i];
            i += 1;
            while i < masked.len() && masked[i] != q {
                i += 1;
            }
            i += 1;
        }
        b'(' | b'[' | b'{' => {
            i = match_bracket(masked, i).map(|c| c + 1).unwrap_or(i + 1);
        }
        c if c.is_ascii_alphanumeric() || c == b'_' => {
            while i < masked.len() && (masked[i].is_ascii_alphanumeric() || masked[i] == b'_') {
                i += 1;
            }
        }
        _ => i += 1,
    }
    // Suffixes: `?`, `::ident`, `.member`/`(args)`, `[idx]`, turbofish.
    loop {
        let mut j = i;
        while j < masked.len() && masked[j].is_ascii_whitespace() {
            j += 1;
        }
        match masked.get(j) {
            Some(b'?') => i = j + 1,
            Some(b'.') if masked.get(j + 1) != Some(&b'.') => {
                let mut k = j + 1;
                while k < masked.len() && (masked[k].is_ascii_alphanumeric() || masked[k] == b'_') {
                    k += 1;
                }
                let mut l = k;
                while l < masked.len() && masked[l].is_ascii_whitespace() {
                    l += 1;
                }
                if masked.get(l) == Some(&b'(') || masked.get(l) == Some(&b'[') {
                    i = match_bracket(masked, l).map(|c| c + 1).unwrap_or(k);
                } else {
                    i = k;
                }
            }
            Some(b':') if masked.get(j + 1) == Some(&b':') => {
                let mut k = j + 2;
                while k < masked.len() && (masked[k].is_ascii_alphanumeric() || masked[k] == b'_') {
                    k += 1;
                }
                i = k;
            }
            Some(b'(') | Some(b'[') => {
                i = match_bracket(masked, j).map(|c| c + 1).unwrap_or(i);
            }
            _ => break,
        }
        if i == j {
            break;
        }
    }
    i
}

/// The full `"a" + b + "c"` chain span a literal `s..e` belongs to —
/// walks `+` both directions when each side carries a real operand.
/// `(s, e)` unchanged when no `+` applies.
pub(super) fn concat_span(masked: &[u8], s: usize, e: usize) -> (usize, usize) {
    let (mut cs, mut ce) = (s, e);
    loop {
        let start = (cs, ce);
        if let Some(p) = prev_nonws(masked, cs)
            && masked[p] == b'+'
            && masked.get(p + 1) != Some(&b'=')
            && prev_nonws(masked, p).is_some_and(|q| is_operand_end(masked[q]))
        {
            cs = expr_start_before(masked, p);
        }
        if let Some(a) = next_nonws(masked, ce)
            && masked[a] == b'+'
            && masked.get(a + 1) != Some(&b'=')
        {
            ce = operand_end(masked, a + 1);
        }
        if (cs, ce) == start {
            break;
        }
    }
    (cs, ce)
}

/// Whether the element text `masked[a..b]` is a `Stage` marker —
/// `Stage::X`, `x::Stage::X`, or under `stage_glob` a bare `UpperCamel`
/// unit variant. The mirror of `source::stage_marker_at` for a whole
/// element rather than the token after a comma.
pub(super) fn element_is_stage(masked: &[u8], a: usize, b: usize, stage_glob: bool) -> bool {
    let t = as_str(masked, a..b).trim();
    if t.starts_with("Stage::") || t.contains("::Stage::") {
        return true;
    }
    stage_glob
        && !t.is_empty()
        && t.chars().next().is_some_and(char::is_uppercase)
        && !t.contains(':')
        && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !t.ends_with('(')
}
