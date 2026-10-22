//! Name-address resolution over the semantic index (ADR-0035), with
//! nearest-name candidates on a miss (#567).
//!
//! A rule address is `NAME` (the declared rule name) or `NAME#N` (the Nth
//! rule, counted from 1 in program order, among the rules named `NAME`; an
//! unnamed rule is `#N`). A symbol address is `NAME` or `KIND:NAME`, where
//! `KIND` is a non-rule value of `inspect symbols --only`; symbol
//! resolution excludes rules — a rule never competes with a variable or
//! subroutine for a name. Resolution never guesses: an inexact address is
//! refused, and a refusal names the nearest declared names or the
//! candidates' own selectable addresses so the caller can retry.

use super::symbols::{RuleId, SemanticIndex, Symbol, SymbolKind};
use crate::service::ErrorInfo;
use workshop_rs::source::Span;

const CANDIDATE_LIMIT: usize = 3;

impl SemanticIndex {
    /// Rule symbols in program order — the order `inspect rules` lists
    /// (ADR-0035): each carries its rule index in `rule`.
    fn rule_symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols()
            .filter(|symbol| symbol.kind == SymbolKind::Rule)
    }

    /// The `N`th rule named `name` in program order, counted from 1.
    fn nth_rule_named(&self, name: &str, ordinal: usize) -> Option<&Symbol> {
        self.rule_symbols()
            .filter(|symbol| symbol.name == name)
            .nth(ordinal - 1)
    }

    /// The rule's canonical address (ADR-0035): `NAME` when the name is
    /// unique and not also a valid `NAME#N` of another rule, otherwise the
    /// rule's own `NAME#N` ordinal. In the rare case where that string
    /// still resolves to more than one rule — a literal `a#2` beside two
    /// `a` rules makes the second `a` unaddressable — the rule has no
    /// name address: `NAME#N` is still printed, and passing it back fails
    /// ambiguous rather than selecting a different rule.
    pub fn canonical_rule_address(&self, rule: RuleId) -> String {
        let symbol = self
            .rule_symbols()
            .find(|symbol| symbol.rule == Some(rule))
            .expect("a rule index addresses a rule symbol");
        let name = symbol.name.as_str();
        let unique = self
            .rule_symbols()
            .filter(|symbol| symbol.name == name)
            .count()
            == 1;
        // `NAME` alone is usable only when its `NAME#N` reading does not
        // select a different rule: a lone `a#2` literal collides with the
        // second `a`, and an empty name has no bare form at all (`#N`).
        let collides = ordinal_form(name)
            .and_then(|(base, ordinal)| self.nth_rule_named(base, ordinal))
            .is_some_and(|other| other.rule != Some(rule));
        if unique && !collides && !name.is_empty() {
            return name.to_string();
        }
        let ordinal = self
            .rule_symbols()
            .filter(|symbol| symbol.name == name)
            .position(|symbol| symbol.rule == Some(rule))
            .expect("a rule is among the rules carrying its name")
            + 1;
        format!("{name}#{ordinal}")
    }

    /// Resolve a rule address — `NAME` or `NAME#N` — to its rule index
    /// (ADR-0035). Candidates are the rules literally named the string
    /// plus, when it ends in `#N`, the `N`th rule named by the part before
    /// the last `#`; exactly one resolves, none is `unknown-rule`, and
    /// more than one is `ambiguous-rule` listing each candidate's
    /// canonical address and location. Only rule symbols participate: a
    /// variable or subroutine sharing the name stays out of the grammar.
    pub fn resolve_rule(&self, address: &str) -> Result<RuleId, ErrorInfo> {
        let mut candidates: Vec<&Symbol> = self
            .rule_symbols()
            .filter(|symbol| symbol.name == address)
            .collect();
        if let Some((name, ordinal)) = ordinal_form(address)
            && let Some(rule) = self.nth_rule_named(name, ordinal)
        {
            // The two readings never name the same rule: `name` is a
            // strict prefix of `address`, so no rule carries both.
            candidates.push(rule);
        }
        match candidates.as_slice() {
            [symbol] => Ok(symbol.rule.expect("rule symbols carry their rule index")),
            [] => Err(unknown(
                "unknown-rule",
                "rule",
                address,
                self.rule_symbols(),
                "rules",
            )),
            _ => Err(self.ambiguous_rule(address, &candidates)),
        }
    }

    /// The `ambiguous-rule` refusal: each candidate's canonical address
    /// and location (ADR-0035). When a candidate's own canonical address
    /// does not resolve back to it — the collision is total — the error
    /// says so: renaming one of the conflicting rules is the only way to
    /// select it.
    fn ambiguous_rule(&self, address: &str, candidates: &[&Symbol]) -> ErrorInfo {
        let mut fields = error_fields();
        fields.insert("name".to_string(), address.into());
        fields.insert(
            "candidates".to_string(),
            candidates
                .iter()
                .map(|rule| {
                    let id = rule.rule.expect("rule symbol");
                    candidate(self.canonical_rule_address(id), rule.span)
                })
                .collect(),
        );
        // The address-grammar candidates a string resolves to, without the
        // error branch: `Some(id)` when it selects exactly `id`. Calling
        // `resolve_rule` here would recurse into `ambiguous_rule` forever
        // when a candidate's canonical address collides totally.
        let resolves_to = |address: &str, id: RuleId| {
            let mut hits: Vec<RuleId> = self
                .rule_symbols()
                .filter(|symbol| symbol.name == address)
                .filter_map(|symbol| symbol.rule)
                .collect();
            if let Some((name, ordinal)) = ordinal_form(address)
                && let Some(rule) = self.nth_rule_named(name, ordinal)
                && let Some(rule) = rule.rule
            {
                hits.push(rule);
            }
            hits == [id]
        };
        let unaddressable = candidates.iter().any(|rule| {
            let id = rule.rule.expect("rule symbol");
            !resolves_to(&self.canonical_rule_address(id), id)
        });
        if unaddressable {
            fields.insert("unaddressable".to_string(), true.into());
        }
        ErrorInfo {
            code: "ambiguous-rule".to_string(),
            message: format!("ambiguous rule '{address}'"),
            fields,
        }
    }

    /// Resolve a symbol address — `NAME` or `KIND:NAME` — to its symbol
    /// (ADR-0035). Only non-rule symbols participate: `inspect refs` never
    /// resolves a rule, so a rule named `x` neither collides with nor
    /// answers for a variable `x`. `KIND` is a non-rule
    /// `inspect symbols --only` value (`globalVariable`, `playerVariable`,
    /// `subroutine`); an unrecognized prefix leaves the whole string as
    /// the name, and declared names cannot contain `:` anyway.
    /// `unknown-symbol` names the nearest declared names or the `symbols`
    /// operation; `ambiguous-symbol` lists each candidate's `KIND:NAME`
    /// form and location.
    pub fn resolve_symbol(&self, address: &str) -> Result<&Symbol, ErrorInfo> {
        let (kind, name) = match address.split_once(':') {
            Some((prefix, name)) => match symbol_address_kind(prefix) {
                Some(kind) => (Some(kind), name),
                None => (None, address),
            },
            None => (None, address),
        };
        let declared = || {
            self.symbols()
                .filter(|symbol| symbol.kind != SymbolKind::Rule)
                .filter(|symbol| kind.is_none_or(|kind| symbol.kind == kind))
        };
        let matches: Vec<&Symbol> = declared().filter(|symbol| symbol.name == name).collect();
        match matches.as_slice() {
            [symbol] => Ok(symbol),
            [] => Err(unknown(
                "unknown-symbol",
                "symbol",
                address,
                declared(),
                "symbols",
            )),
            _ => {
                let mut fields = error_fields();
                fields.insert("name".to_string(), address.into());
                fields.insert(
                    "candidates".to_string(),
                    matches
                        .iter()
                        .map(|symbol| {
                            candidate(
                                format!("{}:{}", symbol.kind.as_str(), symbol.name),
                                symbol.span,
                            )
                        })
                        .collect(),
                );
                Err(ErrorInfo {
                    code: "ambiguous-symbol".to_string(),
                    message: format!("ambiguous symbol '{address}'"),
                    fields,
                })
            }
        }
    }
}

/// The `KIND` of a `KIND:NAME` symbol address: the non-rule values of
/// `inspect symbols --only` (ADR-0035). `rule` is not a symbol-address
/// kind — rules are addressed with rule addresses.
fn symbol_address_kind(kind: &str) -> Option<SymbolKind> {
    SymbolKind::ALL
        .iter()
        .copied()
        .find(|known| *known != SymbolKind::Rule && known.as_str() == kind)
}

/// The `NAME#N` ordinal form of a rule address: `N` is a positive decimal
/// integer counted from 1 with no leading zero, taken after the last `#`.
/// A trailing `#` or a malformed `N` leaves the whole string a literal
/// name — a rule named `a#2x` is still `a#2x`.
fn ordinal_form(address: &str) -> Option<(&str, usize)> {
    let (name, digits) = address.rsplit_once('#')?;
    if digits.is_empty()
        || digits.starts_with('0')
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let ordinal: usize = digits.parse().ok()?;
    Some((name, ordinal))
}

/// A candidate's structured location (`candidates` entry, #888): the
/// canonical address a retry passes back plus the file-table index,
/// line, and column each surface resolves to its own `path:line:col`
/// spelling — the analyzer knows only file ids, never the spellings.
fn candidate(address: String, span: Option<Span>) -> serde_json::Value {
    let mut entry = serde_json::Map::new();
    entry.insert("address".to_string(), address.into());
    if let Some(span) = span {
        entry.insert("file".to_string(), span.file.index().into());
        entry.insert("line".to_string(), span.start.line.into());
        entry.insert("column".to_string(), span.start.col.into());
    }
    entry.into()
}

/// The refusal-metadata map [`ErrorInfo::fields`] carries (#888).
fn error_fields() -> serde_json::Map<String, serde_json::Value> {
    serde_json::Map::new()
}

/// How many declared names an unknown-name error lists (#888); beyond the
/// bound the error reports the total and names the unbounded listing.
const NAME_LIST_LIMIT: usize = 20;

fn unknown<'a>(
    code: &str,
    noun: &str,
    name: &str,
    declared: impl Iterator<Item = &'a Symbol>,
    operation: &str,
) -> ErrorInfo {
    let declared: Vec<&'a Symbol> = declared.collect();
    let candidates = nearest(name, declared.iter().map(|symbol| symbol.name.as_str()));
    let mut fields = error_fields();
    fields.insert("name".to_string(), name.into());
    let hint = if candidates.is_empty() {
        // No near match: name the declared names — each once, in
        // declaration order, bounded with the total (#888) — as `declared`
        // fields and in the message, and let each surface append its own
        // spelling of the `listOp` operation that lists them.
        let mut seen = std::collections::HashSet::new();
        let names: Vec<&str> = declared
            .iter()
            .filter(|symbol| seen.insert(symbol.name.as_str()))
            .map(|symbol| symbol.name.as_str())
            .collect();
        fields.insert("listOp".to_string(), operation.into());
        if names.is_empty() {
            format!("no declared {noun} is near")
        } else {
            let over = names.len() > NAME_LIST_LIMIT;
            if over {
                fields.insert("listAll".to_string(), true.into());
            }
            fields.insert("declaredTotal".to_string(), names.len().into());
            let listed: Vec<&str> = names.iter().take(NAME_LIST_LIMIT).copied().collect();
            fields.insert(
                "declared".to_string(),
                serde_json::Value::Array(
                    listed
                        .iter()
                        .map(|name| serde_json::Value::String((*name).to_string()))
                        .collect(),
                ),
            );
            let mut spelled: Vec<String> = listed.iter().map(|name| format!("'{name}'")).collect();
            if over {
                spelled.push("…".to_string());
            }
            format!(
                "no declared {noun} is near; declared {noun} names ({} total): {}",
                names.len(),
                spelled.join(", "),
            )
        }
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
        fields,
    }
}

fn nearest<'a>(name: &str, declared: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let name = name.to_lowercase();
    let max = if name.chars().count() >= 6 { 2 } else { 1 };
    let mut ranked: Vec<(usize, &str)> = Vec::new();
    for candidate in declared {
        if ranked.iter().any(|(_, seen)| *seen == candidate) {
            continue;
        }
        let distance = strsim::levenshtein(&name, &candidate.to_lowercase());
        if distance <= max {
            ranked.push((distance, candidate));
        }
    }
    ranked.sort_by_key(|(distance, _)| *distance);
    ranked.truncate(CANDIDATE_LIMIT);
    ranked.into_iter().map(|(_, candidate)| candidate).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_ranks_by_case_insensitive_distance_within_the_window() {
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

    #[test]
    fn ordinal_form_requires_a_positive_decimal_without_leading_zero() {
        assert_eq!(ordinal_form("a#2"), Some(("a", 2)));
        assert_eq!(ordinal_form("#1"), Some(("", 1)));
        assert_eq!(ordinal_form("a#b#3"), Some(("a#b", 3)));
        for malformed in ["a#", "a#0", "a#02", "a#2x", "a#-1", "a#1_0"] {
            assert_eq!(ordinal_form(malformed), None, "{malformed}");
        }
    }
}
