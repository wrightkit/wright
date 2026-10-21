//! Slot evaluation for the literal classifier: given an expanded
//! expression's span, decide which evidence position it occupies —
//! `code:`/key fields, binding/assignment LHS, sink arguments,
//! collection elements, for-iterables, statements, and returns.

use super::brackets::{
    depth0_words, enclosing_open, is_struct_lit_brace, next_nonws, prev_nonws, stmt_head,
    word_before,
};
use super::call::call_arg;
use super::classify::{FileCtx, Outcome, Scope, evaluate};
use super::expand::{BlockRole, Callee, block_role, callee_before, is_operand_end};
use super::positions::{code_binding_name, code_tail_field, is_code_field};
use super::source::{as_str, call_args, enclosing_fn_span, match_bracket};
use super::uses::{concat_span, element_is_stage};

/// The slot of the expanded expression `s..e` — the verdict that
/// decides the literal it came from.
pub(super) fn slot_of(ctx: &FileCtx, s: usize, e: usize, crossed: bool) -> Outcome {
    let masked = ctx.masked;
    // What follows the expanded expression: operands and receivers make
    // the literal a piece of a produced value — never the code itself.
    if let Some(a) = next_nonws(masked, e) {
        match masked[a] {
            b'=' if masked.get(a + 1) == Some(&b'>') => {
                return Outcome::Other("pattern-expr");
            }
            // `expr:` — a `name:` field key, an `x: T` annotation, a
            // `'label:` — the value names a slot or is a label, never
            // an emitted code by itself.
            b':' if masked.get(a + 1) != Some(&b':') && (a == 0 || masked[a - 1] != b':') => {
                return Outcome::Other("key");
            }
            // `"a" + "b"`/`lit + x` — the literal is a piece of the
            // `+` chain's produced string; the chain's slot decides.
            b'+' if masked.get(a + 1) != Some(&b'=') => return concat_piece(ctx, s, e),
            b'-' | b'*' | b'/' | b'%' | b'^' | b'&' | b'|' | b'<' | b'>' | b'!' | b'=' => {
                return Outcome::Other("operand");
            }
            b'.' => return Outcome::Other("receiver"),
            _ => {}
        }
    }
    let Some(p) = prev_nonws(masked, s) else {
        return Outcome::Unresolved("file-top");
    };
    match masked[p] {
        b':' if (p == 0 || masked[p - 1] != b':') && masked.get(p + 1) != Some(&b':') => {
            let name = name_before_colon(ctx, p);
            if is_code_field(&name, ctx.analyzer) || code_tail_field(&name) {
                Outcome::Vocab("code-field")
            } else {
                Outcome::Other("field")
            }
        }
        b'=' => {
            if masked.get(p + 1) == Some(&b'=')
                || matches!(
                    masked.get(p.wrapping_sub(1)),
                    Some(b'=') | Some(b'!') | Some(b'<') | Some(b'>')
                )
            {
                return Outcome::Other("comparison");
            }
            if masked.get(p + 1) == Some(&b'>') {
                return Outcome::Other("arm-pattern");
            }
            let compound = matches!(
                masked.get(p.wrapping_sub(1)),
                Some(b'+')
                    | Some(b'-')
                    | Some(b'*')
                    | Some(b'/')
                    | Some(b'%')
                    | Some(b'&')
                    | Some(b'|')
                    | Some(b'^')
            );
            lhs_of_assign(ctx, p, compound)
        }
        b',' | b'(' | b'[' => arg_or_element(ctx, s, e),
        b'{' | b';' | b'}' => statement_slot(ctx, s, e, crossed),
        b'|' => Outcome::Other("operand"),
        // `x + "lit"` — the literal is a piece of the `+` chain.
        b'+' => concat_piece(ctx, s, e),
        b'-' | b'*' | b'/' | b'%' | b'^' | b'&' | b'!' | b'<' | b'>' | b'?' | b'~' | b'#' => {
            Outcome::Other("operand")
        }
        b'"' | b'\'' | b')' | b']' | b'.' => Outcome::Unresolved("adjacent-token"),
        _ => {
            let (ws, we) = word_before(masked, p + 1);
            let word = as_str(masked, ws..we);
            match word {
                "return" | "break" | "yield" => return_tail(ctx, s),
                "in" => for_iterable(ctx, s),
                "if" | "while" | "match" | "for" | "else" | "loop" | "unsafe" | "async"
                | "move" | "do" | "await" | "assert" => Outcome::Other("keyword-operand"),
                // A declaration keyword heads a binding/type/item site —
                // `let name`, `let mut name`, `mod name`, `impl Name`,
                // `x as Name` — a name there is declared or referenced
                // by type, never a produced value.
                "let" | "const" | "static" | "mut" | "ref" | "fn" | "mod" | "struct" | "enum"
                | "union" | "trait" | "impl" | "type" | "use" | "extern" | "dyn" | "pub"
                | "crate" | "as" | "where" | "Self" | "super" | "self" => {
                    Outcome::Other("declaration")
                }
                _ => Outcome::Unresolved("keyword-before-literal"),
            }
        }
    }
}

/// The field/key name before a `:` — an identifier or a `"key"`
/// literal's cooked content.
fn name_before_colon(ctx: &FileCtx, colon: usize) -> String {
    let masked = ctx.masked;
    let Some(i) = prev_nonws(masked, colon) else {
        return String::new();
    };
    if masked[i] == b'"' {
        // A masked literal `"   "` — recover its content from the
        // original text.
        let mut j = i;
        while j > 0 && masked[j - 1] != b'"' {
            j -= 1;
        }
        if j == 0 {
            return String::new();
        }
        let open = j - 1;
        return ctx.original[open + 1..i].to_string();
    }
    let (ws, we) = word_before(masked, i + 1);
    as_str(masked, ws..we).to_string()
}

/// Whether `s..e` sits inside a `for … in …` iterable — and if so the
/// pattern's names and the loop body's range.
fn for_head(masked: &[u8], at: usize) -> Option<(Vec<String>, usize, usize)> {
    // `for <pat> in` — the `in` before `at`, `for` before that.
    let (hs, _) = stmt_head(masked, at);
    let head = as_str(masked, hs..at);
    let in_at = head.rfind(" in ")?;
    let for_at = head[..in_at].rfind("for")?;
    if for_at > 0 {
        let b = head.as_bytes()[for_at - 1];
        if b.is_ascii_alphanumeric() || b == b'_' {
            return None;
        }
    }
    let pat = &head[for_at + 3..in_at];
    let names: Vec<String> = pat
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| {
            !w.is_empty() && !["mut", "ref", "in", "for", "let", "Some", "Ok", "Err"].contains(w)
        })
        .map(|w| w.to_string())
        .collect();
    if names.is_empty() {
        return None;
    }
    // The loop body `{` at this level after `at`.
    let mut depth = 0i32;
    let mut i = at;
    while i < masked.len() {
        match masked[i] {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b'{' if depth == 0 => {
                let close = match_bracket(masked, i).unwrap_or(masked.len());
                return Some((names, i, close));
            }
            b';' | b'}' if depth == 0 => return None,
            _ => {}
        }
        i += 1;
    }
    None
}

/// `for <pat> in <expr>` — the iterable's literals are the values the
/// pattern binds; a code-named pattern makes them code candidates,
/// otherwise the pattern's uses inside the loop body decide.
fn for_iterable(ctx: &FileCtx, at: usize) -> Outcome {
    let Some((names, body_open, body_close)) = for_head(ctx.masked, at) else {
        return Outcome::Other("in-operand");
    };
    if names.iter().any(|n| code_binding_name(n, ctx.analyzer)) {
        return Outcome::Vocab("for-code");
    }
    Outcome::Deferred {
        names,
        scope: Scope::Range(body_open, body_close),
        piece: false,
    }
}

/// `name: <expr>` / `= <expr>` — the LHS of an assignment or binding.
fn lhs_of_assign(ctx: &FileCtx, eq: usize, compound: bool) -> Outcome {
    let masked = ctx.masked;
    let (hs, _) = stmt_head(masked, eq);
    let head = as_str(masked, hs..eq).to_string();
    // A `let`/`const`/`static` declaration names the binding with the
    // identifier after the keyword. The last one at top-level bracket
    // depth heads *this* binding — earlier items can share the head
    // when the last `;` is far away (`impl`/`enum`/`fn` bodies close
    // with `}` and no `;`), but their own declarations sit inside
    // balanced brackets and stay invisible to the depth-0 scan, while
    // visibility/qualifier words (`pub`, `if`/`while` for let-chains)
    // never reach the keyword position last.
    if let Some(&(kat, kw)) = depth0_words(&head)
        .iter()
        .rev()
        .find(|&(_, w)| ["let", "const", "static"].contains(w))
    {
        let rest = head[kat + kw.len()..].trim_start();
        let rest = rest.trim_start_matches("mut ").trim_start_matches("ref ");
        // `let (a, b)`/`let [a]`/`let T { .. }`/`let _` destructure —
        // the whole-RHS literal cannot be a destructured value. `let
        // W(x)`/`if let Some(code)`/`let Ok(code) … else` ctor
        // patterns DO bind: the literal expanded here sat inside
        // `W(…)`, so a single pattern leaf names its channel.
        if rest.starts_with(['(', '[', '{']) {
            return Outcome::Other("destructure-binding");
        }
        let name_len = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        let name = &rest[..name_len];
        let after = rest[name_len..].trim_start();
        if name_len == 0 || name == "_" || after.starts_with(['{', '<', '!']) {
            return Outcome::Other("destructure-binding");
        }
        if after.starts_with('(') || after.starts_with("::") {
            // The pattern's `(` is the first `(` after the `let`
            // keyword — `W(leaf, …)`/`m::W(leaf, …)` binds the RHS
            // `W(…)`'s args per-index; the whole-RHS literal (expanded
            // out of a transparent `Some`/`Ok`/`Err`) binds only a
            // single leaf.
            let pat_at = hs + kat + kw.len();
            let Some(paren) = (pat_at..eq).find(|&i| masked[i] == b'(') else {
                return Outcome::Other("destructure-binding");
            };
            let names = pattern_leaves(masked, paren);
            let mut bare = names.iter().flatten();
            match (bare.next(), names.iter().filter(|n| n.is_some()).count()) {
                (Some(only), 1) => return bind_name(ctx, only, compound, kw == "let", eq),
                _ => return Outcome::Other("destructure-binding"),
            }
        }
        return bind_name(ctx, name, compound, kw == "let", eq);
    }
    let lhs = head.trim();
    let field = lhs.rsplit([' ', '\t']).next().unwrap_or("").trim();
    let field_leaf = field.rsplit('.').next().unwrap_or(field);
    if is_code_field(field_leaf, ctx.analyzer)
        || code_tail_field(field_leaf)
        || (field.ends_with("_code") && !field.ends_with("by_code") && !field.contains('('))
        || field == "code"
        || (ctx.analyzer && field == "id")
    {
        return if compound {
            Outcome::Fragment("code-append")
        } else {
            Outcome::Vocab("code-field-write")
        };
    }
    if let Some(bracketed) = lhs.strip_suffix(']') {
        // `v["code"] =` — the subscript key decides.
        let open = bracketed.rfind('[').unwrap_or(0);
        let key = bracketed[open + 1..].trim().trim_matches('"');
        if is_code_field(key, ctx.analyzer) || code_tail_field(key) {
            return Outcome::Vocab("code-subscript-write");
        }
        return Outcome::Other("subscript-write");
    }
    if code_binding_name(lhs.trim_start_matches(['&', '*']), ctx.analyzer) {
        return if compound {
            Outcome::Fragment("code-append")
        } else {
            Outcome::Vocab("code-write")
        };
    }
    Outcome::Other("assign-other")
}

/// `return`/`break`/`yield` and fn-body tails: the value flows to the
/// caller, so it is a code when the fn's own name carries a `code`
/// segment — `code`, `provider_failure_code`, `code_for_kind` — with
/// `by_code` excluded as the established entity channel
/// (`by_code_field` emits the `byCode=` record). Every other fn defers
/// the literal under the fn's *name*: a callsite that reaches a code
/// position (`deny(tag())`) proves the literal is a code, an
/// unclassifiable callsite fails it as unresolved, and a fn never
/// called at a code position resolves non-code.
fn return_tail(ctx: &FileCtx, at: usize) -> Outcome {
    match enclosing_fn_span(ctx.masked, at) {
        Some((name, _, _))
            if !name.contains("by_code")
                && name.split('_').any(|s| s.eq_ignore_ascii_case("code")) =>
        {
            Outcome::Vocab("code-fn-return")
        }
        Some((name, _, _)) => Outcome::Deferred {
            names: vec![name],
            scope: Scope::AllFiles,
            piece: false,
        },
        None => Outcome::Unresolved("return-outside-fn"),
    }
}

/// A name bound by `let`/`const`/`static`/pattern — the binding's own
/// verdict: a code-channel name is `Vocab` (a `PREFIX`/`SUFFIX` name or
/// a `compound` `op=` write makes it a piece), any other name defers
/// under it for use-resolution.
fn bind_name(ctx: &FileCtx, name: &str, compound: bool, local: bool, at: usize) -> Outcome {
    if code_binding_name(name, ctx.analyzer) {
        if compound || name.split('_').any(|p| p == "PREFIX" || p == "SUFFIX") {
            return Outcome::Fragment("code-binding-fragment");
        }
        return Outcome::Vocab("code-binding");
    }
    let scope = if local {
        enclosing_fn_span(ctx.masked, at)
            .map(|(_, b, be)| Scope::Range(b, be))
            .unwrap_or(Scope::AllFiles)
    } else {
        Scope::AllFiles
    };
    Outcome::Deferred {
        names: vec![name.to_string()],
        scope,
        piece: false,
    }
}

/// The bindable leaf names of a `(`/`[` pattern at `open` — bare
/// identifiers only; `_`, nested patterns, and typed elements are
/// `None` (no single name binds that element).
fn pattern_leaves(masked: &[u8], open: usize) -> Vec<Option<String>> {
    call_args(masked, open)
        .iter()
        .map(|&(a, b)| {
            let el = as_str(masked, a..b)
                .trim()
                .trim_start_matches("ref ")
                .trim_start_matches("mut ")
                .trim_start_matches('&')
                .trim_start_matches("mut ")
                .trim();
            (!el.is_empty()
                && el != "_"
                && el.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
            .then(|| el.to_string())
        })
        .collect()
}

/// The `let`/`if let`/`while let` pattern the `=`-RHS at `open`
/// destructures into — `None` when `open` does not open a let-binding
/// RHS expression.
enum LetPattern {
    /// `let (a, b) = (…)` — the RHS tuple element binds the pattern
    /// name at its own index.
    Tuple(Vec<Option<String>>),
    /// `let [a, b] = […]` — the RHS array element binds per-index.
    Array(Vec<Option<String>>),
    /// `let W(a, …) = W(…)` — the RHS ctor call's arg binds the leaf
    /// at the same index; the stored string is the ctor path's leaf.
    Ctor(String, Vec<Option<String>>),
}

fn let_pattern(masked: &[u8], open: usize) -> Option<LetPattern> {
    // `= <rhs>` — a binding `=`, not `==`/`=>`/`<=`/`!=`.
    let eq = prev_nonws(masked, open)?;
    if masked[eq] != b'='
        || masked.get(eq + 1) == Some(&b'=')
        || matches!(
            masked.get(eq.wrapping_sub(1)),
            Some(b'=') | Some(b'!') | Some(b'<') | Some(b'>')
        )
    {
        return None;
    }
    let (hs, he) = stmt_head(masked, eq);
    let head = as_str(masked, hs..he);
    // The last depth-0 `let` heads this binding — `if let`/`while let`
    // included.
    let &(kat, _) = depth0_words(head)
        .iter()
        .rev()
        .find(|&(_, w)| *w == "let")?;
    let mut j = hs + kat + "let".len();
    while j < eq && masked[j].is_ascii_whitespace() {
        j += 1;
    }
    for _ in 0..2 {
        for kw in ["ref ", "mut "] {
            if masked[j..].starts_with(kw.as_bytes()) {
                j += kw.len();
                while j < eq && masked[j].is_ascii_whitespace() {
                    j += 1;
                }
            }
        }
    }
    match *masked.get(j)? {
        b'(' => Some(LetPattern::Tuple(pattern_leaves(masked, j))),
        b'[' => Some(LetPattern::Array(pattern_leaves(masked, j))),
        b'{' => None,
        c if c.is_ascii_alphanumeric() || c == b'_' => {
            let id_start = j;
            while j < eq
                && (masked[j].is_ascii_alphanumeric() || masked[j] == b'_' || masked[j] == b':')
            {
                j += 1;
            }
            while j < eq && masked[j].is_ascii_whitespace() {
                j += 1;
            }
            if masked.get(j) == Some(&b'(') {
                let ctor = as_str(masked, id_start..j)
                    .trim_end()
                    .rsplit("::")
                    .next()
                    .unwrap_or("")
                    .to_string();
                return Some(LetPattern::Ctor(ctor, pattern_leaves(masked, j)));
            }
            // A bare-ident or struct pattern — not a bracketed RHS
            // destructure.
            None
        }
        _ => None,
    }
}

/// The verdict for a pattern-bound element: a code-channel name is
/// `Vocab`, a plain name defers under it, and `_`/nested/typed pattern
/// elements bind nothing the census can follow.
fn bind_pattern_leaf(ctx: &FileCtx, name: Option<&String>, at: usize) -> Outcome {
    match name {
        Some(n) if code_binding_name(n, ctx.analyzer) => Outcome::Vocab("pattern-code-binding"),
        Some(n) => Outcome::Deferred {
            names: vec![n.clone()],
            scope: enclosing_fn_span(ctx.masked, at)
                .map(|(_, b, be)| Scope::Range(b, be))
                .unwrap_or(Scope::AllFiles),
            piece: false,
        },
        None => Outcome::Other("pattern-element"),
    }
}

/// The literal sits at a `,`/`(`/`[` boundary — an argument or a
/// collection element.
fn arg_or_element(ctx: &FileCtx, s: usize, e: usize) -> Outcome {
    let masked = ctx.masked;
    let Some(open) = enclosing_open(masked, s) else {
        return Outcome::Unresolved("no-bracket");
    };
    let index_of = |open: usize| {
        call_args(masked, open)
            .iter()
            .position(|&(a, b)| s >= a && s <= b)
    };
    match masked[open] {
        b'(' => match callee_before(masked, open) {
            Callee::Call(name, callee_start) => {
                // `let W(code) = W(lit)` — a ctor pattern's RHS call
                // binds arg `i` to the pattern leaf at index `i`.
                if let Some(LetPattern::Ctor(ctor, names)) = let_pattern(masked, open)
                    && ctor
                        == name
                            .trim_end_matches(':')
                            .rsplit([':', '.'])
                            .next()
                            .unwrap_or("")
                {
                    return bind_pattern_leaf(
                        ctx,
                        index_of(open)
                            .and_then(|i| names.get(i))
                            .and_then(Option::as_ref),
                        open,
                    );
                }
                call_arg(ctx, &name, callee_start, open, s)
            }
            Callee::Expr => Outcome::Other("expr-callee-arg"),
            Callee::Group => {
                let args = call_args(masked, open);
                let index = index_of(open);
                // `(code, …, Stage::X)` — the stage marker can sit in
                // any later element; element 0 is the code, other
                // elements ride along as data.
                if let Some(i) = index
                    && args.iter().enumerate().any(|(j, &(a, b))| {
                        j != i && element_is_stage(masked, a, b, ctx.stage_glob)
                    })
                {
                    return if i == 0 {
                        Outcome::Vocab("stage-tuple")
                    } else {
                        Outcome::Other("stage-tuple-field")
                    };
                }
                // `let (a, b) = (x, y)` — the RHS element binds the
                // pattern name at its own index: a code-ish name is a
                // code channel, any other name defers under it so a
                // later `deny(code)` resolves the literal.
                if let Some(LetPattern::Tuple(names)) = let_pattern(masked, open)
                    && let Some(i) = index
                {
                    return bind_pattern_leaf(ctx, names.get(i).and_then(Option::as_ref), open);
                }
                Outcome::Other("group-element")
            }
        },
        b'[' => {
            // `[` is a subscript key when it follows an expression.
            match prev_nonws(masked, open) {
                Some(p) if is_operand_end(masked[p]) => Outcome::Other("subscript-key"),
                _ => {
                    // `let [code] = ["x"]` — per-index pattern binding.
                    if let Some(LetPattern::Array(names)) = let_pattern(masked, open) {
                        return bind_pattern_leaf(
                            ctx,
                            index_of(open)
                                .and_then(|i| names.get(i))
                                .and_then(Option::as_ref),
                            open,
                        );
                    }
                    // `[a, b].join(sep)`/`[a, b].concat()` — the
                    // elements are pieces of the produced string; the
                    // produced expression's own slot decides them.
                    if let Some(produced_end) = produced_join_end(masked, open) {
                        return piece_eval(ctx, open, produced_end);
                    }
                    match for_head(masked, open) {
                        Some((names, body_open, body_close)) => {
                            if names.iter().any(|n| code_binding_name(n, ctx.analyzer)) {
                                Outcome::Vocab("for-code")
                            } else {
                                Outcome::Deferred {
                                    names,
                                    scope: Scope::Range(body_open, body_close),
                                    piece: false,
                                }
                            }
                        }
                        None => Outcome::Other("array-element"),
                    }
                }
            }
        }
        b'{' => statement_slot(ctx, s, e, false),
        _ => Outcome::Unresolved("stray-bracket"),
    }
}

/// The literal is a *piece* of a produced string (`+` operand, a
/// `.join()`/`.concat()` element) — the produced expression `s..e`
/// decides; a code position makes the literal a `Fragment`, and a
/// bound name defers carrying the piece flag.
fn piece_eval(ctx: &FileCtx, s: usize, e: usize) -> Outcome {
    match evaluate(ctx, s, e) {
        Outcome::Vocab(_) | Outcome::Fragment(_) => Outcome::Fragment("code-piece"),
        Outcome::Deferred { names, scope, .. } => Outcome::Deferred {
            names,
            scope,
            piece: true,
        },
        other => other,
    }
}

/// `s..e` is an operand of a binary `+` — the whole `+` chain produces
/// a string; evaluate the chain's slot as a piece context.
fn concat_piece(ctx: &FileCtx, s: usize, e: usize) -> Outcome {
    let (cs, ce) = concat_span(ctx.masked, s, e);
    piece_eval(ctx, cs, ce)
}

/// If the `]` closing the `[` at `open` is followed by `.join(…)` or
/// `.concat()`, the produced expression's end — `call_end` inclusive
/// of further transparent call chains is not walked; the immediate
/// `join`/`concat` call end is the produced span the piece evaluator
/// expands outward from.
fn produced_join_end(masked: &[u8], open: usize) -> Option<usize> {
    let close = match_bracket(masked, open)?;
    let mut i = next_nonws(masked, close + 1)?;
    if masked[i] != b'.' {
        return None;
    }
    i += 1;
    let mstart = i;
    while i < masked.len() && (masked[i].is_ascii_alphanumeric() || masked[i] == b'_') {
        i += 1;
    }
    if !matches!(as_str(masked, mstart..i), "join" | "concat") {
        return None;
    }
    while i < masked.len() && masked[i].is_ascii_whitespace() {
        i += 1;
    }
    if masked.get(i) != Some(&b'(') {
        return None;
    }
    Some(match_bracket(masked, i).map(|c| c + 1).unwrap_or(i + 1))
}

/// `s..e` at a statement boundary — a mid-block statement or a block
/// tail (which `expand_transparent` already handled, so what remains
/// is either the fn body's tail or a discarded one).
fn statement_slot(ctx: &FileCtx, s: usize, e: usize, crossed: bool) -> Outcome {
    let masked = ctx.masked;
    // A `{` that opens a struct literal is not a block — `S { name }`/
    // `S { name, }` is a shorthand field init whose *field* name decides
    // the channel (`S { code }` is a code, `S { msg }` is data).
    if let Some(open) = enclosing_open(masked, s)
        && masked[open] == b'{'
        && is_struct_lit_brace(masked, open)
        && let field = as_str(masked, s..e).trim()
    {
        if is_code_field(field, ctx.analyzer) {
            return Outcome::Vocab("struct-code-field");
        }
        return Outcome::Other("struct-field");
    }
    if let Some(a) = next_nonws(masked, e)
        && masked[a] == b'}'
        && let Some(open) = enclosing_open(masked, s)
        && masked[open] == b'{'
        && match_bracket(masked, open) == Some(a)
    {
        match block_role(masked, open) {
            BlockRole::FnBody => return return_tail(ctx, s),
            BlockRole::Dead => return Outcome::Other("discarded-tail"),
            // Arm/if/closure/expr-block tails are transparent — reaching
            // here means the construct was deeper than the expander
            // walked; report loudly rather than guess.
            BlockRole::Arm | BlockRole::IfArm | BlockRole::Closure | BlockRole::ExprBlock => {
                return Outcome::Unresolved("unexpanded-tail");
            }
        }
    }
    if crossed {
        // `x.unwrap_or("lit");` — the produced value is dropped; the
        // literal is a side-effect operand.
        return Outcome::Other("combinator-sideeffect");
    }
    Outcome::Unresolved("dead-literal")
}
