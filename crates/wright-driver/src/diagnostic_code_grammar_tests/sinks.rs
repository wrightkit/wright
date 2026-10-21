//! Sink collection for `super::driver_codes::driver_code_census`: which
//! `fn`/closure names take a string-typed `code` (or analyzer `id`)
//! parameter, and which string-returning `fn <name>code(` bodies the
//! arm/return/tail scans cover.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::source::{as_str, call_args, match_bracket};

/// `fn` names whose parameter list carries a string-typed `code`
/// parameter, with that parameter's index (receiver excluded). Any
/// callsite's argument at that index is a code position.
pub(super) type CodeSinks = BTreeMap<String, usize>;

/// Sink names that collide with std collection/string methods. Their
/// callsites are ambiguous (`Vec::contains` vs `LintRegistry::contains`)
/// and only classify code-channel-shaped arguments.
pub(super) const AMBIGUOUS_SINKS: &[&str] = &[
    "contains",
    "contains_key",
    "insert",
    "get",
    "get_or",
    "get_or_insert",
    "or_insert",
    "find",
    "find_map",
    "position",
    "rposition",
    "push",
    "push_str",
    "push_front",
    "pop",
    "pop_front",
    "remove",
    "set",
    "entry",
    "key",
    "value",
    "index",
    "take",
    "iter",
    "len",
    "is_empty",
    "replace",
    "replace_range",
    "insert_str",
    "extend",
    "split",
    "split_at",
    "truncate",
    "clear",
    "drain",
    "retain",
    "fill",
    "append",
    "splice",
    "dedup",
    "sort",
    "swap",
    "swap_remove",
    "binary_search",
    "partition",
    "resize",
    "reserve",
    "count",
    "eq",
    "ne",
    "map",
    "filter",
    "any",
    "all",
    "nth",
    "last",
    "first",
    "min",
    "max",
    "sum",
    "product",
    "collect",
    "enumerate",
    "zip",
    "chain",
    "rev",
    "skip",
    "take_while",
    "skip_while",
    "map_while",
    "filter_map",
    "flat_map",
    "flatten",
    "fold",
    "reduce",
    "scan",
    "inspect",
    "copied",
    "cloned",
    "peekable",
    "fuse",
    "cycle",
    "step_by",
    "peek",
    "windows",
    "chunks",
    "next",
    "front",
    "back",
    "capacity",
    "shrink_to",
    "shrink_to_fit",
    "try_reserve",
    "try_reserve_exact",
    "write",
    "read",
    "concat",
    "join",
    "repeat",
];

/// Collect `fn` names whose parameter list carries a string-typed `code`
/// parameter (`code: &str`, `code: String`, `code: impl Into<String>`,
/// a generic `code: T` whose bound mentions `Into`/`AsRef`/`str`), with
/// the parameter's index (the receiver does not count). In
/// wright-analyzer a string-typed `id:` parameter is a rule-code
/// channel too. `let name = |code: &str, …|` closure bindings register
/// under their variable so `cb("lit")` callsites classify. Built from
/// the masked sources so `#[cfg(test)]` helpers never register.
pub(super) fn collect_code_sinks(files: &BTreeMap<PathBuf, (String, Vec<u8>)>) -> CodeSinks {
    let mut sinks = BTreeMap::new();
    for (path, (original, masked)) in files {
        let analyzer = path.to_string_lossy().contains("wright-analyzer");
        let text = as_str(masked, 0..masked.len()).to_string();
        for (at, _) in text.match_indices("fn ") {
            let name_start = at + 3;
            let name_end = text[name_start..]
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .map(|n| name_start + n);
            let Some(name_end) = name_end else {
                continue;
            };
            let name = original[name_start..name_end].to_string();
            // Capture generics `<'a, T: Into<String>>` and the `where`
            // clause — a bare-ident param type (`code: T`) is a string
            // channel when its bound mentions a string trait.
            let mut generics_end = name_end;
            let mut p = name_end;
            while p < masked.len() && masked[p].is_ascii_whitespace() {
                p += 1;
            }
            if masked.get(p) == Some(&b'<') {
                let mut depth = 0i32;
                let mut q = p;
                while q < masked.len() {
                    match masked[q] {
                        b'<' => depth += 1,
                        b'>' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    q += 1;
                }
                generics_end = q + 1;
                p = generics_end;
                while p < masked.len() && masked[p].is_ascii_whitespace() {
                    p += 1;
                }
            }
            if masked.get(p) != Some(&b'(') {
                continue;
            }
            let params = call_args(masked, p);
            let Some(params_close) = match_bracket(masked, p) else {
                continue;
            };
            // The `where` clause runs from `)` to the body's `{`/`;`.
            let mut wend = params_close + 1;
            while wend < masked.len()
                && masked[wend] != b'{'
                && masked[wend] != b';'
                && wend - params_close < 400
            {
                wend += 1;
            }
            let bounds = format!(
                "{}{}",
                &text[name_end..generics_end],
                &text[params_close + 1..wend]
            );
            // The receiver (`self`/`&self`/…) carries no `:` and does not
            // consume a callsite argument — index counts only real params.
            let mut index = 0usize;
            for (ps, pe) in params {
                let param = as_str(masked, ps..pe).trim();
                let Some((pname, ptype)) = param.split_once(':') else {
                    continue;
                };
                let pname = pname.trim().trim_start_matches("mut ").trim();
                let ptype = ptype.trim();
                let bare_ident = !ptype.is_empty()
                    && ptype.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                // `code: T` where `T`'s own bound (`<T: Into<String>>` /
                // `where T: AsRef<str>`) is string-shaped.
                let generic_string = bare_ident
                    && ptype.chars().next().is_some_and(char::is_uppercase)
                    && bounds.split([',', '>']).any(|bound| {
                        let bound = bound.trim_start();
                        (bound.starts_with(&format!("{ptype}:"))
                            || bound.starts_with(&format!("{ptype} :")))
                            && (bound.contains("Into")
                                || bound.contains("AsRef")
                                || bound.contains("Borrow")
                                || bound.contains("String")
                                || bound.contains("str"))
                    });
                let string_typed = ptype.contains("str")
                    || ptype.contains("String")
                    || ptype.contains("Into")
                    || ptype.contains("AsRef")
                    || ptype.contains("Borrow")
                    || ptype.contains("Cow")
                    || generic_string;
                if (pname == "code" || (analyzer && pname == "id")) && string_typed {
                    sinks.entry(name.clone()).or_insert(index);
                }
                index += 1;
            }
        }
        // `let name = |code: &str, …| …` — a closure binding with a
        // string-typed `code` parameter is a sink under its variable.
        for (at, _) in text.match_indices("let ") {
            if at > 0 && (masked[at - 1].is_ascii_alphanumeric() || masked[at - 1] == b'_') {
                continue;
            }
            let mut j = at + 4;
            while j < masked.len() && masked[j].is_ascii_whitespace() {
                j += 1;
            }
            let id_start = j;
            while j < masked.len() && (masked[j].is_ascii_alphanumeric() || masked[j] == b'_') {
                j += 1;
            }
            if j == id_start {
                continue;
            }
            let var = original[id_start..j].to_string();
            // Skip `: type` and ws to the `=`; then optional `move` to `|`.
            while j < masked.len() && masked[j] != b'=' && masked[j] != b';' && masked[j] != b'{' {
                j += 1;
            }
            if masked.get(j) != Some(&b'=') || masked.get(j + 1) == Some(&b'=') {
                continue;
            }
            j += 1;
            while j < masked.len() && masked[j].is_ascii_whitespace() {
                j += 1;
            }
            if masked[j..].starts_with(b"move") {
                let after_move = j + 4;
                if masked
                    .get(after_move)
                    .is_none_or(|b| !(b.is_ascii_alphanumeric() || *b == b'_'))
                {
                    j = after_move;
                    while j < masked.len() && masked[j].is_ascii_whitespace() {
                        j += 1;
                    }
                }
            }
            if masked.get(j) != Some(&b'|') {
                continue;
            }
            let Some(pipe_end) = masked[j + 1..]
                .iter()
                .position(|b| *b == b'|')
                .map(|q| j + 1 + q)
            else {
                continue;
            };
            let params_text = as_str(masked, j + 1..pipe_end);
            for (index, param) in params_text.split(',').enumerate() {
                if let Some((pname, ptype)) = param.split_once(':') {
                    let pname = pname.trim().trim_start_matches("mut ").trim();
                    let ptype = ptype.trim();
                    let string_typed =
                        ptype.contains("str") || ptype.contains("String") || ptype.contains("Into");
                    if (pname == "code" || (analyzer && pname == "id")) && string_typed {
                        sinks.entry(var.clone()).or_insert(index);
                    }
                }
            }
        }
    }
    sinks
}

/// Names of string-returning `fn <name>code(` — the fns whose `=>` arms
/// the census already scans. An outermost call to one in a code position
/// produces a code through that scanned body.
pub(super) fn code_fns(
    files: &BTreeMap<PathBuf, (String, Vec<u8>)>,
) -> std::collections::BTreeSet<String> {
    let mut fns = std::collections::BTreeSet::new();
    for (original, masked) in files.values() {
        let text = as_str(masked, 0..masked.len()).to_string();
        for (at, _) in text.match_indices("fn ") {
            let name_start = at + 3;
            let Some(name_end) = text[name_start..]
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .map(|n| name_start + n)
            else {
                continue;
            };
            let name = &original[name_start..name_end];
            if !name.ends_with("code") {
                continue;
            }
            let Some(brace) = text[name_end..].find('{').map(|n| name_end + n) else {
                continue;
            };
            let returns = text[name_end..brace].rsplit("->").next().unwrap_or("");
            if returns.contains("str") || returns.contains("String") {
                fns.insert(name.to_string());
            }
        }
    }
    fns
}
