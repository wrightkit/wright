//! Binding resolution and mutation classification for
//! `super::driver_codes::driver_code_census`: what a bare `code`/`id`
//! value binds inside its enclosing `fn`, and what `.method(...)` calls
//! on a code channel produce.

use super::classify::{Sites, classify_rhs, is_code_channel, is_entity_id};
use super::patterns::{
    arm_pattern_binds, closure_param_binds, for_pattern_binds, match_arms_stage_ok,
};
use super::source::{
    as_str, call_args, enclosing_fn, enclosing_fn_body, line_of, rhs_span, skeleton, stage_marker,
};

/// Resolve what a bare `code`/`id` value (shorthand field, ctor argument,
/// or `code:`/`id:` field) binds inside the enclosing fn. Every `let`
/// binding and bare `name =` rebind in the fn body is classified — a
/// shadowing inner `let` can not hide behind an outer parameter. With no
/// binding in the body, a same-named parameter passes through a value
/// whose producer is covered at the caller, a match-arm pattern binding
/// defers to the `, Stage::`/arm scans, and anything else is
/// unclassified.
pub(super) fn resolve_code_binding(sites: &mut Sites<'_>, at: usize, name: &str) {
    let unclassified_at = |sites: &mut Sites<'_>, why: &str| {
        sites.unclassified.push(format!(
            "{}:{}: `{name}` value {why}",
            sites.file.display(),
            line_of(sites.original, at)
        ));
    };
    let Some((body, body_end)) = enclosing_fn_body(sites.masked, at) else {
        unclassified_at(sites, "outside a fn");
        return;
    };
    let text = as_str(sites.masked, 0..sites.masked.len());
    let mut bound = false;
    for (let_at, _) in text[body..body_end].match_indices("let ") {
        // `let ` at a word boundary — `outlet `/`tablet ` can not fake it.
        if let_at > 0 {
            let prev = sites.masked[body + let_at - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                continue;
            }
        }
        let stmt = body + let_at + "let ".len();
        // The `let` pattern runs to a top-level `=` or `;`.
        let mut depth = 0i32;
        let mut pend = stmt;
        let mut eq = None;
        for j in stmt..body_end {
            match sites.masked[j] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' if depth > 0 => depth -= 1,
                b'=' if depth == 0 && sites.masked.get(j + 1) != Some(&b'=') => {
                    pend = j;
                    eq = Some(j);
                    break;
                }
                b';' if depth == 0 => {
                    pend = j;
                    break;
                }
                _ => {}
            }
        }
        let pattern = text[stmt..pend].trim_end_matches(|c: char| c.is_ascii_whitespace());
        let Some(eq) = eq else {
            continue;
        };
        // Find where the pattern binds the name (word boundary). `name:`
        // inside a `{…}` destructure is the FIELD name (`{ code: c }`
        // binds `c`), not a binding of `name`; at the pattern's top
        // level it is a type annotation (`let code: String = …`).
        let Some(name_at) = pattern.match_indices(name).map(|(i, _)| i).find(|&i| {
            let before = pattern[..i].chars().last();
            let after = pattern[i + name.len()..].chars().next();
            let depth_in_pattern = pattern[..i].chars().fold(0i32, |d, c| match c {
                '(' | '[' | '{' => d + 1,
                ')' | ']' | '}' => d - 1,
                _ => d,
            });
            !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                && !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                && !(depth_in_pattern > 0
                    && pattern[i + name.len()..].trim_start().starts_with(':'))
        }) else {
            continue;
        };
        bound = true;
        // `name` inside `{…}`/`(…)` in the pattern destructures an
        // already-produced value — classify the RHS unless it is a match
        // feeding the `, Stage::` arm scan, in which case every arm must
        // carry a Stage marker.
        let shallow = !pattern[..name_at].trim_end().contains(['{', '(']);
        // `if let`/`while let` — the scrutinee ends at the block's `{`
        // (struct literals are banned in scrutinee position), not at a
        // `;` the block never has.
        let cond_let = {
            let mut w = body + let_at;
            while w > 0 && sites.masked[w - 1].is_ascii_whitespace() {
                w -= 1;
            }
            let mut ws = w;
            while ws > 0
                && (sites.masked[ws - 1].is_ascii_alphanumeric() || sites.masked[ws - 1] == b'_')
            {
                ws -= 1;
            }
            matches!(&text[ws..w], "if" | "while")
        };
        let (rstart, rend) = if cond_let {
            let mut d = 0i32;
            let mut end = body_end;
            for (j, &b) in sites.masked.iter().enumerate().take(body_end).skip(eq + 1) {
                match b {
                    b'(' | b'[' => d += 1,
                    b')' | b']' => d -= 1,
                    b'{' if d == 0 => {
                        end = j;
                        break;
                    }
                    b'{' => d += 1,
                    b'}' if d > 0 => d -= 1,
                    b';' if d == 0 => {
                        end = j;
                        break;
                    }
                    _ => {}
                }
            }
            let mut start = eq + 1;
            while start < end && sites.masked[start].is_ascii_whitespace() {
                start += 1;
            }
            while end > start && sites.masked[end - 1].is_ascii_whitespace() {
                end -= 1;
            }
            (start, end)
        } else {
            rhs_span(sites.masked, eq, body_end)
        };
        let skel = skeleton(&sites.original[rstart..rend]);
        // `id` is overloaded in the analyzer: entity ids (`SymbolId`,
        // indexes, numeric casts) are not diagnostic codes.
        if name == "id" && is_entity_id(&skel) {
            continue;
        }
        if shallow {
            // A `name`-to-`name` binding traces to an earlier binding this
            // scan already classifies. A numeric RHS is a safe segment
            // for non-`code` names (`format!("x-{}", n)`).
            if skel != name && !(name != "code" && skel.chars().all(|c| c.is_ascii_digit())) {
                classify_rhs(sites, rstart, rend);
            }
        } else {
            // Destructuring binds a value produced elsewhere:
            // - a `match` RHS feeds the `, Stage::` arm scan — every arm
            //   must carry the marker;
            // - a tuple RHS with `, Stage::` is covered by the tuple scan;
            // - a `Some(`/enum/tuple constructor RHS produces the value
            //   right here — classify it so `Some("bad code")` is
            //   censused;
            // - a `.code`/`.id`-family field read defers to that field's
            //   scanned write site; for `id`, a bare ident/field read
            //   (`RuleDefinition { id } = def`) likewise defers to the
            //   struct's `id:` site;
            // - anything else — a call, method, or opaque ident — can not
            //   be traced to a scanned write site: fail closed.
            let rhs_text = &text[eq + 1..rend];
            if rhs_text.contains("match") {
                if !match_arms_stage_ok(rhs_text, sites.stage_glob) {
                    unclassified_at(sites, "bound by a match arm the Stage:: scan misses");
                }
            } else if !stage_marker(rhs_text.as_bytes(), sites.stage_glob) {
                if skel.starts_with("Some(")
                    || skel.starts_with("Ok(")
                    || skel.starts_with("Err(")
                    || skel.starts_with('(')
                    || skel.starts_with('"')
                {
                    classify_rhs(sites, rstart, rend);
                } else if skel.ends_with(".code")
                    || skel.ends_with(".id")
                    || skel.ends_with(".provider_code")
                    || skel.ends_with(".refusal_code")
                    || (name == "id" && ident_or_field(&skel))
                    || (ident_or_field(&skel)
                        && (skel.contains('.') || skel.contains('[') || skel.contains(':')))
                {
                    // Produced at that field/path/index read's own
                    // scanned write site. A bare opaque ident
                    // (`let (code, _) = helper()`-adjacent) can not be
                    // traced — fail closed below.
                } else {
                    unclassified_at(
                        sites,
                        "is destructured from a value the census can not trace",
                    );
                }
            }
        }
    }
    // Whether `name` is bound as a code channel in this fn: a `let`
    // binding (classified above), a string-typed parameter (`code: i64`
    // in a JSON-RPC helper is not a code channel), a match-arm pattern
    // binding, or a `|…|` closure parameter.
    let param = enclosing_fn(sites.masked, at).and_then(|(_, params)| {
        params
            .iter()
            .find(|(p, _)| p == name)
            .map(|(_, t)| t.clone())
    });
    let param_is_string = param.as_deref().is_some_and(|t| {
        t.contains("str")
            || t.contains("String")
            || t.contains("Into")
            || t.contains("AsRef")
            || t.contains("Borrow")
            || t.contains("Cow")
            // A bare ident parameter type (`code: T`) is a generic whose
            // bound the sink collection already verified — `code: T` on
            // `fn f<T: Into<String>>` is a code channel.
            || t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && t.chars().next().is_some_and(|c| c.is_ascii_uppercase())
    });
    let bound_elsewhere = arm_pattern_binds(sites.masked, at, name)
        || closure_param_binds(sites.masked, at, name)
        || for_pattern_binds(sites.masked, at, name);
    // Mutations only matter on a code channel — a non-`code` binding
    // resolved as a passthrough base (`diagnostic.map(|d| d.code)`)
    // holds a container, not a code; its `.map`/`first` calls are not
    // code positions.
    let code_channel =
        is_code_channel(sites, name) && (bound || param_is_string || bound_elsewhere);

    // Mutating a code binding produces codes: `code = …`, `code += …`,
    // `code.push_str(…)`, `code.push(…)`, `code.insert(…)`, `code.extend(…)`,
    // `code.replace_range(…)` — every operand is a code position. A
    // `name` inside a `let` pattern is a binding site, not a mutation.
    if code_channel {
        for (name_at, _) in text[body..body_end].match_indices(name) {
            let a = body + name_at;
            if a > 0
                && (sites.masked[a - 1].is_ascii_alphanumeric()
                    || sites.masked[a - 1] == b'_'
                    || sites.masked[a - 1] == b'.')
            {
                continue;
            }
            // Inside a `let` pattern? The pattern runs to a top-level
            // `=`/`;`; anything before that binds rather than mutates.
            if let Some(lt) = text[body..a].rfind("let ") {
                let lt = body + lt;
                if lt > 0
                    && (sites.masked[lt - 1].is_ascii_alphanumeric()
                        || sites.masked[lt - 1] == b'_')
                {
                    // `outlet `/`tablet ` — not a real `let `.
                } else {
                    let mut d = 0i32;
                    let mut in_pattern = true;
                    for j in lt + 4..a {
                        match sites.masked[j] {
                            b'(' | b'[' | b'{' => d += 1,
                            b')' | b']' | b'}' => d -= 1,
                            b'=' | b';' if d == 0 => {
                                in_pattern = false;
                                break;
                            }
                            _ => {}
                        }
                    }
                    if in_pattern {
                        continue;
                    }
                }
            }
            let mut i = a + name.len();
            while i < body_end && sites.masked[i].is_ascii_whitespace() {
                i += 1;
            }
            match masked_get(sites.masked, i) {
                Some(b'=')
                    if masked_get(sites.masked, i + 1) != Some(b'=')
                        && masked_get(sites.masked, i + 1) != Some(b'>') =>
                {
                    let (rstart, rend) = rhs_span(sites.masked, i, body_end);
                    let skel = skeleton(&sites.original[rstart..rend]);
                    if plausibly_string(&skel) && !(name == "id" && is_entity_id(&skel)) {
                        classify_rhs(sites, rstart, rend);
                    }
                }
                Some(b'+') if masked_get(sites.masked, i + 1) == Some(b'=') => {
                    let (rstart, rend) = rhs_span(sites.masked, i + 1, body_end);
                    let skel = skeleton(&sites.original[rstart..rend]);
                    if plausibly_string(&skel) && !(name == "id" && is_entity_id(&skel)) {
                        classify_rhs(sites, rstart, rend);
                    }
                }
                Some(b'.') => classify_code_method(sites, i + 1),
                _ => {}
            }
        }
    }
    if bound || param.is_some() || bound_elsewhere {
        return;
    }
    unclassified_at(sites, &format!("has no `{name}` binding in its fn"));
}

/// Whether a callsite argument can possibly be a `&str`/`String`/
/// `Into<String>` value — literals, idents, paths, `format!`, method
/// calls, `&`-references all can; `vec![…]`, `json!(…)`, `[…]`, `{…}`,
/// and numbers cannot. Used to disambiguate same-named fns whose `code`
/// slot is not a string channel, and to skip non-string operands of
/// `code` mutations.
pub(super) fn plausibly_string(shape: &str) -> bool {
    !shape.starts_with("vec!")
        && !shape.starts_with("json!")
        && !shape.starts_with('\'')
        && !shape.starts_with('[')
        && !shape.starts_with('{')
        && !shape
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit() || (c == '-' && shape.len() > 1))
}

/// Whether a skeleton is a bare ident or a field/index read (`x`,
/// `x.y`, `self.x`, `x[..]`, `&x`) — a value produced at its own write
/// site, not an opaque call.
fn ident_or_field(skel: &str) -> bool {
    !skel.contains('(')
        && !skel.is_empty()
        && skel.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '[' | ']' | '"' | '&' | '*')
        })
}

fn masked_get(masked: &[u8], i: usize) -> Option<u8> {
    masked.get(i).copied()
}

/// `x.<method>(args)` on a code channel — `.code`/`.provider_code`/
/// `.refusal_code` field tails and bound `code`/`.._code` variables
/// share the same mutation semantics. `i` points just after the `.`
/// that starts the method name. Operand-taking mutators classify their
/// string args (`insert_str`/`clone_from` included); `push` validates a
/// char literal or classifies a string arg; non-string mutators fail
/// closed; reads pass; anything else fails closed.
pub(super) fn classify_code_method(sites: &mut Sites<'_>, i: usize) {
    let end = i + sites.masked[i..]
        .iter()
        .position(|b| !(b.is_ascii_alphanumeric() || *b == b'_'))
        .unwrap_or(sites.masked.len() - i);
    let name = as_str(sites.masked, i..end);
    if sites.masked.get(end) != Some(&b'(') {
        return; // a field read (`x.code.len`-less forms), not a call
    }
    let open = end;
    // `push('x')` carries a char literal — validate it against the
    // grammar charset; `push(expr)` classifies the expression.
    if name == "push" {
        for arg in call_args(sites.masked, open) {
            let arg_text = sites.original[arg.0..arg.1].trim();
            if let Some(inner) = arg_text
                .strip_prefix('\'')
                .and_then(|r| r.strip_suffix('\''))
            {
                let ok = inner.chars().count() == 1
                    && inner.chars().next().is_some_and(|c| {
                        c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-'
                    });
                if !ok {
                    sites.unclassified.push(format!(
                        "{}:{}: `push` writes a grammar-invalid char into a code",
                        sites.file.display(),
                        line_of(sites.original, i)
                    ));
                }
            } else if plausibly_string(&skeleton(arg_text)) {
                classify_rhs(sites, arg.0, arg.1);
            }
        }
        return;
    }
    for method in [
        "push_str",
        "insert",
        "insert_str",
        "extend",
        "replace_range",
        "clone_from",
    ] {
        if name == method {
            for arg in call_args(sites.masked, open) {
                if plausibly_string(&skeleton(&sites.original[arg.0..arg.1])) {
                    classify_rhs(sites, arg.0, arg.1);
                }
            }
            return;
        }
    }
    // Option/Result combinators pass or map the code through; their
    // args (`unwrap_or_else(|| "lit")`, `map(|c| …)`) are code
    // positions and classify (`|…|` peels in `classify_rhs`).
    for method in [
        "unwrap_or",
        "unwrap_or_else",
        "or_else",
        "or",
        "map",
        "map_or",
        "map_or_else",
        "map_err",
        "and_then",
        "then",
        "then_some",
        "then_some_or",
        "zip",
        "and",
        "filter_map",
        "as_deref",
        "flatten",
        "copied",
        "cloned",
        "transpose",
        "ok",
        "err",
        "unwrap",
        "unwrap_or_default",
        "unwrap_unchecked",
        "as_ref",
        "as_mut",
        "to_owned",
        "into",
        "as_str",
        "into_iter",
        "iter",
    ] {
        if name == method {
            for arg in call_args(sites.masked, open) {
                classify_rhs(sites, arg.0, arg.1);
            }
            return;
        }
    }
    // Mutators that produce an unverifiable code (`truncate`, `clear`,
    // case folds, drains) fail closed — the resulting string can not be
    // traced to a censused literal.
    for method in [
        "truncate",
        "clear",
        "pop",
        "drain",
        "retain",
        "swap",
        "split_off",
        "remove",
        "set_len",
        "write_fmt",
        "make_ascii_lowercase",
        "make_ascii_uppercase",
    ] {
        if name == method {
            sites.unclassified.push(format!(
                "{}:{}: `{method}` mutates a code outside the census",
                sites.file.display(),
                line_of(sites.original, i)
            ));
            return;
        }
    }
    // Reads and capacity ops do not produce codes.
    for method in [
        "len",
        "is_empty",
        "capacity",
        "to_string",
        "to_owned",
        "clone",
        "as_str",
        "as_mut_str",
        "as_bytes",
        "as_mut_vec",
        "chars",
        "bytes",
        "char_indices",
        "contains",
        "starts_with",
        "ends_with",
        "find",
        "rfind",
        "split",
        "rsplit",
        "splitn",
        "rsplitn",
        "split_once",
        "rsplit_once",
        "split_terminator",
        "rsplit_terminator",
        "split_whitespace",
        "split_ascii_whitespace",
        "split_inclusive",
        "lines",
        "trim",
        "trim_start",
        "trim_end",
        "trim_matches",
        "trim_start_matches",
        "trim_end_matches",
        "strip_prefix",
        "strip_suffix",
        "repeat",
        "replace",
        "replacen",
        "eq",
        "ne",
        "cmp",
        "partial_cmp",
        "iter",
        "iter_mut",
        "into_iter",
        "into_boxed_str",
        "into_bytes",
        "into_string",
        "into",
        "parse",
        "matches",
        "match_indices",
        "reserve",
        "reserve_exact",
        "try_reserve",
        "try_reserve_exact",
        "leak",
        "shrink_to_fit",
        "shrink_to",
        "encode_utf16",
        "escape_debug",
        "escape_default",
        "escape_unicode",
        "to_lowercase",
        "to_uppercase",
        "to_ascii_lowercase",
        "to_ascii_uppercase",
        "floor_char_boundary",
        "ceil_char_boundary",
        "get",
        "get_mut",
        "deref",
        "borrow",
        "borrow_mut",
        "fmt",
        "concat",
        "join",
        "dedup",
        "sort",
        "sort_unstable",
        "chunks",
        "windows",
        "first",
        "last",
        "as_ref",
        "as_mut",
    ] {
        if name == method {
            return;
        }
    }
    sites.unclassified.push(format!(
        "{}:{}: `.{name}` is an unclassified mutation of a code channel",
        sites.file.display(),
        line_of(sites.original, i)
    ));
}
