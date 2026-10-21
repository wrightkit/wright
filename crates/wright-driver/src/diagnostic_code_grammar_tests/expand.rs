//! Transparent-structure expansion for the literal classifier: the
//! outward walk from a literal to the expression whose slot has
//! evidence — through `Some`/`Ok`/`&`/`.to_string()` wrappers, fallback
//! combinators (`x.unwrap_or(alt)`), `=>`/if-arm/closure produced-value
//! bodies, and grouping parens.
//!
//! A position is transparent when the value the subexpression carries
//! is the value the whole expression produces; opaque callees stop the
//! walk and their argument stays data (`f("lit")` classifies as `f`'s
//! arg, not as the produced call result).

use super::brackets::{
    enclosing_open, if_chain_start, is_match_body, last_construct_kw, next_nonws, prev_nonws,
    stmt_head, word_before,
};
use super::source::{as_str, call_args, match_bracket};

/// Callee shape before a `(`: a call name, an expression result, or a
/// plain grouping/tuple/keyword paren.
pub(super) enum Callee {
    /// `name(…)`/`path::name(…)`/`recv.name(…)` with the name's start.
    Call(String, usize),
    /// `expr(…)` — a call on a computed callee.
    Expr,
    /// A grouping, tuple, or keyword paren.
    Group,
}

/// The callee ending before the `(` at `open`, with its start —
/// tolerates turbofish `::<T>` between name and paren.
pub(super) fn callee_before(masked: &[u8], open: usize) -> Callee {
    let mut e = open;
    while e > 0 && masked[e - 1].is_ascii_whitespace() {
        e -= 1;
    }
    if e == 0 {
        return Callee::Group;
    }
    match masked[e - 1] {
        b')' | b']' | b'"' | b'}' | b'\'' => Callee::Expr,
        b'>' => {
            // Turbofish `::<…>` — walk back over the balanced `<>`.
            let mut depth = 0i32;
            let mut j = e;
            while j > 0 {
                match masked[j - 1] {
                    b'>' => depth += 1,
                    b'<' => {
                        depth -= 1;
                        if depth == 0 {
                            j -= 1;
                            break;
                        }
                    }
                    _ => {}
                }
                j -= 1;
            }
            if j == 0 {
                return Callee::Expr;
            }
            callee_ident(masked, j - 1)
        }
        c if c.is_ascii_alphanumeric() || c == b'_' || c == b'!' => callee_ident(masked, e),
        _ => Callee::Group,
    }
}

fn callee_ident(masked: &[u8], end: usize) -> Callee {
    let mut s = end;
    while s > 0 {
        let b = masked[s - 1];
        if b.is_ascii_alphanumeric() || b == b'_' || b == b':' || b == b'.' || b == b'!' {
            s -= 1;
        } else {
            break;
        }
    }
    let name = as_str(masked, s..end).to_string();
    let first = name.split([':', '.', '!']).next().unwrap_or("");
    if [
        "if",
        "while",
        "match",
        "for",
        "return",
        "let",
        "in",
        "else",
        "loop",
        "fn",
        "use",
        "mod",
        "impl",
        "trait",
        "struct",
        "enum",
        "type",
        "const",
        "static",
        "pub",
        "where",
        "as",
        "move",
        "async",
        "unsafe",
        "do",
        "await",
        "yield",
        "break",
        "continue",
        "union",
        "extern",
        "dyn",
        "ref",
        "mut",
        "box",
        "try",
        "macro_rules",
    ]
    .contains(&first)
        && !name.ends_with('!')
    {
        // A keyword before `(` is a construct head, not a call —
        // `name!` macro spellings stay calls.
        return Callee::Group;
    }
    Callee::Call(name, s)
}

/// Whether `b` can end an operand — used to tell a binary `&`/`*`
/// (`x & "lit"`) from a prefix (`&"lit"`).
pub(super) fn is_operand_end(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || b == b'_'
        || matches!(
            b,
            b')' | b']' | b'}' | b'"' | b'\'' | b'?' | b'!' | b'.' | b'>'
        )
}

/// Constructors whose produced value is the argument whole —
/// `Some(…)`, `Ok(…)`, `String::from(…)` and friends.
fn transparent_ctor(name: &str) -> bool {
    let leaf = name.rsplit([':', '.']).next().unwrap_or(name);
    if matches!(leaf, "Some" | "Ok" | "Err") {
        return true;
    }
    matches!(
        name,
        "String::from"
            | "str::to_string"
            | "String::leak"
            | "Box::new"
            | "Box::leak"
            | "Arc::new"
            | "Rc::new"
            | "Cell::new"
            | "RefCell::new"
            | "Mutex::new"
            | "RwLock::new"
            | "Cow::Borrowed"
            | "Cow::Owned"
    )
}

/// Method calls whose argument is a produced value alternative —
/// `unwrap_or(alt)`, `or_else(|| alt)`, `then(|| alt)`, `map(f)`, … —
/// so a literal at that argument inherits the call expression's slot.
fn fallback_combinator(method: &str) -> bool {
    [
        "unwrap_or",
        "unwrap_or_else",
        "or",
        "or_else",
        "ok_or",
        "ok_or_else",
        "then",
        "then_some",
        "map_or",
        "map_or_else",
        "map",
        "map_err",
        "and",
        "and_then",
        "filter_map",
        "flat_map",
        "ok",
    ]
    .contains(&method)
}

/// Method suffixes the *receiver* flows through — `.to_string()`,
/// `.unwrap()`, `.expect(msg)`, `?`, and the fallback combinators
/// (`x.unwrap_or(alt)` produces either its receiver or `alt`) — so
/// `lit.suffix(…)` keeps the literal's evidence position.
fn transparent_suffix_end(masked: &[u8], e: usize) -> Option<usize> {
    let i = next_nonws(masked, e)?;
    if masked[i] == b'?' {
        return Some(i + 1);
    }
    if masked[i] != b'.' || masked.get(i + 1) == Some(&b'.') {
        return None;
    }
    let mut j = i + 1;
    while j < masked.len() && (masked[j].is_ascii_alphanumeric() || masked[j] == b'_') {
        j += 1;
    }
    let method = as_str(masked, i + 1..j);
    if ![
        "to_string",
        "to_owned",
        "into",
        "clone",
        "as_str",
        "as_mut_str",
        "trim",
        "trim_matches",
        "unwrap",
        "ok",
        "expect",
    ]
    .contains(&method)
        && !fallback_combinator(method)
    {
        return None;
    }
    let mut k = j;
    while k < masked.len() && masked[k].is_ascii_whitespace() {
        k += 1;
    }
    if masked.get(k) != Some(&b'(') {
        return None;
    }
    let close = match_bracket(masked, k)?;
    Some(close + 1)
}

/// The start of the primary expression ending at `pos` — walks back
/// over `ident`, `::`/`.` paths, call/index brackets, literals, `?`/`!`
/// suffixes, and turbofish `::<…>` — the receiver of a method call.
fn expr_start_before(masked: &[u8], pos: usize) -> usize {
    let mut i = pos;
    loop {
        while i > 0 && masked[i - 1].is_ascii_whitespace() {
            i -= 1;
        }
        if i == 0 {
            return i;
        }
        match masked[i - 1] {
            b')' | b']' | b'}' => {
                // Walk back to the matching open.
                let mut depth = 0i32;
                let mut j = i;
                while j > 0 {
                    match masked[j - 1] {
                        b')' | b']' | b'}' => depth += 1,
                        b'(' | b'[' | b'{' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j -= 1;
                }
                i = j.saturating_sub(1);
            }
            b'"' | b'\'' => {
                // A masked literal `"   "`/`' '` — walk back over its
                // opener and an optional `r`/`b`/`c` prefix.
                let q = masked[i - 1];
                let mut j = i - 1;
                while j > 0 && masked[j - 1] != q {
                    j -= 1;
                }
                i = j.saturating_sub(1);
                while i > 0 && matches!(masked[i - 1], b'r' | b'b' | b'c' | b'#') {
                    i -= 1;
                }
            }
            c if c.is_ascii_alphanumeric() || c == b'_' => {
                while i > 0 && (masked[i - 1].is_ascii_alphanumeric() || masked[i - 1] == b'_') {
                    i -= 1;
                }
                // The receiver chain continues only through `.`/`::`;
                // a bare keyword/ident boundary ends it (`return x.y`).
                let mut j = i;
                while j > 0 && masked[j - 1].is_ascii_whitespace() {
                    j -= 1;
                }
                if j > 0
                    && (masked[j - 1] == b'.'
                        || (masked[j - 1] == b':' && masked.get(j - 2) == Some(&b':')))
                {
                    continue;
                }
                return i;
            }
            b'.' | b':' | b'?' | b'!' => {
                i -= 1;
            }
            b'>' => {
                if masked.get(i - 2) == Some(&b'-') {
                    return i; // `->` ends the receiver — a return type
                }
                let mut depth = 0i32;
                let mut j = i;
                while j > 0 {
                    match masked[j - 1] {
                        b'>' => depth += 1,
                        b'<' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j -= 1;
                }
                i = j;
            }
            _ => return i,
        }
    }
}

/// What the `{` at `open` is — how a literal at its tail flows.
pub(super) enum BlockRole {
    /// `=> { … }` — a match-arm block.
    Arm,
    /// `if c {`/`else {`/`else if c {` — an if arm.
    IfArm,
    /// `|…| {` — a closure body block.
    Closure,
    /// A block in expression position (`let x = { … }`, `unsafe {`) —
    /// the block itself is the produced value.
    ExprBlock,
    /// A `fn` body — its tail is the fn's return value.
    FnBody,
    /// A body whose tail is discarded — `for`/`while`/`impl`/`mod`/
    /// `match`/`trait`/item bodies.
    Dead,
}

pub(super) fn block_role(masked: &[u8], open: usize) -> BlockRole {
    let (hs, he) = stmt_head(masked, open);
    let head = as_str(masked, hs..he).trim().to_string();
    block_role_inner(&head)
}

fn block_role_inner(head: &str) -> BlockRole {
    if head.ends_with("=>") {
        return BlockRole::Arm;
    }
    if head.ends_with('|') {
        return BlockRole::Closure;
    }
    // The last construct keyword at top-level bracket depth decides:
    // `kind: if c {` and `let x = if a {} else` heads read `if`/`else`
    // the same as statement-level arms, while a `match`/`fn`/`else if`
    // inside an earlier item's balanced body stays invisible.
    match last_construct_kw(head) {
        Some(("if" | "else", _)) => BlockRole::IfArm,
        Some(("fn", _)) => BlockRole::FnBody,
        Some(_) => BlockRole::Dead,
        // `unsafe {}`, `async {}`, `const {}`, `kind: {}`, bare
        // `= { … }` blocks produce their tail.
        None => BlockRole::ExprBlock,
    }
}

/// The `{` of the match body enclosing `at` (skipping `(`/`[` that may
/// hold the arm pattern), plus the `match` keyword's start.
fn match_enclosing(masked: &[u8], mut at: usize) -> Option<(usize, usize, usize)> {
    for _ in 0..32 {
        let open = enclosing_open(masked, at)?;
        if masked[open] == b'{' && is_match_body(masked, open) {
            let close = match_bracket(masked, open)?;
            let (hs, he) = stmt_head(masked, open);
            let head = as_str(masked, hs..he);
            // The `match` keyword is the last depth-0 construct word —
            // a `match` inside the scrutinee's `()` args is invisible.
            let (_, moff) = last_construct_kw(head)?;
            return Some((hs + moff, open, close));
        }
        at = open;
    }
    None
}

/// The span of the whole `match`/`if`/`closure`/block construct whose
/// produced value is `s..e`, when `s..e` is an arm body, an arm-block
/// tail, a closure body, or an expr-block's tail.
fn enclosing_construct(masked: &[u8], s: usize, e: usize) -> Option<(usize, usize)> {
    // `=> <expr>` — a match-arm body.
    if let Some(a) = prev_nonws(masked, s)
        && masked[a] == b'>'
        && a > 0
        && masked[a - 1] == b'='
        && let Some((mstart, _, close)) = match_enclosing(masked, a - 1)
    {
        return Some((mstart, close + 1));
    }
    // A `|`/`||` closure body — `|x| <expr>` or `|| <expr>`: the literal
    // is the whole body when the next token ends the expression.
    if let Some(a) = prev_nonws(masked, s)
        && masked[a] == b'|'
        && let Some(next) = next_nonws(masked, e)
        && matches!(masked[next], b')' | b',' | b']' | b'}' | b';')
    {
        let opens = if a > 0 && masked[a - 1] == b'|' {
            Some(a - 1)
        } else {
            // Walk back over the params for the opening `|`, skipping
            // balanced `()`/`[]`: params are idents, `,`, `:`, `&`,
            // `mut`, tuple patterns. A literal delimiter, `=`, `;`, or
            // an unbalanced opener cannot sit inside a param list —
            // `"a" | "b"` or-patterns and `x | "lit"` or-exprs stop
            // here and stay operands.
            let mut depth = 0i32;
            let mut j = a;
            loop {
                if j == 0 {
                    break None;
                }
                match masked[j - 1] {
                    b')' | b']' => {
                        depth += 1;
                        j -= 1;
                    }
                    b'(' | b'[' => {
                        if depth == 0 {
                            break None;
                        }
                        depth -= 1;
                        j -= 1;
                    }
                    b'|' if depth == 0 => break Some(j - 1),
                    b'"' | b'\'' | b'=' | b';' | b'{' | b'}' if depth == 0 => break None,
                    _ => j -= 1,
                }
            }
        };
        if let Some(open_pipe) = opens {
            let mut start = open_pipe;
            // `move |x|`, `async move |`, `static move |`.
            for _ in 0..3 {
                let (ws, we) = word_before(masked, start);
                if matches!(
                    as_str(masked, ws..we),
                    "move" | "async" | "static" | "unsafe" | "do" | "try"
                ) {
                    start = ws;
                } else {
                    break;
                }
            }
            return Some((start, e));
        }
        return None;
    }
    // A `{` block's tail — `s..e` is the last expression of its block,
    // i.e. the block's own `{`, a `;`, or a `}` stands before it. A
    // literal inside the tail's call args (`{ f("lit") }`) is not the
    // produced value and keeps its own slot.
    let next = next_nonws(masked, e)?;
    if masked[next] != b'}' {
        return None;
    }
    let mut at = s;
    let open = loop {
        let o = enclosing_open(masked, at)?;
        if masked[o] == b'{' && match_bracket(masked, o) == Some(next) {
            break o;
        }
        at = o;
    };
    let prev = prev_nonws(masked, s)?;
    if !(prev == open || masked[prev] == b';' || masked[prev] == b'}') {
        return None;
    }
    match block_role(masked, open) {
        BlockRole::Arm => {
            let (hs, he) = stmt_head(masked, open);
            let head_text = as_str(masked, hs..he);
            let arrow = head_text.rfind("=>").map(|n| hs + n)?;
            let (mstart, _, close) = match_enclosing(masked, arrow)?;
            Some((mstart, close + 1))
        }
        BlockRole::IfArm => {
            // The chain runs from the head's chain-start `if` to the
            // last `else`/`else if` block's close — `kind: if c {`,
            // `let x = if a {} else {}`, and heads holding earlier
            // complete `if` statements all resolve alike.
            let (hs, he) = stmt_head(masked, open);
            let head = as_str(masked, hs..he);
            let if_at = if_chain_start(head)?;
            let mut end = next;
            loop {
                let mut j = end + 1;
                while j < masked.len() && masked[j].is_ascii_whitespace() {
                    j += 1;
                }
                if masked[j..].starts_with(b"else")
                    && masked
                        .get(j + 4)
                        .is_none_or(|b| !(b.is_ascii_alphanumeric() || *b == b'_'))
                {
                    let mut k = j + 4;
                    while k < masked.len() && masked[k].is_ascii_whitespace() {
                        k += 1;
                    }
                    while k < masked.len() && masked[k] != b'{' && masked[k] != b';' {
                        k += 1;
                    }
                    if masked.get(k) == Some(&b'{')
                        && let Some(c) = match_bracket(masked, k)
                    {
                        end = c;
                        continue;
                    }
                }
                break;
            }
            Some((hs + if_at, end + 1))
        }
        BlockRole::Closure => {
            let (hs, he) = stmt_head(masked, open);
            let head = as_str(masked, hs..he);
            let pipe = head.rfind('|').map(|n| hs + n)?;
            let mut start = pipe;
            let mut j = pipe;
            while j > hs && j > 0 {
                j -= 1;
                if masked[j] == b'|' {
                    start = j;
                    break;
                }
            }
            for _ in 0..3 {
                let (ws, we) = word_before(masked, start);
                if matches!(
                    as_str(masked, ws..we),
                    "move" | "async" | "static" | "unsafe" | "do" | "try"
                ) {
                    start = ws;
                } else {
                    break;
                }
            }
            Some((start, next + 1))
        }
        BlockRole::ExprBlock => Some((open, next + 1)),
        BlockRole::Dead | BlockRole::FnBody => None,
    }
}

/// If `s..e` is the argument of a value-transparent call, the whole
/// call's span — `Some(…)`, `x.unwrap_or(…)`, `Option::unwrap_or(o, …)`
/// (last arg).
fn enclosing_transparent_call(masked: &[u8], s: usize, e: usize) -> Option<(usize, usize)> {
    let open = enclosing_open(masked, s)?;
    if masked[open] != b'(' {
        return None;
    }
    let close = match_bracket(masked, open)?;
    if e > close {
        return None;
    }
    let Callee::Call(name, name_start) = callee_before(masked, open) else {
        return None;
    };
    let leaf = name.rsplit([':', '.']).next().unwrap_or(&name);
    if transparent_ctor(&name) {
        return Some((name_start, close + 1));
    }
    if name.ends_with('!') {
        return None; // a macro — its args are data/templates
    }
    if fallback_combinator(leaf) {
        // `.method(` — the receiver is the expr before the `.` before
        // `leaf`; qualified `Option::unwrap_or(` treats its last arg.
        if let Some(dot) = name.rfind('.')
            && name[dot + 1..] == *leaf
        {
            let recv_start = expr_start_before(masked, name_start + dot);
            return Some((recv_start, close + 1));
        }
        let args = call_args(masked, open);
        if let Some(&(as_, ae)) = args.last()
            && s >= as_
            && e <= ae
        {
            return Some((name_start, close + 1));
        }
    }
    None
}

/// Walk `s..e` outward through value-transparent structure: prefix
/// `&`/`*`, transparent method suffixes, transparent constructors,
/// fallback-combinator arguments, `=>`/if-arm/closure/block produced
/// values, and grouping parens. Returns the expanded span and whether
/// the walk crossed a value-producing combinator (used to keep a
/// statement-level `x.unwrap_or("lit");` from screaming — the produced
/// value is discarded, so the arg is data).
pub(super) fn expand_transparent(masked: &[u8], s0: usize, e0: usize) -> (usize, usize, bool) {
    let (mut s, mut e, mut crossed) = (s0, e0, false);
    for _ in 0..48 {
        let start = (s, e);
        // `&`/`*` prefix — a reference/deref of the same value.
        if let Some(p) = prev_nonws(masked, s)
            && matches!(masked[p], b'&' | b'*')
            && (masked[p] != b'&' || masked.get(p + 1) != Some(&b'&'))
            && prev_nonws(masked, p).is_none_or(|q| !is_operand_end(masked[q]))
        {
            s = p;
        }
        // `.to_string()`/`?`/combinator suffixes.
        if let Some(pe) = transparent_suffix_end(masked, e) {
            e = pe;
        }
        // Transparent ctor or combinator arg.
        if let Some((cs, ce)) = enclosing_transparent_call(masked, s, e) {
            s = cs;
            e = ce;
            crossed = true;
        }
        // Produced-value bodies: `=>`, `|…|`, block tails.
        if let Some((cs, ce)) = enclosing_construct(masked, s, e) {
            s = cs;
            e = ce;
        }
        // A single-expression grouping `(x)`.
        if let Some(open) = enclosing_open(masked, s)
            && masked[open] == b'('
            && let Some(close) = match_bracket(masked, open)
            && e <= close
            && matches!(callee_before(masked, open), Callee::Group)
        {
            let args = call_args(masked, open);
            if args.len() == 1 {
                s = open;
                e = close + 1;
            }
        }
        if (s, e) == start {
            break;
        }
    }
    (s, e, crossed)
}
