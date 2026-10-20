//! The engine `kind` census: every `kind` the pinned `workshop-rs` engine
//! can pass to a `WorkshopError` `unknown`-family variant — the only
//! variant family whose `kind` reaches a diagnostic code
//! (`unknown-<kind>`).
//!
//! Kinds are `&'static str`, so the vocabulary is the union of:
//! - the literal first argument at every `WorkshopError::unknown(`,
//!   `WorkshopError::unknown_with_candidates(`,
//!   `WorkshopError::not_detected(`, `.unknown(`, `.unknown_at(`, and
//!   `.not_detected(` call site;
//! - every `catalog::Kind` spelling wherever `kind` is a `Kind`-typed
//!   parameter (e.g. `kind.as_str()`);
//! - string literals forwarded through `&'static str` helper parameters,
//!   resolved by looking up the enclosing function's callers;
//! - `kind:` literal fields in direct `Unknown`-family struct
//!   constructions.
//!
//! Anything else — a computed argument, a `let`-bound value, an
//! identifier that resolves to no `&'static str`/`Kind` parameter —
//! fails the census instead of being sampled (`#871`'s "enumerate or
//! stop" rule).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::source::{
    as_str, blank_cfg_test_items, call_args, enclosing_fn, line_of, mask_rust, match_bracket,
    read_rs_tree, workshop_rs_src,
};

/// `catalog::Kind` — the closed owner enum whose spellings can serve as
/// `kind` where an emitter parameter is `Kind`-typed.
const CATALOG_KINDS: [workshop_rs::catalog::Kind; 7] = [
    workshop_rs::catalog::Kind::Structural,
    workshop_rs::catalog::Kind::Action,
    workshop_rs::catalog::Kind::Value,
    workshop_rs::catalog::Kind::Event,
    workshop_rs::catalog::Kind::Operator,
    workshop_rs::catalog::Kind::Enum,
    workshop_rs::catalog::Kind::Setting,
];

fn catalog_kind_strings() -> Vec<&'static str> {
    // Compile-time and run-time exhaustiveness gates: if the owner enum
    // grows, the census must be re-audited, never silently sampled.
    for kind in CATALOG_KINDS {
        match kind {
            workshop_rs::catalog::Kind::Structural
            | workshop_rs::catalog::Kind::Action
            | workshop_rs::catalog::Kind::Value
            | workshop_rs::catalog::Kind::Event
            | workshop_rs::catalog::Kind::Operator
            | workshop_rs::catalog::Kind::Enum
            | workshop_rs::catalog::Kind::Setting => {}
        }
    }
    assert_eq!(
        workshop_rs::catalog::Kind::NUM_KINDS,
        CATALOG_KINDS.len(),
        "catalog::Kind grew — re-audit the engine kind census"
    );
    CATALOG_KINDS.iter().map(|kind| kind.as_str()).collect()
}

/// Every `kind` the pinned Workshop engine can pass to a `WorkshopError`
/// `unknown`-family variant — the only variant family whose `kind` reaches a
/// diagnostic code (`unknown-<kind>`).
///
/// Kinds are `&'static str`, so the vocabulary is the union of:
/// - the literal first argument at every `WorkshopError::unknown(`,
///   `WorkshopError::unknown_with_candidates(`, `WorkshopError::not_detected(`,
///   `.unknown(`, `.unknown_at(`, and `.not_detected(` call site;
/// - every `catalog::Kind` spelling wherever `kind` is a `Kind`-typed
///   parameter (e.g. `kind.as_str()`);
/// - string literals forwarded through `&'static str` helper parameters,
///   resolved by looking up the enclosing function's callers;
/// - `kind:` literal fields in direct `Unknown`-family struct constructions.
///
/// Anything else — a computed argument, a `let`-bound value, an identifier
/// that resolves to no `&'static str`/`Kind` parameter — fails the census
/// instead of being sampled.
pub(super) fn engine_error_kinds() -> BTreeSet<String> {
    let src_dir = workshop_rs_src();
    let files = read_rs_tree(&src_dir);
    // `#[cfg(test)]` code is excluded: test-only literals are not part of
    // the engine's production vocabulary, and test helpers would otherwise
    // produce unclassifiable kind arguments.
    let masked: BTreeMap<PathBuf, Vec<u8>> = files
        .iter()
        .map(|(path, text)| (path.clone(), blank_cfg_test_items(&mask_rust(text))))
        .collect();
    let mut state = KindCensus {
        kinds: BTreeSet::new(),
        problems: Vec::new(),
        visited: BTreeSet::new(),
    };

    const SITES: &[&str] = &[
        "WorkshopError::unknown(",
        "WorkshopError::unknown_with_candidates(",
        "WorkshopError::not_detected(",
        ".unknown(",
        ".unknown_at(",
        ".not_detected(",
    ];

    for (path, mask) in &masked {
        let original = &files[path];
        let text = as_str(mask, 0..mask.len()).to_string();
        for site in SITES {
            for (at, _) in text.match_indices(site) {
                let open = at + site.len() - 1;
                let Some(&(start, end)) = call_args(mask, open).first() else {
                    continue;
                };
                state.classify_arg(original[start..end].trim(), path, start, &files, &masked, 0);
            }
        }
        // Direct `WorkshopError::Unknown`-family struct construction with an
        // explicit `kind:` field. Shorthand `kind,` is not scanned: inside
        // the engine's own constructors the field binds the parameter whose
        // callers are already enumerated, and in match patterns `kind`
        // binds a variable — never a produced kind.
        for variant in ["Unknown", "UnknownWithCandidates", "NotDetected"] {
            let needle = format!("WorkshopError::{variant} {{");
            for (at, _) in text.match_indices(&needle) {
                let open = at + needle.len() - 1;
                let Some(end) = match_bracket(mask, open) else {
                    continue;
                };
                let body = &text[open..end];
                let Some(field) = body.find("kind:") else {
                    continue;
                };
                let field_start = open + field + "kind:".len();
                let field_text = &original[field_start..end];
                let literal_end = field_text.find([',', '}']).unwrap_or(field_text.len());
                state.classify_arg(
                    field_text[..literal_end].trim(),
                    path,
                    field_start,
                    &files,
                    &masked,
                    0,
                );
            }
        }
    }

    assert!(
        state.problems.is_empty(),
        "cannot enumerate every Workshop engine error kind — the census \
         stops here rather than sampling:\n  {}",
        state.problems.join("\n  ")
    );
    state.kinds
}

struct KindCensus {
    kinds: BTreeSet<String>,
    problems: Vec<String>,
    visited: BTreeSet<(PathBuf, usize, usize)>,
}

impl KindCensus {
    /// Classify one kind-producing argument: literal → record it; an
    /// identifier or `x.as_str()` → resolve through the enclosing
    /// function's parameter (`Kind` → the whole enum; `&'static str` →
    /// that function's callers, recursively); anything else → fail loudly.
    fn classify_arg(
        &mut self,
        arg: &str,
        file: &Path,
        arg_pos: usize,
        files: &BTreeMap<PathBuf, String>,
        masked: &BTreeMap<PathBuf, Vec<u8>>,
        depth: usize,
    ) {
        let file_src = &files[file];
        let file_mask = &masked[file];
        if !self
            .visited
            .insert((file.to_path_buf(), arg_pos, arg.len()))
        {
            return;
        }
        if depth > 12 {
            self.problems.push(format!(
                "{}:{}: kind resolution recursed beyond depth 12 at {arg:?}",
                file.display(),
                line_of(file_src, arg_pos)
            ));
            return;
        }
        if arg.is_empty() {
            return; // e.g. `SettingScope::unknown()` — not an error-kind site
        }
        if let Some(rest) = arg.strip_prefix('"') {
            match rest.find('"') {
                Some(end) => {
                    self.kinds.insert(rest[..end].to_string());
                }
                None => self.problems.push(format!(
                    "{}:{}: unterminated kind literal {arg:?}",
                    file.display(),
                    line_of(file_src, arg_pos)
                )),
            };
            return;
        }
        let (ident, receiver_is_as_str) = match arg.strip_suffix(".as_str()") {
            Some(receiver) => (receiver.trim(), true),
            None => (arg, false),
        };
        let is_ident = !ident.is_empty()
            && ident
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '.');
        if !is_ident {
            self.problems.push(format!(
                "{}:{}: unclassifiable kind argument {arg:?}",
                file.display(),
                line_of(file_src, arg_pos)
            ));
            return;
        }
        let leaf = ident.rsplit('.').next().unwrap_or(ident);
        let Some((fn_name, params)) = enclosing_fn(file_mask, arg_pos) else {
            self.problems.push(format!(
                "{}:{}: kind argument {arg:?} is not inside a function",
                file.display(),
                line_of(file_src, arg_pos)
            ));
            return;
        };
        let param = params.iter().position(|(name, _)| name == leaf);
        let Some(index) = param else {
            self.problems.push(format!(
                "{}:{}: kind argument {arg:?} is not a parameter of {fn_name:?}",
                file.display(),
                line_of(file_src, arg_pos)
            ));
            return;
        };
        let ty = params[index].1.as_str();
        if ty == "Kind" || ty.ends_with("::Kind") {
            for spelling in catalog_kind_strings() {
                self.kinds.insert(spelling.to_string());
            }
            return;
        }
        if ty.contains("&'static str") || ty == "&str" {
            if receiver_is_as_str {
                self.problems.push(format!(
                    "{}:{}: {arg:?} applies .as_str() to a string kind parameter",
                    file.display(),
                    line_of(file_src, arg_pos)
                ));
                return;
            }
            // The kind is forwarded through this parameter: its values are
            // whatever the enclosing function's callers pass. The shared
            // `unknown`-family helpers are called across the crate; other
            // helpers are resolved inside their own file so an unrelated
            // same-named function cannot leak into the census.
            let global = matches!(fn_name.as_str(), "unknown" | "unknown_at" | "not_detected");
            let mut callers = Vec::new();
            for (other_path, other_mask) in masked {
                if !global && other_path != file {
                    continue;
                }
                let other_text = as_str(other_mask, 0..other_mask.len());
                let needle = format!("{fn_name}(");
                for (call_at, _) in other_text.match_indices(&needle) {
                    // Skip the definition site and qualified paths that are
                    // not this helper (e.g. `NumericBounds::unknown(` — the
                    // `::` prefix is still a call shape for a free/global
                    // name, so only `fn` definitions are excluded).
                    if other_text[..call_at].ends_with("fn ") {
                        continue;
                    }
                    let open = call_at + needle.len() - 1;
                    if let Some(&(s, e)) = call_args(other_mask, open).get(index) {
                        callers.push((other_path.clone(), s, e));
                    }
                }
            }
            if callers.is_empty() {
                self.problems.push(format!(
                    "{}:{}: kind parameter {leaf:?} of {fn_name:?} has no resolvable callers",
                    file.display(),
                    line_of(file_src, arg_pos)
                ));
                return;
            }
            for (caller_path, s, e) in callers {
                let arg = files[&caller_path][s..e].trim().to_string();
                self.classify_arg(&arg, &caller_path, s, files, masked, depth + 1);
            }
            return;
        }
        self.problems.push(format!(
            "{}:{}: kind argument {arg:?} resolves to parameter {leaf:?} of \
             unsupported type {ty:?} on {fn_name:?}",
            file.display(),
            line_of(file_src, arg_pos)
        ));
    }
}
