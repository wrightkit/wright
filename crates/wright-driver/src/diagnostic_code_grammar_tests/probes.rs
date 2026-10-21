//! Position-rule probes: synthetic source literals driven through
//! `classify` + `resolve_deferred` to pin each construct's verdict.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use super::{classify, sinks, source};

/// Position-rule probes on a synthetic source: each construct the
/// round-8 review called out classifies the way the fix requires —
/// receiver-shifted sink args, `concat!` folding, per-index tuple
/// destructures, `fn`-return deferral (a `code`-position callsite
/// proves the return literal; a never-called `fn` resolves non-code),
/// unknown bare callees and unknown macros staying unresolved, an
/// orphan literal staying unresolved, and an ordinary non-code binding
/// resolving non-code through a defined callee.
#[test]
fn census_position_probes() {
    const PROBE: &str = r##"
struct Recv;
struct Other;
struct Registry;
impl Recv {
    fn deny(&self, code: &str) {
        let _ = code;
    }
    fn multi(&self, msg: &str, code: &str) {
        let _ = (msg, code);
    }
    fn via_self(&self) {
        Self::deny(self, "probe-ufcs-self");
    }
}
impl Registry {
    // A `code`-param name that collides with std `contains` — the
    // callsite only proves the sink through the receiver's owner.
    fn contains(&self, code: &str) -> bool {
        code.is_empty()
    }
}
mod probe_mod {
    pub const ROUTE: &str = "probe-const";
}
fn deny_once(code: &str) {
    let _ = code;
}
fn consume(text: &str) {
    let _ = text;
}
fn make_tag() -> String {
    "probe-tag".to_string()
}
fn never_called() -> String {
    "probe-unused".to_string()
}
fn drive<T>(recv: &Recv, reg: &Registry, input: &str, generic: &T, other: &Other) {
    recv.deny("probe-sink-arg");
    recv.multi("probe-first", "probe-second");
    // UFCS / explicit receivers — `code` is param 1 counting `self`.
    Recv::deny(recv, "probe-ufcs-path");
    <Recv>::deny(recv, "probe-ufcs-lt");
    let mut code = String::new();
    String::push_str(&mut code, "probe-ufcs-piece");
    // Qualified ambiguous sinks decide by receiver type.
    reg.contains("probe-ambig-own");
    input.contains("probe-ambig-str");
    other.contains("probe-ambig-local");
    generic.contains("probe-ambig-gen");
    // Path-leaf const uses.
    deny_once(probe_mod::ROUTE);
    deny_once(crate::probe_mod::ROUTE);
    // Non-tuple destructures bind per element.
    let [code] = ["probe-arr-code"];
    let [first, rest] = ["probe-a0", "probe-a1"];
    if let Some(code) = Some("probe-wrap") {
        let _ = code;
    }
    let Ok(code) = Ok::<&str, ()>("probe-ok") else {
        return;
    };
    let _ = (first, rest);
    // format!/write! template holes, named args included.
    let tag = "probe-hole";
    let s = format!("{tag}");
    deny_once(&s);
    let tag2 = "probe-hole-two";
    deny_once(&format!("{tag2}"));
    let tag3 = "probe-hole-three";
    deny_once(&format!("{t}", t = tag3));
    let piece = "probe-hole-write";
    let _ = write!(code, "{piece}");
    // json!/serde code keys and a later-element stage marker.
    let details = json!({ "refusalCode": "probe-json-code" });
    let _ = details;
    deny_once(("probe-stage-late", Stage::Frontend).0);
    // String concatenation and collection producers.
    deny_once("probe-a" + "whole-b");
    let s = "probe" + " tail";
    deny_once(&s);
    deny_once(&["probe-join", "bad"].join("-"));
    let code = concat!("probe-concat-", "tail");
    deny_once(&code);
    println!("{}", concat!("probe-print-", "joined"));
    let (code, note) = ("probe-tuple-code", "probe-tuple-note");
    deny_once(&code);
    consume(note);
    recv.deny(&make_tag());
    let msg = "probe-msg";
    println!("{}", msg);
    let c = "probe-unknown";
    unknown_fn(c);
    mystery!("probe-macro");
    "probe-orphan";
}
"##;
    let (masked, lits) = source::mask_rust_full(PROBE);
    let path = PathBuf::from("probe.rs");
    let files: BTreeMap<PathBuf, (String, Vec<u8>)> =
        [(path.clone(), (PROBE.to_string(), masked.clone()))].into();
    let tables = sinks::collect_code_sinks(&files);
    let scan = classify::ScanCtx {
        files: &files,
        tables: &tables,
    };
    let ctx = classify::FileCtx {
        original: PROBE,
        masked: &masked,
        analyzer: false,
        stage_glob: false,
        tables: &tables,
    };
    let mut got: BTreeMap<&str, &str> = BTreeMap::new();
    for lit in &lits {
        let content = &PROBE[lit.content.0..lit.content.1];
        let label = match classify::classify(&ctx, lit) {
            classify::Outcome::Vocab(_) => "vocab",
            classify::Outcome::Fragment(_) => "fragment",
            classify::Outcome::Other(_) => "other",
            classify::Outcome::Unresolved(_) => "unresolved",
            classify::Outcome::Deferred {
                names,
                scope,
                piece,
            } => {
                match classify::resolve_deferred(
                    &names,
                    &path,
                    &scope,
                    &scan,
                    &mut HashSet::new(),
                    0,
                ) {
                    classify::Flow::Vocab if piece => "deferred-piece",
                    classify::Flow::Vocab => "deferred-vocab",
                    classify::Flow::Piece => "deferred-piece",
                    classify::Flow::Other => "deferred-other",
                    classify::Flow::Unknown(_) => "deferred-unresolved",
                }
            }
        };
        got.insert(content, label);
    }
    for (content, expected) in [
        // `fn deny(&self, code)` registers `code` at callsite index 0.
        ("probe-sink-arg", "vocab"),
        // `fn multi(&self, msg, code)` — only the second arg codes.
        ("probe-first", "other"),
        ("probe-second", "vocab"),
        // UFCS / explicit receivers bind `code` at its parameter index.
        ("probe-ufcs-self", "vocab"),
        ("probe-ufcs-path", "vocab"),
        ("probe-ufcs-lt", "vocab"),
        // `String::push_str(&mut code, "x")` appends a code piece.
        ("probe-ufcs-piece", "fragment"),
        // Qualified ambiguous `contains`: owner receiver codes, `str`
        // and defined non-owner receivers stay data, an unprovable
        // receiver fails closed.
        ("probe-ambig-own", "vocab"),
        ("probe-ambig-str", "other"),
        ("probe-ambig-local", "other"),
        ("probe-ambig-gen", "unresolved"),
        // `m::CODE`/`crate::m::CODE` path leaves are uses of the const.
        ("probe-const", "deferred-vocab"),
        // `[code]`/`Some(code)`/`Ok(code)` patterns bind per element.
        ("probe-arr-code", "vocab"),
        ("probe-a0", "deferred-other"),
        ("probe-a1", "deferred-other"),
        ("probe-wrap", "vocab"),
        ("probe-ok", "vocab"),
        // format!/write! holes use the bound name — `format!("{t}",
        // t = tag3)` resolves through the named argument.
        ("probe-hole", "deferred-vocab"),
        ("probe-hole-two", "deferred-vocab"),
        ("probe-hole-three", "deferred-vocab"),
        ("probe-hole-write", "deferred-piece"),
        // `"xCode": v` JSON/serde keys make the value a code.
        ("refusalCode", "other"),
        ("probe-json-code", "vocab"),
        // `(lit, …, Stage::X)` — the code is element 0.
        ("probe-stage-late", "vocab"),
        // `+` concat at a code position makes each operand a piece;
        // a bound `+` chain stays a piece through deferral;
        // `[..].join` elements are pieces of the produced string.
        ("probe-a", "fragment"),
        ("whole-b", "fragment"),
        ("probe", "deferred-piece"),
        (" tail", "deferred-piece"),
        ("probe-join", "fragment"),
        ("bad", "fragment"),
        ("-", "other"),
        // `concat!` args fold into the produced string — the `code`
        // binding's RHS is a code position.
        ("probe-concat-", "vocab"),
        ("tail", "vocab"),
        // …and inside a whitelisted macro they stay data.
        ("probe-print-", "other"),
        ("joined", "other"),
        // Per-index tuple destructure: `code` codes, `note` defers and
        // resolves non-code through `consume`.
        ("probe-tuple-code", "vocab"),
        ("probe-tuple-note", "deferred-other"),
        // A non-code-named `fn`'s return literal defers under the fn
        // name: `recv.deny(&make_tag())` proves it, `never_called`
        // resolves non-code.
        ("probe-tag", "deferred-vocab"),
        ("probe-unused", "deferred-other"),
        // A deferred name used only by an unresolvable callee stays
        // loud, an unknown macro's arg is unresolved, and an orphan
        // statement literal has no evidence at all.
        ("probe-unknown", "deferred-unresolved"),
        ("probe-macro", "unresolved"),
        ("probe-orphan", "unresolved"),
        // The ordinary deferred non-code binding keeps resolving.
        ("probe-msg", "deferred-other"),
    ] {
        assert_eq!(
            got.get(content).copied(),
            Some(expected),
            "probe literal {content:?} classified {:?}, expected {expected:?}",
            got.get(content)
        );
    }
}
