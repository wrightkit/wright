//! Exact name resolution over the semantic index (#429), with nearest-name
//! candidates on a miss (#567).
//!
//! Resolution never guesses: an inexact name is refused. A refusal names the
//! nearest declared names so the caller can retry, using the closeness rules
//! `workshop-rs` applies to its unknown-spelling diagnostics.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use super::symbols::{RuleId, SemanticIndex, Symbol, SymbolKind};
use crate::service::ErrorInfo;

/// The most candidates a refusal names.
const CANDIDATE_LIMIT: usize = 3;

impl SemanticIndex {
    /// Resolve a symbol by its exact declared name. A name matching no
    /// symbol is `unknown-symbol`, naming the nearest declared names or the
    /// `symbols` operation; a name shared by more than one symbol is
    /// `ambiguous-symbol` with each candidate's kind and numeric id.
    pub fn resolve_symbol(&self, name: &str) -> Result<&Symbol, ErrorInfo> {
        let matches: Vec<&Symbol> = self
            .symbols()
            .filter(|symbol| symbol.name == name)
            .collect();
        match matches.as_slice() {
            [symbol] => Ok(symbol),
            [] => Err(unknown(
                "unknown-symbol",
                "symbol",
                name,
                self.symbols(),
                "symbols",
            )),
            _ => Err(ErrorInfo {
                code: "ambiguous-symbol".to_string(),
                message: format!(
                    "ambiguous symbol '{name}': {}",
                    matches
                        .iter()
                        .map(|symbol| format!("{} {}", symbol.kind.as_str(), symbol.id.index()))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }),
        }
    }

    /// Resolve a rule by its exact declared name to its rule index. Only rule
    /// symbols participate: a rule named `x` stays reachable even when a
    /// variable or subroutine is also named `x`. Unmatched and duplicate rule
    /// names are `unknown-rule`/`ambiguous-rule`.
    pub fn resolve_rule(&self, name: &str) -> Result<RuleId, ErrorInfo> {
        let rules = || {
            self.symbols()
                .filter(|symbol| symbol.kind == SymbolKind::Rule)
        };
        let matches: Vec<&Symbol> = rules().filter(|symbol| symbol.name == name).collect();
        match matches.as_slice() {
            [symbol] => Ok(symbol.rule.expect("rule symbols carry their rule index")),
            [] => Err(unknown("unknown-rule", "rule", name, rules(), "rules")),
            _ => Err(ErrorInfo {
                code: "ambiguous-rule".to_string(),
                message: format!(
                    "ambiguous rule '{name}': {}",
                    matches
                        .iter()
                        .map(|symbol| format!("rule {}", symbol.rule.expect("rule symbol")))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }),
        }
    }
}

fn unknown<'a>(
    code: &str,
    noun: &str,
    name: &str,
    declared: impl Iterator<Item = &'a Symbol>,
    operation: &str,
) -> ErrorInfo {
    let candidates = nearest(name, declared.map(|symbol| symbol.name.as_str()));
    let hint = if candidates.is_empty() {
        format!("no declared {noun} is near; the '{operation}' operation lists them")
    } else {
        format!(
            "nearest declared: {}",
            candidates
                .iter()
                .map(|candidate| format!("'{candidate}'"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    ErrorInfo {
        code: code.to_string(),
        message: format!("unknown {noun} '{name}'; {hint}"),
    }
}

/// Up to [`CANDIDATE_LIMIT`] distinct names nearest to `name`, ranked by
/// folded edit distance, ties in declaration order. A name of fewer than six
/// folded characters admits one edit, a longer one two.
fn nearest<'a>(name: &str, declared: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let folded = fold(name);
    let max = if folded.chars().count() >= 6 { 2 } else { 1 };
    let mut ranked: Vec<(usize, &str)> = Vec::new();
    for candidate in declared {
        if ranked.iter().any(|(_, seen)| *seen == candidate) {
            continue;
        }
        let distance = strsim::levenshtein(&folded, &fold(candidate));
        if distance <= max {
            ranked.push((distance, candidate));
        }
    }
    ranked.sort_by_key(|(distance, _)| *distance);
    ranked.truncate(CANDIDATE_LIMIT);
    ranked.into_iter().map(|(_, candidate)| candidate).collect()
}

/// Comparison fold: accents and case are insignificant.
fn fold(name: &str) -> String {
    name.nfd()
        .filter(|character| !is_combining_mark(*character))
        .collect::<String>()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_ranks_by_folded_distance_within_the_window() {
        let declared = [
            "progressionDeathCounter",
            "Progression",
            "deathCount",
            "zzz",
        ];
        assert_eq!(
            nearest("progressionDeathCounte", declared.into_iter()),
            ["progressionDeathCounter"]
        );
        // Case and accents fold away; short names admit one edit only.
        assert_eq!(nearest("ZZ", declared.into_iter()), ["zzz"]);
        assert!(nearest("zz_x", declared.into_iter()).is_empty());
        assert_eq!(
            nearest("prógression", declared.into_iter()),
            ["Progression"]
        );
    }

    #[test]
    fn nearest_keeps_declaration_order_on_ties_and_names_each_once() {
        let declared = ["abcdef1", "abcdef2", "abcdef1", "abcdef3", "abcdef4"];
        assert_eq!(
            nearest("abcdef", declared.into_iter()),
            ["abcdef1", "abcdef2", "abcdef3"]
        );
    }
}
