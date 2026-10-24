use super::symbols::{RuleId, SemanticIndex, Symbol, SymbolKind};
use crate::service::ErrorInfo;
use workshop_rs::source::Span;

const CANDIDATE_LIMIT: usize = 3;

impl SemanticIndex {
    fn rule_symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols()
            .filter(|symbol| symbol.kind == SymbolKind::Rule)
    }

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

    fn ambiguous_rule(&self, address: &str, candidates: &[&Symbol]) -> ErrorInfo {
        let mut fields = serde_json::Map::new();
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
            fields,
            ..ErrorInfo::new("ambiguous-rule", format!("ambiguous rule '{address}'"))
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
                let mut fields = serde_json::Map::new();
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
                    fields,
                    ..ErrorInfo::new("ambiguous-symbol", format!("ambiguous symbol '{address}'"))
                })
            }
        }
    }
}

fn symbol_address_kind(kind: &str) -> Option<SymbolKind> {
    SymbolKind::ALL
        .iter()
        .copied()
        .find(|known| *known != SymbolKind::Rule && known.as_str() == kind)
}

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
    let mut fields = serde_json::Map::new();
    fields.insert("name".to_string(), name.into());
    let hint = if candidates.is_empty() {
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
        fields,
        ..ErrorInfo::new(code, format!("unknown {noun} '{name}'; {hint}"))
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
