//! Call-site argument classification: the callee's identity and shape
//! decide which arguments sit on the `code` channel — registered sink
//! declarations applied by callsite form (`.`-method vs positional
//! `T::f`/`f`), qualified ambiguous sinks resolved through the
//! receiver/head's proven type, code-channel target writes
//! (`mem::replace(&mut d.code, …)`, `String::push_str`), `insert("code",
//! v)` key-values, and the data-macro whitelist.

use super::classify::{FileCtx, Outcome};
use super::expand::expr_start_before;
use super::positions::{code_tail_field, is_code_field, mutates_string};
use super::sinks::AMBIGUOUS_SINKS;
use super::source::{as_str, call_args};
use super::uses::{OwnerCheck, receiver_owner};

/// How the callsite supplies a `self` receiver — decides which
/// parameter index a callsite argument binds.
enum CallForm {
    /// `recv.f(a0, …)` — the receiver binds parameter 0, so argument
    /// `i` sits at parameter `i + 1`; `rs..re` is the receiver span.
    Method(usize, usize),
    /// `T::f(r, a1, …)`/`Self::f`/`m::f`/`f` — arguments pass
    /// positionally: argument `i` is parameter `i`. `head` is the path
    /// before the leaf (`T`, `Self`, `m`); a method call's receiver
    /// arg is just argument 0.
    Path(String),
}

/// The callsite's shape — a `.` method call with its receiver span, or
/// a `::`/bare path with the head before the leaf (`""` when the head
/// is unrecoverable, e.g. `<T>::f` whose `<` stops callee capture).
fn call_form(masked: &[u8], callee: &str, callee_start: usize) -> CallForm {
    let trimmed = callee.trim_end_matches(':');
    let leaf_len = trimmed.rsplit([':', '.']).next().map(str::len).unwrap_or(0);
    let sep = &trimmed[..trimmed.len() - leaf_len];
    if sep.ends_with('.') && !sep.ends_with("::") {
        // `recv.f` — the receiver ends at the last `.`; `a.b.f`'s
        // receiver is `a.b`, inside the callee text. When the callee
        // capture started *at* the `.` (`f().contains` captures
        // `.contains`), the receiver is the expression before it.
        let re = callee_start + sep.len() - 1;
        let rs = if sep == "." {
            expr_start_before(masked, callee_start)
        } else {
            callee_start
        };
        CallForm::Method(rs, re)
    } else {
        CallForm::Path(
            sep.trim_end_matches([':', '.'])
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string(),
        )
    }
}

/// The type a `T::f`/`Self::f` path head names — `Self` resolves to
/// the enclosing `impl` block's type, `m::T` and `T` name `T`, a
/// lowercase head is a module path (`None` — module fns own no `self`).
fn ufcs_head_ty(ctx: &FileCtx, head: &str, at: usize) -> Option<String> {
    let leaf = head.rsplit("::").next().unwrap_or(head).trim();
    if leaf.is_empty() {
        return None;
    }
    if leaf == "Self" {
        return super::uses::impl_owner(ctx.masked, at);
    }
    if leaf.chars().next().is_some_and(char::is_uppercase) {
        return Some(leaf.to_string());
    }
    None
}

/// `call(args)` — the callee decides the argument's role.
pub(super) fn call_arg(
    ctx: &FileCtx,
    callee: &str,
    callee_start: usize,
    open: usize,
    s: usize,
) -> Outcome {
    let masked = ctx.masked;
    let args = call_args(masked, open);
    let index = args
        .iter()
        .position(|&(a, b)| s >= a && s <= b)
        .unwrap_or(args.len());
    let trimmed = callee.trim_end_matches(':');
    let leaf = trimmed.rsplit([':', '.']).next().unwrap_or(trimmed);
    if callee.ends_with('!') {
        let name = leaf.trim_end_matches('!');
        // `write!(w, "tpl", x)`/`writeln!` — every argument past the
        // writer lands in `w`; a code-channel writer makes each a code
        // piece (the deferred name-hole scan resolves the values).
        if matches!(name, "write" | "writeln")
            && index >= 1
            && args
                .first()
                .is_some_and(|&(a, b)| arg_is_code_channel(ctx, a, b))
        {
            return Outcome::Fragment("code-write");
        }
        return match name {
            // `concat!` args are pieces of the produced string — the
            // expander absorbs them into the call's span, so reaching
            // here means the walk missed one: fail loudly.
            "concat" => Outcome::Unresolved("concat-arg"),
            // Macros whose produced value can never be a diagnostic
            // code: formatting/printing/panicking templates, the
            // assert family, data constructors (`vec!`, `json!`,
            // `matches!`, …), `env!`/`include_*!` (their produced
            // strings are external data), and the pinned third-party
            // helpers (`hotpath::measure_block!`). An unlisted macro's
            // argument is unclassifiable — it must be audited, not
            // defaulted.
            _ if KNOWN_DATA_MACROS.contains(&name) => Outcome::Other("macro-arg"),
            _ => Outcome::Unresolved("unknown-macro-arg"),
        };
    }
    let form = call_form(masked, callee, callee_start);
    let receiver_span = match form {
        CallForm::Method(rs, re) => Some((rs, re)),
        CallForm::Path(_) => None,
    };
    // Writes that take the code channel as target arg0 —
    // `mem::replace(&mut d.code, "x")`, `ptr::write(&mut d.code, "x")` —
    // write the whole value; `String` mutators in UFCS form
    // (`String::push_str(&mut code, "x")`, `Write::write_fmt`) append a
    // piece. Both fire on any callsite shape — the receiver-rule below
    // covers the `code.push_str("x")` spelling.
    if index >= 1
        && args
            .first()
            .is_some_and(|&(a, b)| arg_is_code_channel(ctx, a, b))
    {
        if [
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
        {
            return Outcome::Vocab("code-write-arg");
        }
        if mutates_string(leaf) || leaf == "write_fmt" {
            return Outcome::Fragment("code-append");
        }
    }
    // `insert("code", v)` — the literal value behind a code-named key.
    if matches!(leaf, "insert" | "insert_str" | "get_or_insert")
        && index >= 1
        && args.first().is_some_and(|&(a, b)| {
            lit_content(ctx, a, b)
                .is_some_and(|k| is_code_field(&k, ctx.analyzer) || code_tail_field(&k))
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
    // Sink declarations bind callsite arguments to `code` parameters.
    // The callsite shape picks the applicable declarations: a method
    // call supplies the receiver out-of-band (`args + 1 == arity`, arg
    // `i` ↔ param `i + 1`), while `T::f(r, …)`/`Self::f`/bare `f(…)`
    // pass arguments positionally (`args == arity`, arg `i` ↔ param
    // `i`; a bare call cannot supply a `self` param). A leaf that also
    // collides with a std method (`contains`, `insert`, …) is
    // ambiguous: the callsite only proves the sink when the receiver
    // or path head resolves to the declaration's `impl` owner —
    // `ProbeOther::contains` and `str::contains` are not it, and a
    // receiver nothing proves (`x: &T`) cannot clear the use.
    // A char-literal argument can never satisfy the `&str`/`String`
    // code channel a sink declares — `x.contains(':')` is some other
    // fn's callsite regardless of what the receiver proves.
    let char_arg = args
        .get(index)
        .is_some_and(|&(a, b)| masked[a] == b'\'' && masked[b - 1] == b'\'');
    if !char_arg && let Some(decls) = ctx.tables.sinks.get(leaf) {
        let ambiguous = AMBIGUOUS_SINKS.contains(&leaf);
        let mut saw_slot = false;
        let mut undecided = false;
        for decl in decls {
            let (fits, hit) = match &form {
                CallForm::Method(..) => (
                    decl.has_self && args.len() + 1 == decl.arity,
                    decl.param == index + 1,
                ),
                CallForm::Path(head) => {
                    let bare = head.is_empty() && !callee.contains([':', '.']);
                    if bare && decl.has_self {
                        (false, false) // a bare call cannot reach a method
                    } else {
                        (args.len() == decl.arity, decl.param == index)
                    }
                }
            };
            if !fits {
                continue;
            }
            if ambiguous {
                let check = match (&form, decl.owner.as_deref()) {
                    (CallForm::Method(rs, re), Some(owner)) => receiver_owner(ctx, *rs, *re, owner),
                    // A receiver-typed decl on a `.` callsite whose
                    // `impl` owner the census could not read.
                    (CallForm::Method(..), None) => OwnerCheck::Unknown,
                    (CallForm::Path(head), Some(owner)) => {
                        match ufcs_head_ty(ctx, head, callee_start) {
                            Some(ty) if ty == owner => OwnerCheck::Owner,
                            Some(_) => OwnerCheck::Other,
                            None => {
                                if callee.contains(':') {
                                    // `T::f` with an unreadable head,
                                    // or a module path — the fn could
                                    // still be a free `f`.
                                    if decl.has_self {
                                        OwnerCheck::Unknown
                                    } else {
                                        OwnerCheck::Owner
                                    }
                                } else {
                                    OwnerCheck::Other
                                }
                            }
                        }
                    }
                    // A module-level decl: `m::f`/bare `f` calls bind
                    // it directly.
                    (CallForm::Path(_), None) => OwnerCheck::Owner,
                };
                match check {
                    OwnerCheck::Owner => {}
                    OwnerCheck::Other => continue,
                    OwnerCheck::Unknown => {
                        undecided = true;
                        continue;
                    }
                }
            }
            if hit {
                return Outcome::Vocab("sink-arg");
            }
            saw_slot = true;
        }
        if undecided {
            return Outcome::Unresolved("ambiguous-receiver");
        }
        if saw_slot {
            return Outcome::Other("sink-other-arg");
        }
        // No declaration fits this callsite — it is some other fn's
        // call. An ambiguous leaf falls through to the qualified-call
        // rule; a leaf declared only as a method can never be the
        // callee here, so it is no more provable than any qualified
        // call.
    }
    if callee.contains(['.', ':']) && AMBIGUOUS_SINKS.contains(&leaf) {
        return Outcome::Other("ambiguous-callee");
    }
    // The argument of a callee the census can attribute is positively
    // data: `recv.f(…)`/`path::f(…)` defer to the receiver's type or
    // the owning module, and a `fn`/closure/type defined in the scanned
    // sources had its parameter list audited — no `code` channel
    // registered. A *bare* callee that resolves to nothing is
    // unanalyzable — `unknown_fn(c)` proves nothing about `c`, so the
    // use cannot clear a deferred literal.
    if callee.contains(['.', ':']) || ctx.tables.defined.contains(leaf) {
        return Outcome::Other("call-arg");
    }
    Outcome::Unresolved("unknown-callee")
}

/// Macro callees whose arguments can never produce a diagnostic code —
/// `call_arg` whitelists them as `Other`; any other `!`-callee's
/// argument is unclassifiable (`Unresolved`), and `concat!` is
/// transparent to the expander so its args never reach this table.
const KNOWN_DATA_MACROS: &[&str] = &[
    "assert",
    "assert_eq",
    "assert_matches",
    "assert_ne",
    "cfg",
    "column",
    "compile_error",
    "concat_idents",
    "dbg",
    "debug",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "env",
    "eprint",
    "eprintln",
    "error",
    "file",
    "format",
    "format_args",
    "format_args_nl",
    "include",
    "include_bytes",
    "include_str",
    "info",
    "json",
    "json_internal",
    "line",
    "log",
    "matches",
    "measure_block",
    "module_path",
    "option_env",
    "panic",
    "print",
    "println",
    "stringify",
    "thread_local",
    "todo",
    "trace",
    "unimplemented",
    "unreachable",
    "vec",
    "warn",
    "write",
    "writeln",
];

/// The cooked text of `masked[a..b]` when it is exactly one literal.
fn lit_content(ctx: &FileCtx, a: usize, b: usize) -> Option<String> {
    let text = ctx.original[a..b].trim();
    let text = text.strip_prefix('r').unwrap_or(text);
    let inner = text.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.to_string())
}

/// Whether the arg at `a..b` names a code channel — `&mut d.code`,
/// `code`, `d.provider_code`, analyzer `d.id`.
pub(super) fn arg_is_code_channel(ctx: &FileCtx, a: usize, b: usize) -> bool {
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
