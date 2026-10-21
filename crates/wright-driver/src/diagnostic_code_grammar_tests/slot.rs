//! Slot evaluation for the literal classifier: given an expanded
//! expression's span, decide which evidence position it occupies —
//! `code:`/key fields, binding/assignment LHS, sink arguments,
//! collection elements, for-iterables, statements, and returns.

use super::brackets::{
    depth0_words, enclosing_open, next_nonws, prev_nonws, stmt_head, word_before,
};
use super::classify::{FileCtx, Outcome, Scope};
use super::expand::{BlockRole, Callee, block_role, callee_before, is_operand_end};
use super::positions::{code_binding_name, is_code_field, mutates_string};
use super::sinks::AMBIGUOUS_SINKS;
use super::source::{as_str, call_args, enclosing_fn_span, match_bracket, stage_marker_at};

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
            b'+' | b'-' | b'*' | b'/' | b'%' | b'^' | b'&' | b'|' | b'<' | b'>' | b'!' | b'=' => {
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
            if is_code_field(&name, ctx.analyzer) {
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
        b'+' | b'-' | b'*' | b'/' | b'%' | b'^' | b'&' | b'!' | b'<' | b'>' | b'?' | b'~'
        | b'#' => Outcome::Other("operand"),
        b'"' | b'\'' | b')' | b']' | b'.' => Outcome::Unresolved("adjacent-token"),
        _ => {
            let (ws, we) = word_before(masked, p + 1);
            let word = as_str(masked, ws..we);
            match word {
                "return" | "break" | "yield" => return_tail(ctx, s),
                "in" => for_iterable(ctx, s),
                "if" | "while" | "match" | "for" | "else" | "loop" | "unsafe" | "async"
                | "move" | "do" | "await" | "assert" => Outcome::Other("keyword-operand"),
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
        // `let (a, b)`/`let T { .. }`/`let Some(x)`/`let _` destructure
        // — a literal on the RHS is not bound whole to one name.
        if rest.starts_with(['(', '[', '{']) {
            return Outcome::Other("destructure-binding");
        }
        let name_len = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        let name = &rest[..name_len];
        let after = rest[name_len..].trim_start();
        if name_len == 0 || name == "_" || after.starts_with(['(', '{', '<', '!']) {
            return Outcome::Other("destructure-binding");
        }
        if code_binding_name(name, ctx.analyzer) {
            if compound || name.split('_').any(|p| p == "PREFIX" || p == "SUFFIX") {
                return Outcome::Fragment("code-binding-fragment");
            }
            return Outcome::Vocab("code-binding");
        }
        let scope = if kw == "let" {
            enclosing_fn_span(masked, eq)
                .map(|(_, b, be)| Scope::Range(b, be))
                .unwrap_or(Scope::AllFiles)
        } else {
            Scope::AllFiles
        };
        return Outcome::Deferred {
            names: vec![name.to_string()],
            scope,
        };
    }
    let lhs = head.trim();
    let field = lhs.rsplit([' ', '\t']).next().unwrap_or("").trim();
    let field_leaf = field.rsplit('.').next().unwrap_or(field);
    if is_code_field(field_leaf, ctx.analyzer)
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
        if is_code_field(key, ctx.analyzer) {
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
/// caller, so it is a code when the fn's own name ends with the channel
/// noun — `provider_failure_code`, `fn code(&self) -> &str`. A `code`
/// segment elsewhere in the name cannot prove the channel either way —
/// `code_execution` modifies, `code_for` means "the code of" — except
/// `by_code`, the established entity channel (`by_code_field` emits the
/// `byCode=` record).
fn return_tail(ctx: &FileCtx, at: usize) -> Outcome {
    match enclosing_fn_span(ctx.masked, at) {
        Some((name, _, _))
            if (name == "code" || name.ends_with("_code")) && !name.contains("by_code") =>
        {
            Outcome::Vocab("code-fn-return")
        }
        Some((name, _, _))
            if !name.contains("by_code")
                && name.split('_').any(|s| s.eq_ignore_ascii_case("code")) =>
        {
            Outcome::Unresolved("ambiguous-code-fn")
        }
        Some(_) => Outcome::Other("fn-return"),
        None => Outcome::Unresolved("return-outside-fn"),
    }
}

/// The literal sits at a `,`/`(`/`[` boundary — an argument or a
/// collection element.
fn arg_or_element(ctx: &FileCtx, s: usize, e: usize) -> Outcome {
    let masked = ctx.masked;
    let Some(open) = enclosing_open(masked, s) else {
        return Outcome::Unresolved("no-bracket");
    };
    match masked[open] {
        b'(' => match callee_before(masked, open) {
            Callee::Call(name, callee_start) => call_arg(ctx, &name, callee_start, open, s),
            Callee::Expr => Outcome::Other("expr-callee-arg"),
            Callee::Group => {
                // `(x, Stage::Y)` — the tuple's first element is a code.
                if let Some(a) = next_nonws(masked, e)
                    && masked[a] == b','
                    && stage_marker_at(masked, a, ctx.stage_glob)
                {
                    return Outcome::Vocab("stage-tuple");
                }
                Outcome::Other("group-element")
            }
        },
        b'[' => {
            // `[` is a subscript key when it follows an expression.
            match prev_nonws(masked, open) {
                Some(p) if is_operand_end(masked[p]) => Outcome::Other("subscript-key"),
                _ => match for_head(masked, open) {
                    Some((names, body_open, body_close)) => {
                        if names.iter().any(|n| code_binding_name(n, ctx.analyzer)) {
                            Outcome::Vocab("for-code")
                        } else {
                            Outcome::Deferred {
                                names,
                                scope: Scope::Range(body_open, body_close),
                            }
                        }
                    }
                    None => Outcome::Other("array-element"),
                },
            }
        }
        b'{' => statement_slot(ctx, s, e, false),
        _ => Outcome::Unresolved("stray-bracket"),
    }
}

/// `call(args)` — the callee decides the argument's role.
fn call_arg(ctx: &FileCtx, callee: &str, callee_start: usize, open: usize, s: usize) -> Outcome {
    let masked = ctx.masked;
    let args = call_args(masked, open);
    let index = args
        .iter()
        .position(|&(a, b)| s >= a && s <= b)
        .unwrap_or(args.len());
    if callee.ends_with('!') {
        return Outcome::Other("macro-arg");
    }
    let leaf = callee.rsplit([':', '.']).next().unwrap_or(callee);
    // `.` method calls give the receiver — the span before the `.`
    // before the leaf; `::` paths and bare names have none.
    let receiver_span = callee
        .rfind('.')
        .filter(|dot| !callee[..*dot].ends_with(':'))
        .map(|dot| (callee_start, callee_start + dot));
    // Writes that take the code channel as a target:
    // `mem::replace(&mut d.code, "x")`, `ptr::write(&mut d.code, "x")` —
    // any arg after a code-channel arg0 is a code position.
    if index >= 1
        && [
            "replace",
            "write",
            "write_unaligned",
            "take",
            "swap",
            "read",
            "write_bytes",
            "drop_in_place",
            "copy",
            "copy_nonoverlapping",
            "clone_from",
        ]
        .contains(&leaf)
        && args
            .first()
            .is_some_and(|&(a, b)| arg_is_code_channel(ctx, a, b))
    {
        return Outcome::Vocab("code-write-arg");
    }
    // `insert("code", v)` — the literal value behind a code-named key.
    if matches!(leaf, "insert" | "insert_str" | "get_or_insert")
        && index >= 1
        && args.first().is_some_and(|&(a, b)| {
            lit_content(ctx, a, b).is_some_and(|k| is_code_field(&k, ctx.analyzer))
        })
    {
        return Outcome::Vocab("code-key-value");
    }
    if matches!(leaf, "or_insert" | "or_insert_with" | "get_or_insert_with")
        && receiver_span.is_some_and(|(rs, re)| receiver_has_code_entry(ctx, rs, re))
    {
        return Outcome::Vocab("code-entry-value");
    }
    // `code.push_str("x")`-family: the arg is a code fragment.
    if mutates_string(leaf)
        && receiver_span.is_some_and(|(rs, re)| {
            let r = as_str(ctx.masked, rs..re).trim();
            r == "code"
                || r.ends_with(".code")
                || r.ends_with("_code")
                || (ctx.analyzer && (r == "id" || r.ends_with(".id")))
        })
    {
        return Outcome::Fragment("code-append");
    }
    if let Some(indices) = ctx.sinks.get(leaf) {
        if indices.contains(&index) {
            return Outcome::Vocab("sink-arg");
        }
        return Outcome::Other("sink-other-arg");
    }
    if AMBIGUOUS_SINKS.contains(&leaf) {
        // A literal cannot prove a code channel for a name that collides
        // with a std method (`contains`, `insert`, `join`, …).
        return Outcome::Other("ambiguous-callee");
    }
    Outcome::Other("call-arg")
}

/// The cooked text of `masked[a..b]` when it is exactly one literal.
fn lit_content(ctx: &FileCtx, a: usize, b: usize) -> Option<String> {
    let text = ctx.original[a..b].trim();
    let text = text.strip_prefix('r').unwrap_or(text);
    let inner = text.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.to_string())
}

/// Whether the arg at `a..b` names a code channel — `&mut d.code`,
/// `code`, `d.provider_code`, analyzer `d.id`.
fn arg_is_code_channel(ctx: &FileCtx, a: usize, b: usize) -> bool {
    let text = as_str(ctx.masked, a..b).trim().to_string();
    let text = text
        .trim_start_matches('&')
        .trim_start_matches("mut ")
        .trim_start_matches('*')
        .trim();
    text == "code"
        || text.ends_with(".code")
        || text.ends_with("_code")
        || (ctx.analyzer && (text == "id" || text.ends_with(".id")))
        || {
            let last = text.rsplit('.').next().unwrap_or(text);
            is_code_field(last, ctx.analyzer)
        }
}

/// Whether the receiver span `rs..re` holds an `entry("code")`-family
/// call — read the original text since the mask blanks the literal.
fn receiver_has_code_entry(ctx: &FileCtx, rs: usize, re: usize) -> bool {
    let text = &ctx.original[rs..re];
    let Some(at) = text.find("entry(") else {
        return false;
    };
    let keys: &[&str] = if ctx.analyzer {
        &["code", "provider_code", "refusal_code", "id"]
    } else {
        &["code", "provider_code", "refusal_code"]
    };
    let arg = text[at + 6..].trim_start();
    keys.iter()
        .any(|k| arg.starts_with(&format!("\"{k}\"")) || arg.starts_with(&format!("r\"{k}\"")))
}

/// `s..e` at a statement boundary — a mid-block statement or a block
/// tail (which `expand_transparent` already handled, so what remains
/// is either the fn body's tail or a discarded one).
fn statement_slot(ctx: &FileCtx, s: usize, e: usize, crossed: bool) -> Outcome {
    let masked = ctx.masked;
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
