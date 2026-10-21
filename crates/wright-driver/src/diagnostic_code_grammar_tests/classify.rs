//! The literal classifier behind the driver-code census: for every
//! literal the mask recorded in a production source file, decide what
//! position it sits in.
//!
//! The classifier is fail-closed — a literal lands in exactly one of:
//!
//! - [`Outcome::Vocab`]: positive evidence the literal is a whole
//!   diagnostic code (a `code:`/`.._code:`/`id:` field or JSON-key
//!   value, a sink argument at a code-parameter index, a `code`-named
//!   binding, a `(lit, Stage::…)` tuple, a code-returning fn's
//!   return/tail/arm value);
//! - [`Outcome::Fragment`]: a literal appended to a code channel — a
//!   piece, not a whole code (`+=`, `push_str`/`mem::replace` on a code
//!   receiver, `*_CODE_PREFIX`/`_SUFFIX` constants);
//! - [`Outcome::Other`]: positive evidence of a non-code position —
//!   `name:` keys and message fields, operands, comparisons, patterns,
//!   attributes, collection elements, and non-code bindings; arguments
//!   of whitelisted data macros (`format!`, `println!`, `json!`, the
//!   assert family, …) and of callees the census can attribute
//!   (`recv.f`, `path::f`, and `fn`/type names defined in the scanned
//!   sources); and consumption-side reads — `d.code == "x"`
//!   comparisons, `x.code` member reads, `x..name` range bounds,
//!   `name:` field keys, `use` items — positions that *consume* an
//!   already-emitted code rather than emitting one. A literal passed to
//!   a parameter that does not declare the `code` channel is data: if
//!   that parameter reaches a code position, the receiving fn's own
//!   position is recorded on the dynamic report and the runtime chokes
//!   check the produced value;
//! - [`Outcome::Deferred`]: a literal bound to a name — resolved by
//!   proving whether the name is *used* at a code position (`let`
//!   bindings within their fn body, `const`/`static` items and
//!   non-code fn returns crate-wide);
//! - [`Outcome::Unresolved`]: neither side has evidence — the census
//!   fails and prints the site rather than letting a literal slip.
//!
//! Evaluation walks *outward* from the literal through transparent
//! structure (`Some`/`Ok`/`&`/`.to_string()` wrappers, fallback
//! combinators, `if`/`match`/closure produced values, grouping parens)
//! until a position with evidence is found, then that position decides.
//! The pieces live in `brackets.rs` (structure), `expand.rs` (the
//! transparent walk), and `slot.rs` (the position verdicts).

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use super::brackets::{in_attribute, in_pattern, in_use_item, next_nonws};
use super::expand::expand_transparent;
use super::sinks::{CodeSinks, DefinedCallees};
use super::slot::slot_of;
use super::source::{Lit, as_str};

/// Per-file classification context.
pub(super) struct FileCtx<'a> {
    pub original: &'a str,
    pub masked: &'a [u8],
    pub analyzer: bool,
    pub stage_glob: bool,
    pub sinks: &'a CodeSinks,
    /// `fn`, closure, and type names declared anywhere in the scanned
    /// sources — a bare callsite argument is positively non-code when
    /// its callee is one of these (its parameters were audited).
    pub defined: &'a DefinedCallees,
}

/// How far a name bound to a literal can flow.
#[derive(Clone)]
pub(super) enum Scope {
    /// A byte range inside one file — a `let` binding's fn body, a
    /// `for` pattern's loop body.
    Range(usize, usize),
    /// The whole scanned file set — a `const`/`static` item can be
    /// named anywhere.
    AllFiles,
}

/// The classification of one literal.
pub(super) enum Outcome {
    /// A whole diagnostic code — grammar-check it.
    Vocab(&'static str),
    /// A code piece — check whitespace-freedom and the code charset.
    Fragment(&'static str),
    /// Positively not a diagnostic-code position.
    Other(&'static str),
    /// Bound to a name — resolve by proving where the name is used.
    Deferred { names: Vec<String>, scope: Scope },
    /// No evidence either way — the census must fail.
    Unresolved(&'static str),
}

/// Classify one literal: where its value can flow.
pub(super) fn classify(ctx: &FileCtx, lit: &Lit) -> Outcome {
    let masked = ctx.masked;
    let (s, e) = lit.span;
    // What follows the literal: a `:` makes it a JSON/struct key, an
    // `=>` a match-arm pattern.
    if let Some(a) = next_nonws(masked, e) {
        match masked[a] {
            b':' if masked.get(a + 1) != Some(&b':') => return Outcome::Other("key"),
            b'=' if masked.get(a + 1) == Some(&b'>') => return Outcome::Other("arm-pattern"),
            _ => {}
        }
    }
    if in_attribute(masked, s) {
        return Outcome::Other("attribute");
    }
    if in_pattern(masked, s) {
        return Outcome::Other("pattern");
    }
    evaluate(ctx, s, e)
}

/// Expand `s..e` through transparent structure and evaluate the slot
/// the produced value lands in — shared by literal classification and
/// deferred identifier-use resolution.
fn evaluate(ctx: &FileCtx, s: usize, e: usize) -> Outcome {
    let (es, ee, crossed) = expand_transparent(ctx.masked, s, e);
    slot_of(ctx, es, ee, crossed)
}

/// What a deferred literal's use-scan proved.
pub(super) enum Flow {
    /// A use reached a code position — the literal is vocabulary.
    Vocab,
    /// Every use found is positively non-code — including none at all;
    /// a bound-but-never-used value goes nowhere.
    Other,
    /// A use sits at a position the census cannot classify — the
    /// literal is unplaceable and must fail with this reason.
    Unknown(&'static str),
}

/// The whole-scan context deferred resolution searches.
pub(super) struct ScanCtx<'a> {
    pub files: &'a BTreeMap<PathBuf, (String, Vec<u8>)>,
    pub sinks: &'a CodeSinks,
    pub defined: &'a DefinedCallees,
}

/// Whether the literal bound to `names` at `decl_file` flows to a code
/// position: scan the deferred scope for bare-identifier uses and
/// evaluate each use's slot — `let s = "lit"; … refusal(s, …)` proves
/// `s` is a code. A use that itself binds (`let t = s`) re-defers onto
/// `t` until a name reaches a code position or every use resolves.
/// Resolution is three-valued: a `Vocab` use proves the literal is a
/// code, `Other` uses prove nothing happens to it, and any
/// `Unresolved` use — a bare call to a callee nothing defines, a slot
/// the classifier cannot read — propagates [`Flow::Unknown`] so the
/// census fails rather than quietly treating the literal as non-code.
/// `visiting` breaks `let a = b; let b = a` cycles; a cyclic binding
/// contributes no evidence.
pub(super) fn resolve_deferred(
    names: &[String],
    decl_file: &Path,
    scope: &Scope,
    scan: &ScanCtx,
    visiting: &mut HashSet<String>,
    depth: usize,
) -> Flow {
    if depth > 8 {
        return Flow::Unknown("resolve-depth");
    }
    let mut unknown = None;
    for name in names {
        let key = format!("{}::{name}", decl_file.display());
        if !visiting.insert(key.clone()) {
            continue;
        }
        for (path, (original, masked)) in scan.files {
            let file_ctx = FileCtx {
                original,
                masked,
                analyzer: path.to_string_lossy().contains("wright-analyzer"),
                stage_glob: original.contains("use crate::diag::Stage::*"),
                sinks: scan.sinks,
                defined: scan.defined,
            };
            // The scope's byte range; AllFiles searches every file whole.
            let (ls, le) = match scope {
                Scope::Range(rs, re) if path == decl_file => (*rs, *re),
                Scope::Range(_, _) => continue,
                Scope::AllFiles => (0, masked.len()),
            };
            let text = as_str(masked, ls..le).to_string();
            for (at, _) in text.match_indices(name.as_str()) {
                let pos = ls + at;
                let end = pos + name.len();
                // Identifier boundaries — `$x` macro vars, `'x`
                // lifetimes, `x::y` path leaves, `x!` macro names, and
                // `x.name` member pieces are not the bound name. A name
                // after `.` is a field/method of the receiver's type —
                // `name.as_str()` calls `String::as_str`, not a fn this
                // census defined — so it can never be a use of the
                // binding and is skipped outright (consumption-side
                // member reads are covered by the slot rules' `receiver`
                // verdict on the receiver itself).
                let before_ok = pos == 0 || {
                    let b = masked[pos - 1];
                    !(b.is_ascii_alphanumeric()
                        || b == b'_'
                        || b == b':'
                        || b == b'$'
                        || b == b'\''
                        || b == b'.')
                };
                let after_ok = masked.get(end).is_none_or(|b| {
                    !(b.is_ascii_alphanumeric() || *b == b'_' || *b == b':' || *b == b'!')
                });
                if !before_ok || !after_ok {
                    continue;
                }
                // `fn name(` is the fn's declaration, never a use.
                let mut i = pos;
                while i > 0 && masked[i - 1].is_ascii_whitespace() {
                    i -= 1;
                }
                if i >= 2
                    && masked[i - 2..i] == *b"fn"
                    && (i == 2 || !(masked[i - 3].is_ascii_alphanumeric() || masked[i - 3] == b'_'))
                {
                    continue;
                }
                // `use` items, patterns, and attributes are binding or
                // declaration sites — never uses of the bound value.
                if in_use_item(masked, pos) || in_pattern(masked, pos) || in_attribute(masked, pos)
                {
                    continue;
                }
                match evaluate(&file_ctx, pos, end) {
                    Outcome::Vocab(_) | Outcome::Fragment(_) => {
                        visiting.remove(&key);
                        return Flow::Vocab;
                    }
                    Outcome::Deferred { names: next, scope } => {
                        match resolve_deferred(&next, path, &scope, scan, visiting, depth + 1) {
                            Flow::Vocab => {
                                visiting.remove(&key);
                                return Flow::Vocab;
                            }
                            Flow::Unknown(reason) => unknown = unknown.or(Some(reason)),
                            Flow::Other => {}
                        }
                    }
                    Outcome::Other(_) => {}
                    Outcome::Unresolved(reason) => unknown = unknown.or(Some(reason)),
                }
            }
        }
        visiting.remove(&key);
    }
    unknown.map(Flow::Unknown).unwrap_or(Flow::Other)
}
