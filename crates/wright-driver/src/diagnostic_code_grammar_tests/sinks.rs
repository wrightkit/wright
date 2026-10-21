//! Sink collection for `super::driver_codes::driver_code_census`: which
//! `fn`/closure names take a string-typed `code` (or analyzer `id`)
//! parameter and at which *parameter* index, plus the direct
//! `use`/`let` aliases that forward to those names, the set of every
//! `fn`/closure/`struct`/`enum`/`trait`/`mod`/`type` name defined in
//! the scanned sources, each `fn`'s return-type leaf, and every
//! `const`/`static`'s declared-type leaf.
//!
//! Two facts the census relies on:
//!
//! - a parameter is a code channel when its *name* declares it (`code`,
//!   `.._code`, analyzer `id`) and its type can carry a string (`&str`,
//!   `String`, `impl Into<String>`, `impl AsRef<str>`, or a bare generic
//!   `T` bound — in the generics list or the `where` clause — that
//!   mentions a string trait);
//! - sink declarations carry *parameter* indices together with the
//!   receiver shape: a `&self` method declares `param` counting the
//!   receiver, so `recv.deny("x")` matches `args.len() + 1 == arity`
//!   and classifies arg `param - 1`, while `Recv::deny(r, "x")` and
//!   `Self::deny(self, "x")` pass arguments positionally
//!   (`args.len() == arity`). A fn with two code-named parameters
//!   registers both, and a name defined twice keeps a declaration per
//!   `impl`/module.
//!
//! The defined-name set backs the unknown-callee rule in `slot.rs`: an
//! argument of a bare callee the census cannot resolve to any `fn`,
//! closure, or type is unclassifiable and must fail, while the argument
//! of a defined fn is positively data — its parameters were audited.
//! `rets` and `consts` feed receiver-type inference in `uses.rs`: a
//! qualified ambiguous callsite (`x.contains("lit")`) resolves by
//! proving the receiver type, and `let`/`const` annotations are the
//! local evidence that does it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::source::{as_str, call_args, match_bracket};
use super::uses::{impl_owner, ty_leaf};

/// One registered `code`-channel declaration — a `fn` or closure whose
/// parameter list carries a string-typed `code`/`.._code`/analyzer-`id`
/// parameter. Indices are *parameter* indices counting a leading `self`
/// receiver; callsites apply them by shape — `recv.f(…)` binds the
/// receiver, `T::f(recv, …)`/`f(…)` pass arguments positionally.
#[derive(Clone)]
pub(super) struct SinkDecl {
    /// The parameter index carrying the code channel, `self` included.
    pub param: usize,
    /// The declaration's parameter count, `self` included.
    pub arity: usize,
    /// Whether a `self` receiver occupies parameter 0.
    pub has_self: bool,
    /// The `impl` owner type — `contains` on `RuleRegistry` — used to
    /// prove a qualified ambiguous callsite's receiver. `None` for
    /// closures and module-level fns.
    pub owner: Option<String>,
}

/// `fn` names with one declaration per registered `code` parameter —
/// a name defined twice (`error` on `Diagnostic` and on the canonical
/// service) keeps every declaration.
pub(super) type CodeSinks = BTreeMap<String, Vec<SinkDecl>>;

/// Every `fn` name, `let`-bound closure variable, `struct`/`enum`/
/// `union`/`trait`/`type`/`mod` name defined in the scanned sources —
/// the callees and receiver types the census can attribute. A bare
/// callsite callee absent here is unanalyzable and fails closed; a
/// receiver type absent here stays unproven.
pub(super) type DefinedCallees = BTreeSet<String>;

/// The callee/owner/name tables the census collects before scanning —
/// sink declarations, defined names, `fn` return-type leaves for
/// receiver inference (`let x = make()`), and `const`/`static` type
/// leaves (`x::CODE` path-leaf bindings and `KINDS.contains` receivers).
pub(super) struct CalleeTables {
    pub sinks: CodeSinks,
    pub defined: DefinedCallees,
    /// `fn name` → leaf type name of its `-> Type`.
    pub rets: BTreeMap<String, String>,
    /// `const`/`static` name → leaf type name of its `: Type`.
    pub consts: BTreeMap<String, String>,
}

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

/// Split `text` on commas that sit outside `<>`/`()`/`[]`/`{}` — where
/// clauses and generics use both bracket kinds inside a single bound.
fn split_top_level(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out
}

/// Whether `bounds` (generics list plus `where` clause text) binds the
/// bare-ident parameter type `ty` to a string-carrying trait —
/// `code: T` where `T: Into<String>`/`AsRef<str>` is a code channel.
fn bound_is_string(bounds: &[&str], ty: &str) -> bool {
    bounds.iter().any(|bound| {
        let bound = bound.trim();
        let bound = bound.strip_prefix("where").unwrap_or(bound).trim();
        let bound = bound.strip_prefix('<').unwrap_or(bound).trim();
        (bound.starts_with(&format!("{ty}:")) || bound.starts_with(&format!("{ty} :")))
            && [
                "Into", "AsRef", "Borrow", "String", "Display", "ToString", "str",
            ]
            .iter()
            .any(|t| bound.contains(t))
    })
}

/// Whether `param` (`name: type`) declares a code channel: the name is
/// `code`/`.._code` (or analyzer `id`) and the type can carry a string.
fn is_code_param(pname: &str, ptype: &str, bounds: &[&str], analyzer: bool) -> bool {
    let pname = pname.trim().trim_start_matches("mut ").trim();
    if !(pname == "code" || pname.ends_with("_code") || (analyzer && pname == "id")) {
        return false;
    }
    let ptype = ptype.trim();
    let bare_ident = !ptype.is_empty()
        && ptype.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && ptype.chars().next().is_some_and(char::is_uppercase);
    ptype.contains("str")
        || ptype.contains("String")
        || ptype.contains("Into")
        || ptype.contains("AsRef")
        || ptype.contains("Borrow")
        || ptype.contains("Cow")
        || ptype.contains("Display")
        || ptype.contains("ToString")
        || (bare_ident && bound_is_string(bounds, ptype))
}

/// Collect the callee tables: `fn`/closure names whose parameter list
/// carries a string-typed `code` parameter (`code: &str`,
/// `code: String`, `code: impl Into<String>`, a generic `code: T`
/// whose bound mentions `Into`/`AsRef`/`str`) — one [`SinkDecl`] per
/// code parameter, recording its *parameter* index (the receiver is a
/// parameter but consumes no callsite argument), the arity, whether a
/// `self` receiver leads, and the `impl` owner type. In wright-analyzer
/// a string-typed `id:` parameter is a rule-code channel too.
/// `let name = |code: &str, …|` closure bindings register under their
/// variable so `cb("lit")` callsites classify. Built from the masked
/// sources so `#[cfg(test)]` helpers never register.
///
/// The rest of [`CalleeTables`]: every `fn` name, closure-binding name,
/// `struct`/`enum`/`union`/`trait`/`type`/`mod` name (`call_arg` tells
/// an audited non-code callee — `Other` — from an unresolvable one —
/// `Unresolved`), each `fn`'s `-> Type` leaf, and each `const`/`static`'s
/// `: Type` leaf for `uses.rs` receiver inference.
pub(super) fn collect_code_sinks(files: &BTreeMap<PathBuf, (String, Vec<u8>)>) -> CalleeTables {
    let mut tables = CalleeTables {
        sinks: BTreeMap::new(),
        defined: BTreeSet::new(),
        rets: BTreeMap::new(),
        consts: BTreeMap::new(),
    };
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
            tables.defined.insert(name.clone());
            // Capture the generics list `<'a, T: Bound>` and the `where`
            // clause after `)` — a bare-ident param type (`code: T`) is
            // a string channel only when its own bound says so, and the
            // bound must be matched to *this* fn's parameter names.
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
            // The `where` clause runs from its `where` keyword to the
            // body's `{`/`;`; anything before it (`-> Type`) is not a
            // bound and must not be associated with a parameter.
            let mut wend = params_close + 1;
            while wend < masked.len()
                && masked[wend] != b'{'
                && masked[wend] != b';'
                && wend - params_close < 400
            {
                wend += 1;
            }
            let tail = &text[params_close + 1..wend];
            // `-> Type` — the return-type leaf feeds `uses.rs`
            // receiver inference (`let x = f()`).
            if let Some(arrow) = tail.find("->") {
                let ret = tail[arrow + 2..].split("where").next().unwrap_or("").trim();
                let leaf = ty_leaf(ret);
                if !leaf.is_empty() {
                    tables.rets.entry(name.clone()).or_insert(leaf);
                }
            }
            let where_text = tail
                .match_indices("where")
                .find(|(at, _)| {
                    tail[..*at]
                        .chars()
                        .last()
                        .is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
                })
                .map(|(at, _)| &tail[at..])
                .unwrap_or("");
            let mut bounds: Vec<&str> = split_top_level(&text[name_end..generics_end]);
            bounds.extend(split_top_level(where_text));
            // The receiver (`self`/`&self`/`&mut self`/`self: T`) is a
            // parameter but takes no callsite argument — callsites
            // apply a declaration by shape: `recv.f(…)` supplies the
            // receiver (`args + 1 == arity`, arg `i` ↔ param `i + 1`),
            // while `T::f(recv, …)` and bare `f(…)` pass arguments
            // positionally (`args == arity`, arg `i` ↔ param `i`).
            let has_self = params
                .first()
                .map(|&(ps, pe)| {
                    let mut p = as_str(masked, ps..pe).trim();
                    if let Some(rest) = p.strip_prefix('&') {
                        p = rest.trim_start();
                        if let Some(rest) = p.strip_prefix('\'') {
                            p = rest
                                .trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_')
                                .trim_start();
                        }
                    }
                    p = p.strip_prefix("mut ").unwrap_or(p).trim_start();
                    p == "self"
                        || p.strip_prefix("self")
                            .is_some_and(|r| r.trim_start().starts_with(':'))
                })
                .unwrap_or(false);
            let owner = has_self.then(|| impl_owner(masked, at)).flatten();
            for (index, (ps, pe)) in params.iter().enumerate() {
                let param = as_str(masked, *ps..*pe).trim().to_string();
                let Some((pname, ptype)) = param.split_once(':') else {
                    continue;
                };
                if is_code_param(pname, ptype, &bounds, analyzer) {
                    tables
                        .sinks
                        .entry(name.clone())
                        .or_default()
                        .push(SinkDecl {
                            param: index,
                            arity: params.len(),
                            has_self,
                            owner: owner.clone(),
                        });
                }
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
            // The binding is a closure — a defined callee name.
            tables.defined.insert(var.clone());
            let Some(pipe_end) = masked[j + 1..]
                .iter()
                .position(|b| *b == b'|')
                .map(|q| j + 1 + q)
            else {
                continue;
            };
            let params_text = as_str(masked, j + 1..pipe_end);
            let arity = params_text.split(',').count();
            for (index, param) in params_text.split(',').enumerate() {
                let param = param.trim().trim_start_matches("mut ").trim();
                let code_named = match param.split_once(':') {
                    Some((pname, ptype)) => is_code_param(pname, ptype, &[], analyzer),
                    // `|code| …` — untyped params register on name.
                    None => {
                        param == "code" || param.ends_with("_code") || (analyzer && param == "id")
                    }
                };
                if code_named {
                    tables.sinks.entry(var.clone()).or_default().push(SinkDecl {
                        param: index,
                        arity,
                        has_self: false,
                        owner: None,
                    });
                }
            }
        }
        // `const NAME: Ty`/`static NAME: Ty` — the declared type's leaf
        // names the binding's type for `uses.rs` receiver inference and
        // the `x::NAME` path-leaf rule.
        for kw in ["const ", "static "] {
            for (at, _) in text.match_indices(kw) {
                if at > 0 && (masked[at - 1].is_ascii_alphanumeric() || masked[at - 1] == b'_') {
                    continue;
                }
                let rest = &text[at + kw.len()..];
                let name_len = rest
                    .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ' '))
                    .unwrap_or(0);
                let name = original[at + kw.len()..at + kw.len() + name_len]
                    .trim()
                    .trim_start_matches("mut ")
                    .trim();
                if name.is_empty() {
                    continue;
                }
                let mut j = at + kw.len() + name_len;
                while j < masked.len()
                    && masked[j] != b':'
                    && masked[j] != b'='
                    && masked[j] != b';'
                    && masked[j] != b'{'
                {
                    j += 1;
                }
                if masked.get(j) != Some(&b':') || masked.get(j + 1) == Some(&b':') {
                    continue;
                }
                let ty_start = j + 1;
                let mut ty_end = ty_start;
                let mut depth = 0i32;
                while ty_end < masked.len() {
                    match masked[ty_end] {
                        b'<' | b'(' | b'[' => depth += 1,
                        b'>' | b')' | b']' => depth -= 1,
                        b'=' | b';' | b'{' if depth <= 0 => break,
                        _ => {}
                    }
                    ty_end += 1;
                }
                let leaf = ty_leaf(as_str(masked, ty_start..ty_end).trim());
                if !leaf.is_empty() {
                    tables.consts.insert(name.to_string(), leaf);
                }
            }
        }
        // `struct Name`/`enum Name`/`union Name`/`trait Name`/
        // `type Name`/`mod Name` — type, module, and constructor names
        // are resolvable callees and receiver types.
        for kw in ["struct ", "enum ", "union ", "trait ", "type ", "mod "] {
            for (at, _) in text.match_indices(kw) {
                if at > 0 && (masked[at - 1].is_ascii_alphanumeric() || masked[at - 1] == b'_') {
                    continue;
                }
                let mut j = at + kw.len();
                while j < masked.len() && masked[j].is_ascii_whitespace() {
                    j += 1;
                }
                let nstart = j;
                while j < masked.len() && (masked[j].is_ascii_alphanumeric() || masked[j] == b'_') {
                    j += 1;
                }
                if j > nstart {
                    tables.defined.insert(original[nstart..j].to_string());
                }
            }
        }
    }
    tables
}

/// The leaf identifier of a `use`/`let`-bound path — `a::b::f` and
/// `a::b::{f}` both leaf at `f`.
fn path_leaf(path: &str) -> &str {
    path.trim_end_matches('}')
        .rsplit("::")
        .next()
        .unwrap_or(path)
        .trim()
}

/// Register direct aliases of known sinks into `sinks`:
/// `use a::b::sink as alias`, `use a::b::{sink as alias}`, and
/// `let alias = a::b::sink;`/`const alias = a::b::sink` all forward the
/// callee's code-parameter indices onto the alias name. Deeper
/// indirection (fn pointers built by other means, `use` targets that
/// are not sinks) stays unregistered by design — those callsites are
/// covered by the runtime chokes, and the module docs say so.
pub(super) fn collect_aliases(files: &BTreeMap<PathBuf, (String, Vec<u8>)>, sinks: &mut CodeSinks) {
    for (original, masked) in files.values() {
        let text = as_str(masked, 0..masked.len()).to_string();
        // `use` statements: every `X as alias` where leaf `X` is a sink.
        for (at, _) in text.match_indices("use ") {
            if at > 0 && (masked[at - 1].is_ascii_alphanumeric() || masked[at - 1] == b'_') {
                continue;
            }
            let Some(semi) = text[at..].find(';').map(|n| at + n) else {
                continue;
            };
            let stmt = &original[at + 4..semi];
            for (as_at, _) in stmt.match_indices(" as ") {
                let mut leaf_end = as_at;
                while leaf_end > 0
                    && (stmt.as_bytes()[leaf_end - 1].is_ascii_alphanumeric()
                        || stmt.as_bytes()[leaf_end - 1] == b'_')
                {
                    leaf_end -= 1;
                }
                let leaf = &stmt[leaf_end..as_at];
                let mut a = as_at + 4;
                while a < stmt.len() && stmt.as_bytes()[a].is_ascii_whitespace() {
                    a += 1;
                }
                let mut a_end = a;
                while a_end < stmt.len()
                    && (stmt.as_bytes()[a_end].is_ascii_alphanumeric()
                        || stmt.as_bytes()[a_end] == b'_')
                {
                    a_end += 1;
                }
                let alias = &stmt[a..a_end];
                if !alias.is_empty()
                    && let Some(decls) = sinks.get(leaf).cloned()
                {
                    sinks.entry(alias.to_string()).or_insert(decls);
                }
            }
        }
        // `let alias = path::leaf;` / `const ALIAS = path::leaf;` — the
        // RHS is a bare `ident(::ident)*` path whose leaf is a sink.
        for kw in ["let ", "const ", "static "] {
            for (at, _) in text.match_indices(kw) {
                if at > 0 && (masked[at - 1].is_ascii_alphanumeric() || masked[at - 1] == b'_') {
                    continue;
                }
                let rest = &text[at + kw.len()..];
                let name_len = rest
                    .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ' '))
                    .unwrap_or(0);
                let name = original[at + kw.len()..at + kw.len() + name_len]
                    .trim()
                    .to_string();
                if name.is_empty() || name == "mut" {
                    continue;
                }
                let name = name.trim_start_matches("mut ").to_string();
                let mut j = at + kw.len() + name_len;
                while j < masked.len()
                    && masked[j] != b'='
                    && masked[j] != b';'
                    && masked[j] != b'{'
                {
                    j += 1;
                }
                if masked.get(j) != Some(&b'=') || masked.get(j + 1) == Some(&b'=') {
                    continue;
                }
                let mut rhs_end = j + 1;
                while rhs_end < masked.len() && masked[rhs_end] != b';' {
                    rhs_end += 1;
                }
                let rhs = original[j + 1..rhs_end].trim();
                let is_path = !rhs.is_empty()
                    && rhs
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == ':' || c == ' ');
                if is_path && let Some(decls) = sinks.get(path_leaf(rhs)).cloned() {
                    sinks.entry(name).or_insert(decls);
                }
            }
        }
    }
}
