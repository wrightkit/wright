//! The classifiers behind `super::driver_codes::driver_code_census`: every code
//! position the scans surface resolves to a literal (censused), a
//! verified producer/passthrough, or an unclassified site that fails the
//! test — enumerate-or-fail, never silent.

use std::path::{Path, PathBuf};

use super::format::check_format_literal;
use super::patterns::arm_pattern_binds;
use super::resolve::resolve_code_binding;
use super::source::{as_str, line_of, match_bracket, skeleton};

/// One literal found in a code position, with its site for diagnostics.
pub(super) struct CodeSite {
    pub(super) file: PathBuf,
    pub(super) line: usize,
    pub(super) code: String,
}

/// The census accumulators and the file under scan, shared by the
/// code-position classifiers.
pub(super) struct Sites<'a> {
    pub(super) codes: &'a mut Vec<CodeSite>,
    pub(super) unclassified: &'a mut Vec<String>,
    pub(super) original: &'a str,
    pub(super) masked: &'a [u8],
    pub(super) file: &'a Path,
    /// `fn`s returning `&str`/`String` — outermost calls to them in a code
    /// position produce codes through their scanned bodies.
    pub(super) code_fns: &'a std::collections::BTreeSet<String>,
    /// Whether this file glob-imports `Stage` (`use …Stage::*`) so a
    /// bare `Frontend` can serve as a `, Stage::` tuple marker.
    pub(super) stage_glob: bool,
}

/// Whether a bare ident `shape` names a code channel: `code` itself or
/// a `.._code`-suffixed field/binding (`provider_code`,
/// `refusal_code`, `failure_code`, …). Preposition-prefixed names
/// (`by_code`, `per_code`, `for_code`, …) are "indexed by code"
/// containers, not codes. In wright-analyzer `id` is a rule-code
/// channel too; elsewhere `id` names entity ids and stays out.
pub(super) fn is_code_channel(sites: &Sites<'_>, shape: &str) -> bool {
    if !shape.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    if shape == "code" {
        return true;
    }
    const PREPOSITION_CODE: &[&str] = &[
        "by_code",
        "per_code",
        "for_code",
        "of_code",
        "in_code",
        "on_code",
        "to_code",
        "as_code",
        "at_code",
        "vs_code",
        "with_code",
        "via_code",
        "over_code",
        "under_code",
        "after_code",
        "before_code",
        "between_code",
        "during_code",
        "from_code",
        "into_code",
    ];
    if shape.ends_with("_code") && !PREPOSITION_CODE.contains(&shape) {
        return true;
    }
    shape == "id" && sites.file.to_string_lossy().contains("wright-analyzer")
}

/// A code position inside a `(expr, Stage::…)` tuple: a bare `code`/`id`
/// is read from the match arm's own pattern (passthrough); anything else
/// is classified like any code position.
pub(super) fn classify_tuple_code(sites: &mut Sites<'_>, start: usize, end: usize) {
    let shape = skeleton(&sites.original[start..end]);
    if is_code_channel(sites, &shape) && arm_pattern_binds(sites.masked, start, &shape) {
        return;
    }
    classify_rhs(sites, start, end);
}

/// A code position in a `fn <name>code` `=>` arm value: same arm-pattern
/// rule as tuple positions.
pub(super) fn classify_arm_value(sites: &mut Sites<'_>, arm_at: usize, start: usize, end: usize) {
    let shape = skeleton(&sites.original[start..end]);
    if is_code_channel(sites, &shape) && arm_pattern_binds(sites.masked, arm_at, &shape) {
        return;
    }
    classify_rhs(sites, start, end);
}

/// One top-level field segment of a struct literal. A segment that trims
/// to exactly a code name is a shorthand binding to resolve.
pub(super) fn check_shorthand_field(sites: &mut Sites<'_>, start: usize, end: usize, at: usize) {
    let field = as_str(sites.masked, start..end).trim();
    if is_code_channel(sites, field) {
        resolve_code_binding(sites, at, field);
    }
}

/// Extract the leading string literal of a code-position expression
/// (`"lit"`, `Some("lit")`, `"lit".to_string()`); returns whether one was
/// found and, when found, pushes it onto `codes`.
pub(super) fn push_literal(
    codes: &mut Vec<CodeSite>,
    original: &str,
    file: &Path,
    span: (usize, usize),
) -> bool {
    let text = original[span.0..span.1].trim();
    let inner = text
        .trim_start_matches(['&', '*'])
        .trim_start()
        .strip_prefix("Some(")
        .unwrap_or(text.trim_start_matches(['&', '*']).trim_start())
        .trim_start();
    let Some(rest) = inner.strip_prefix('"') else {
        return false;
    };
    let Some(end) = rest.find('"') else {
        return false;
    };
    codes.push(CodeSite {
        file: file.to_path_buf(),
        line: line_of(original, span.0),
        code: rest[..end].to_string(),
    });
    true
}

/// Args of `unwrap_or`/`or`/`map`/`and_then`-family combinators inside a
/// code-position expression carry codes too — fallbacks, mapped results,
/// closure bodies. Each arg is classified (`|…|` closures resolve their
/// body through `classify_rhs`'s `|`-peel).
fn classify_combinator_args(sites: &mut Sites<'_>, start: usize, end: usize) {
    let expr = &sites.original[start..end];
    for marker in [
        "unwrap_or(",
        "unwrap_or_else(",
        "or_else(",
        ".or(",
        ".map(",
        ".map_or(",
        ".map_or_else(",
        ".and_then(",
        ".then(",
        ".then_some(",
        ".then_some_or(",
        ".zip(",
        ".and(",
        ".filter_map(",
        ".flat_map(",
        ".map_while(",
        ".scan(",
        ".find_map(",
        ".chain(",
    ] {
        for (off, _) in expr.match_indices(marker) {
            let open = start + off + marker.len() - 1;
            let Some(close) = match_bracket(sites.masked, open) else {
                continue;
            };
            let mut depth = 0i32;
            let mut seg = open + 1;
            for i in open + 1..close {
                match sites.masked[i] {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => depth -= 1,
                    b',' if depth == 0 => {
                        classify_rhs(sites, seg, i);
                        seg = i + 1;
                    }
                    _ => {}
                }
            }
            classify_rhs(sites, seg, close);
        }
    }
}

/// Classify one code-position RHS: a literal enters the census; a bare
/// `code`/`id` (after peeling wrappers) resolves through the enclosing
/// fn's binding; `format!` literals are grammar-checked around their
/// holes; remaining shapes must match a verified producer or passthrough.
pub(super) fn classify_rhs(sites: &mut Sites<'_>, start: usize, end: usize) {
    if start >= end {
        return;
    }
    if push_literal(sites.codes, sites.original, sites.file, (start, end)) {
        return;
    }
    // `a + b` concatenation in a code position: every operand is a code
    // position of its own (`"ok".to_string() + &suffix()` classifies the
    // tail, not just the leading literal).
    {
        let mut depth = 0i32;
        let mut plus = Vec::new();
        for i in start..end {
            match sites.masked[i] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b'+' if depth == 0
                    && sites.masked.get(i + 1) != Some(&b'=')
                    && sites.masked.get(i + 1) != Some(&b'+')
                    && sites.masked.get(i.wrapping_sub(1)) != Some(&b'+') =>
                {
                    plus.push(i);
                }
                _ => {}
            }
        }
        if !plus.is_empty() {
            let mut seg = start;
            for i in plus {
                classify_plus_operand(sites, seg, i);
                seg = i + 1;
            }
            classify_plus_operand(sites, seg, end);
            return;
        }
    }
    classify_combinator_args(sites, start, end);
    let expr = &sites.original[start..end];
    // Peel wrappers — `Some(…)`, `.to_string()`/`.into()`/`.clone()`/
    // `.as_deref()`/`.as_str()`/`.unwrap_or_default()` suffixes, and
    // `unwrap_or*`/`or*`/`or_else` fallback tails — checking the peeled
    // shape after every step so `Some(code)` and `code.to_string()` both
    // reach binding resolution instead of a shape whitelist.
    let mut normalized: String = expr.chars().filter(|c| !c.is_whitespace()).collect();
    // Whether the most recent tail cut carried the code through its base
    // (`unwrap_or*`/`or*`/…) — or only gated it (`then`/`and`), or was a
    // transformer (`map`/`and_then`/…) whose arg decides: an arg that
    // returns the element (`|d| d`, `str::to_string`) means the base
    // carries the code, an arg that produces a field/literal
    // (`|d| d.code`, `|| "lit"`) makes the base a container.
    enum CutKind {
        None,
        Carries,
        Gate,
        Transformer(String),
    }
    let mut last_cut = CutKind::None;
    loop {
        while let Some(rest) = normalized
            .strip_prefix('&')
            .or_else(|| normalized.strip_prefix('*'))
        {
            normalized = rest.to_string();
        }
        // A trailing `,` is rustfmt's multi-line tail, not part of the
        // expression (`Some(expr,)`).
        if normalized.ends_with(',') {
            normalized.truncate(normalized.len() - 1);
            continue;
        }
        // `|…|` / `||` closure body — the produced value is inside.
        if normalized.starts_with('|')
            && let Some(pipe) = normalized[1..].find('|').map(|p| p + 2)
        {
            normalized = normalized[pipe..].to_string();
            continue;
        }
        // A literal surfaced by peeling (`|| "lit".to_string()` →
        // `"lit"`) — find it in the original span and census it.
        if normalized.starts_with('"')
            && let Some(rel) = sites.original[start..end].find('"')
            && push_literal(sites.codes, sites.original, sites.file, (start + rel, end))
        {
            return;
        }
        if normalized.starts_with("format!(") && normalized.ends_with(')') {
            check_format_literal(sites, start, end);
            return;
        }
        let shape = skeleton(&normalized);
        if is_code_channel(sites, &shape) {
            resolve_at(sites, start, end, &shape);
            return;
        }
        if producer(&normalized, sites.code_fns) || passthrough(&shape) {
            return;
        }
        if normalized.starts_with("Some(") && normalized.ends_with(')') {
            normalized = normalized["Some(".len()..normalized.len() - 1].to_string();
            continue;
        }
        let mut peeled = false;
        for suffix in [
            ".to_string()",
            ".into()",
            ".clone()",
            ".as_deref()",
            ".as_str()",
            ".unwrap_or_default()",
        ] {
            if let Some(rest) = normalized.strip_suffix(suffix) {
                normalized = rest.to_string();
                peeled = true;
                break;
            }
        }
        if !peeled && normalized.ends_with(')') {
            // Value-carrying cuts: the base flows into the result.
            for cut in [
                ".unwrap_or_else(",
                ".unwrap_or(",
                ".unwrap(",
                ".unwrap_unchecked(",
                ".unwrap_or_default(",
                ".or_else(",
                ".or(",
                ".map_err(",
                ".filter(",
                ".inspect(",
                ".flatten(",
                ".copied(",
                ".cloned(",
                ".transpose(",
                ".ok(",
                ".err(",
                ".expect(",
                ".take(",
                ".as_deref(",
                ".as_ref(",
                ".as_mut(",
                ".into_iter(",
                ".zip(",
                // Iterator adapters and consumers: the element flows
                // through (`x.iter().map(f).next()`'s `next`).
                ".iter(",
                ".iter_mut(",
                ".next(",
                ".nth(",
                // `.find(` stays unlisted: `str::find` yields an index
                // while `Iterator::find` yields the element — an
                // ambiguous cut must fail closed.
                ".first(",
                ".last(",
                ".get(",
                ".min(",
                ".max(",
                ".min_by(",
                ".max_by(",
                ".min_by_key(",
                ".max_by_key(",
                ".collect(",
                ".skip(",
                ".skip_while(",
                ".take_while(",
                ".chain(",
                ".enumerate(",
                ".rev(",
                ".cycle(",
                ".step_by(",
                ".peekable(",
                ".split(",
                ".splitn(",
                ".rsplit(",
                ".lines(",
                ".chars(",
                ".bytes(",
                ".join(",
                ".concat(",
            ] {
                if let Some(index) = normalized.rfind(cut) {
                    normalized.truncate(index);
                    last_cut = CutKind::Carries;
                    peeled = true;
                    break;
                }
            }
            // Gate cuts: the base is a `bool`/discriminant — the value
            // comes only from the args (already classified above).
            if !peeled {
                for cut in [
                    ".then(",
                    ".then_some(",
                    ".then_some_or(",
                    ".and(",
                    // Predicates and counts inspect the base; the
                    // result is `bool`/`usize`/`()`, never a code.
                    ".any(",
                    ".all(",
                    ".position(",
                    ".rposition(",
                    ".count(",
                    ".for_each(",
                    ".try_for_each(",
                ] {
                    if let Some(index) = normalized.rfind(cut) {
                        normalized.truncate(index);
                        last_cut = CutKind::Gate;
                        peeled = true;
                        break;
                    }
                }
            }
            // Transformer cuts: the arg decides whether the element
            // flows through — keep the removed tail for the check.
            if !peeled {
                for cut in [
                    ".map(",
                    ".and_then(",
                    ".filter_map(",
                    ".map_or(",
                    ".map_or_else(",
                    ".flat_map(",
                    ".map_while(",
                    ".scan(",
                    ".find_map(",
                ] {
                    if let Some(index) = normalized.rfind(cut) {
                        last_cut = CutKind::Transformer(normalized[index..].to_string());
                        normalized.truncate(index);
                        peeled = true;
                        break;
                    }
                }
            }
        }
        if !peeled {
            break;
        }
        if normalized
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            match &last_cut {
                // `bool.then(f)`/`opt.and(o)` — the base is a gate, not
                // a code position.
                CutKind::Gate => return,
                // `x.map(|d| d.code)` — the code flows through the
                // arg's field read; `x` is a container. `x.map(|d| d)`/
                // `x.map(str::to_string)` — the element flows out, so
                // `x` carries the code and resolves.
                CutKind::Transformer(tail) if !transformer_passes_element(tail) => return,
                _ => resolve_at(sites, start, end, &normalized),
            }
            return;
        }
    }
    // A bare non-code ident in a code position still traces its binding
    // (`id` stays out — it is overloaded and its channel checks live in
    // `is_code_channel`/`resolve_code_binding`'s own filters).
    if normalized
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
        && normalized
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && normalized != "id"
    {
        resolve_at(sites, start, end, &normalized);
        return;
    }
    sites.unclassified.push(format!(
        "{}:{}: {}",
        sites.file.display(),
        line_of(sites.original, start),
        expr.trim()
    ));
}

/// Resolve `name` at its own position — after peeling, `start` still
/// points at the whole expression (`&_x.map(|s| s.to_string())`), so a
/// `|s|` closure parameter that follows `start` stays invisible. The
/// last word-boundary occurrence of `name` inside the span is the use
/// being classified.
fn resolve_at(sites: &mut Sites<'_>, start: usize, end: usize, name: &str) {
    let span = &sites.original[start..end.min(sites.original.len())];
    let at = last_ident_pos(span, name)
        .map(|p| start + p)
        .unwrap_or(start);
    resolve_code_binding(sites, at, name);
}

/// The last occurrence of `name` in `text` at identifier boundaries
/// (`s.to_string` finds the `s`, not the `s` inside `to_string`).
fn last_ident_pos(text: &str, name: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = text.len();
    while let Some(off) = text[..i].rfind(name) {
        let before_ok =
            off == 0 || !bytes[off - 1].is_ascii_alphanumeric() && bytes[off - 1] != b'_';
        let after_ok = bytes
            .get(off + name.len())
            .is_none_or(|b| !b.is_ascii_alphanumeric() && *b != b'_');
        if before_ok && after_ok {
            return Some(off);
        }
        i = off;
    }
    None
}

/// Whether a transformer tail (`.map(args…)`, `.map_or(d, f)`) returns
/// its element: the LAST top-level arg — the transformer — peels to a
/// bare ident (`|d| d`, `|s| s.to_string()`) or a fn path
/// (`str::to_string`, `helper::make`). Anything else (`|d| d.code`,
/// `|| "lit"`, `|_| f()`) produces the value — the base is a container.
fn transformer_passes_element(tail: &str) -> bool {
    let Some(open) = tail.find('(') else {
        return false;
    };
    let args = &tail[open + 1..tail.len().saturating_sub(1)];
    // Last top-level `,`-separated arg is the transformer (`map_or(d,
    // f)`'s `f`); single-arg transformers split to themselves.
    let mut depth = 0i32;
    let mut seg = 0usize;
    for (i, c) in args.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => seg = i + 1,
            _ => {}
        }
    }
    let mut body = args[seg..].trim();
    // `|…|` closure body — the produced value is inside.
    if let Some(rest) = body.strip_prefix('|')
        && let Some(pipe) = rest.find('|')
    {
        body = rest[pipe + 1..].trim();
    }
    // Transparent wrappers around the element.
    loop {
        let mut step = false;
        for prefix in ["&", "*", "Some(", "Ok("] {
            if let Some(rest) = body.strip_prefix(prefix) {
                body = rest.strip_suffix(')').unwrap_or(rest).trim();
                step = true;
                break;
            }
        }
        if !step {
            for suffix in [
                ".to_string()",
                ".to_owned()",
                ".clone()",
                ".into()",
                ".as_str()",
                ".as_deref()",
                ".unwrap()",
            ] {
                if let Some(rest) = body.strip_suffix(suffix) {
                    body = rest.trim();
                    step = true;
                    break;
                }
            }
        }
        if !step {
            break;
        }
    }
    // A bare ident returns the element itself; a `path::name` is a fn
    // reference that transforms the element — both carry the base.
    !body.is_empty()
        && body
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
}

/// One `+`-concatenated operand: a literal is a fragment — its
/// characters must be grammar-valid but it is not a whole code and is
/// not censused; anything else classifies normally.
fn classify_plus_operand(sites: &mut Sites<'_>, start: usize, end: usize) {
    let text = sites.original[start..end]
        .trim()
        .trim_start_matches(['&', '*'])
        .trim_start();
    if let Some(rest) = text.strip_prefix('"')
        && let Some(close) = rest.find('"')
    {
        let literal = &rest[..close];
        if !literal
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
        {
            sites.unclassified.push(format!(
                "{}:{}: `+` operand literal produces a grammar-invalid code: {}",
                sites.file.display(),
                line_of(sites.original, start),
                literal
            ));
        }
        return;
    }
    classify_rhs(sites, start, end);
}

/// Verified code producers — the whole peeled expression must be the
/// call (`X::provider_failure_code(…)`, `diagnostic_code(…)`, or any
/// `fn` returning `&str`/`String` in the scanned crates) or an enumerated
/// constant/passthrough read. A producer nested inside a wrapper that
/// changes its output (e.g. inside `format!` args) never matches —
/// `format!` goes through `check_format_literal` instead.
pub(super) fn producer(normalized: &str, code_fns: &std::collections::BTreeSet<String>) -> bool {
    if normalized == "UNKNOWN_WITHIN" || normalized == "finding[\"code\"]" {
        return true;
    }
    // Outermost call `path::name(args)` — args may be anything; the call
    // must span the whole expression.
    let Some(open) = normalized.find('(') else {
        return false;
    };
    if !normalized.ends_with(')') {
        return false;
    }
    let callee = &normalized[..open];
    let qualifier_ok = callee
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':' || c == '!');
    if !qualifier_ok {
        return false;
    }
    let basename = callee.rsplit("::").next().unwrap_or(callee);
    basename == "provider_failure_code"
        || basename == "diagnostic_code"
        || code_fns.contains(basename)
}

/// Passthrough reads of already-enumerated or owner/provider-spelled
/// codes. Bare `code`/`id` never reach here — `classify_rhs` resolves
/// their bindings first.
pub(super) fn passthrough(shape: &str) -> bool {
    shape.ends_with(".code")
        || shape.ends_with(".code()")
        || shape.ends_with(".refusal_code()")
        || shape.ends_with(".provider_code")
        || shape.ends_with(".provider_code()")
        || shape.ends_with(".id")
        || shape.ends_with(".id()")
        || shape == "finding[\"\"]" // skeleton of `finding["code"]`
        // `None` produces no code — it flows out of `Option` code
        // channels the way `Some(x)`'s `x` flows in.
        || shape == "None"
        // `key(item).1` — a tuple-index read; the code was produced at
        // the tuple's own site (the `(expr, Stage::)` scan).
        || shape
            .rsplit('.')
            .next()
            .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
        // `path::name` — a fn reference (`x.map(str::to_string)`) or
        // associated value spelled at its own site. The last segment
        // must start lowercase so `Module::SCREAMING_CONST` can not
        // smuggle an unverified literal.
        || (shape.contains("::")
            && shape
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
            && shape
                .rsplit("::")
                .next()
                .is_some_and(|s| s.chars().next().is_some_and(|c| c.is_ascii_lowercase())))
}

/// Whether a skeleton is clearly a non-string entity id — `SymbolId`,
/// index/len arithmetic, `as usize`-style casts, or a number — as
/// opposed to a rule code. `id` is overloaded in wright-analyzer; these
/// shapes are never diagnostic codes.
pub(super) fn is_entity_id(shape: &str) -> bool {
    shape.starts_with("SymbolId::")
        || shape.contains("::from_index")
        || shape.ends_with(".index()")
        || shape.ends_with(".len()")
        || shape.contains("asusize")
        || shape.contains("asu32")
        || shape.contains("asu64")
        || shape.contains("asi64")
        || (!shape.is_empty() && shape.chars().all(|c| c.is_ascii_digit()))
}
