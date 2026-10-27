//! Canonicalized matcher forms and the node-level matching predicates.
//!
//! `canonical_*` turns the YAML schema types into catalog-resolved
//! patterns; `public_*_matches` evaluates them against public Workshop
//! nodes, and `search_value_tree` runs the deep `values` search with
//! `within`/`in-args` ancestor constraints (#970).

use workshop_rs::catalog::{Catalog, Kind, Locale};
use workshop_rs::{Action as PublicAction, Value as PublicValue};

use super::RuleError;
use super::schema::{ActionKind, ActionPattern, CallPattern, Count, NodePattern, ValuePattern};

#[derive(Debug, Clone)]
pub(crate) struct CanonicalNodePattern {
    pub(crate) value: CanonicalValuePattern,
    pub(crate) count: Count,
}

#[derive(Debug, Clone)]
pub(crate) struct CanonicalActionPattern {
    pub(crate) kind: Option<ActionKind>,
    pub(crate) name: Option<String>,
    pub(crate) args: Vec<CanonicalValuePattern>,
    pub(crate) parameters: Vec<(usize, CanonicalValuePattern)>,
    pub(crate) count: Count,
    pub(crate) present: bool,
}

/// The canonicalized ancestor constraint of a `within` member (#970):
/// `value` must match a strict ancestor of the matched node, and when
/// `in_args` is set only an ancestor whose edge toward the match sits at a
/// child index `>= in_args` qualifies.
#[derive(Debug, Clone)]
pub(crate) struct CanonicalWithin {
    pub(crate) value: Box<CanonicalValuePattern>,
    pub(crate) in_args: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) enum CanonicalValuePattern {
    Any,
    Number(f64),
    String(String),
    Boolean(bool),
    Call {
        names: Vec<String>,
        args: Vec<CanonicalValuePattern>,
    },
    Enum {
        domain: String,
        member: String,
    },
    Comparison {
        operator: super::schema::ComparisonOperator,
        value: Box<CanonicalValuePattern>,
    },
    Within {
        value: Box<CanonicalValuePattern>,
        within: CanonicalWithin,
    },
}

/// Search one value tree at any depth for nodes matching any `values`
/// pattern, reporting `(path, node)` per match (#970). `ancestors` holds the
/// `(node, edge index)` pairs on the path to the current node; a `within`
/// constraint matches when some strict ancestor matches its pattern and the
/// edge below it satisfies `in-args.from`.
pub(crate) fn search_value_tree<'a>(
    value: &'a PublicValue,
    ancestors: &mut Vec<(&'a PublicValue, usize)>,
    patterns: &[CanonicalValuePattern],
    path: &mut Vec<usize>,
    out: &mut impl FnMut(&[usize], &'a PublicValue),
) {
    for pattern in patterns {
        if pattern_matches(value, pattern, ancestors) {
            out(path, value);
        }
    }
    for (index, child) in crate::canonical::value_children(value)
        .into_iter()
        .enumerate()
    {
        path.push(index);
        ancestors.push((value, index));
        search_value_tree(child, ancestors, patterns, path, out);
        ancestors.pop();
        path.pop();
    }
}

fn pattern_matches(
    value: &PublicValue,
    pattern: &CanonicalValuePattern,
    ancestors: &[(&PublicValue, usize)],
) -> bool {
    match pattern {
        CanonicalValuePattern::Within {
            value: inner,
            within,
        } => {
            public_value_matches(value, inner)
                && ancestors.iter().any(|(ancestor, edge)| {
                    public_value_matches(ancestor, &within.value)
                        && within.in_args.is_none_or(|from| *edge >= from)
                })
        }
        _ => public_value_matches(value, pattern),
    }
}

pub(crate) fn public_action_matches(
    action: &PublicAction,
    pattern: &CanonicalActionPattern,
) -> bool {
    let kind_matches = match pattern.kind {
        None | Some(ActionKind::Any) => true,
        Some(ActionKind::Call) => matches!(action, PublicAction::Call { .. }),
        Some(ActionKind::While) => matches!(action, PublicAction::While { .. }),
        Some(ActionKind::ForGlobalVariable) => {
            matches!(action, PublicAction::ForGlobalVariable { .. })
        }
        Some(ActionKind::ForPlayerVariable) => {
            matches!(action, PublicAction::ForPlayerVariable { .. })
        }
        Some(ActionKind::If) => matches!(action, PublicAction::If { .. }),
    };
    if !kind_matches {
        return false;
    }
    let PublicAction::Call { name, args } = action else {
        return pattern.name.is_none() && pattern.args.is_empty();
    };
    if pattern
        .name
        .as_deref()
        .is_some_and(|expected| expected != name)
    {
        return false;
    }
    pattern.args.iter().enumerate().all(|(index, expected)| {
        args.get(index)
            .is_some_and(|actual| public_value_matches(actual, expected))
    }) && pattern.parameters.iter().all(|(index, expected)| {
        args.get(*index)
            .is_some_and(|actual| public_value_matches(actual, expected))
    })
}

pub(crate) fn public_value_matches(value: &PublicValue, pattern: &CanonicalValuePattern) -> bool {
    use super::schema::ComparisonOperator as Op;
    match (pattern, value) {
        (CanonicalValuePattern::Any, _) => true,
        (CanonicalValuePattern::Number(expected), PublicValue::Number(actual)) => {
            expected == actual
        }
        (CanonicalValuePattern::String(expected), PublicValue::String(actual))
        | (CanonicalValuePattern::String(expected), PublicValue::LocalizedString(actual)) => {
            expected == actual
        }
        (CanonicalValuePattern::Boolean(expected), PublicValue::Bool(actual)) => expected == actual,
        (
            CanonicalValuePattern::Enum { domain, member },
            PublicValue::Enum { value_type, value },
        ) => domain == value_type && member == value,
        (
            CanonicalValuePattern::Call { names, args },
            PublicValue::Call {
                name: actual,
                args: actual_args,
            },
        ) => {
            names.iter().any(|name| name == actual)
                && args.len() <= actual_args.len()
                && args
                    .iter()
                    .enumerate()
                    .all(|(index, expected)| public_value_matches(&actual_args[index], expected))
        }
        (CanonicalValuePattern::Comparison { operator, value }, PublicValue::Number(actual)) => {
            let CanonicalValuePattern::Number(expected) = value.as_ref() else {
                return false;
            };
            match operator {
                Op::Less => *actual < *expected,
                Op::LessOrEqual => *actual <= *expected,
                Op::Greater => *actual > *expected,
                Op::GreaterOrEqual => *actual >= *expected,
            }
        }
        // A `within` constraint only has meaning under the `values` search,
        // which supplies the ancestor chain. Outside it the inner pattern
        // decides on its own.
        (CanonicalValuePattern::Within { value: inner, .. }, _) => {
            public_value_matches(value, inner)
        }
        _ => false,
    }
}

pub(crate) fn canonical_node(
    pattern: NodePattern,
    catalog: &Catalog,
    locale: &Locale,
) -> Result<CanonicalNodePattern, RuleError> {
    Ok(CanonicalNodePattern {
        value: canonical_value(pattern.value, catalog, locale, false)?,
        count: pattern.count,
    })
}

pub(crate) fn canonical_action(
    pattern: ActionPattern,
    catalog: &Catalog,
    locale: &Locale,
) -> Result<CanonicalActionPattern, RuleError> {
    let ActionPattern {
        kind,
        name,
        args,
        parameters: named_parameters,
        count,
        present,
    } = pattern;
    let name = name
        .as_deref()
        .map(|name| resolve(catalog, Kind::Action, locale, name))
        .transpose()?;
    let action_entry = name
        .as_deref()
        .and_then(|id| catalog.entry(Kind::Action, id));
    let mut parameters = Vec::new();
    for param in named_parameters {
        let Some(entry) = action_entry else {
            return Err(RuleError::NamedParameterRequiresAction);
        };
        let Some(index) = resolve_parameter(entry, locale, &param.name) else {
            return Err(RuleError::UnknownParameter {
                action: entry.id.clone(),
                parameter: param.name,
                locale: locale.to_string(),
            });
        };
        parameters.push((index, canonical_value(param.value, catalog, locale, false)?));
    }
    Ok(CanonicalActionPattern {
        kind,
        name,
        args: args
            .into_iter()
            .map(|value| canonical_value(value, catalog, locale, false))
            .collect::<Result<_, _>>()?,
        parameters,
        count,
        present,
    })
}

fn resolve_parameter(
    action: &workshop_rs::catalog::CatalogEntry,
    locale: &Locale,
    spelling: &str,
) -> Option<usize> {
    action.resolve_param(locale, spelling)
}

pub(crate) fn canonical_value(
    pattern: ValuePattern,
    catalog: &Catalog,
    locale: &Locale,
    within_allowed: bool,
) -> Result<CanonicalValuePattern, RuleError> {
    let selected = usize::from(pattern.number.is_some())
        + usize::from(pattern.string.is_some())
        + usize::from(pattern.boolean.is_some())
        + usize::from(pattern.call.is_some())
        + usize::from(pattern.enum_value.is_some())
        + usize::from(pattern.comparison.is_some());
    if selected > 1 {
        return Err(RuleError::AmbiguousValuePattern);
    }
    let within = pattern
        .within
        .map(|within| {
            if !within_allowed {
                return Err(RuleError::WithinOutsideValues);
            }
            Ok::<CanonicalWithin, RuleError>(CanonicalWithin {
                value: Box::new(canonical_value(within.value, catalog, locale, false)?),
                in_args: within.in_args.map(|in_args| in_args.from),
            })
        })
        .transpose()?;
    let base = if let Some(number) = pattern.number {
        CanonicalValuePattern::Number(number)
    } else if let Some(string) = pattern.string {
        CanonicalValuePattern::String(string)
    } else if let Some(boolean) = pattern.boolean {
        CanonicalValuePattern::Boolean(boolean)
    } else if let Some(call) = pattern.call {
        canonical_call(call, catalog, locale)?
    } else if let Some(en) = pattern.enum_value {
        let domain = resolve_enum_domain(catalog, locale, &en.domain)?;
        let member = catalog
            .resolve_enum_member(&domain, locale, &en.member)
            .ok_or_else(|| RuleError::UnknownSpelling {
                kind: "enum member",
                spelling: en.member,
                locale: locale.to_string(),
            })?
            .1;
        CanonicalValuePattern::Enum { domain, member }
    } else if let Some(comparison) = pattern.comparison {
        let value = canonical_value(*comparison.value, catalog, locale, false)?;
        if !matches!(value, CanonicalValuePattern::Number(_)) {
            return Err(RuleError::ComparisonNeedsNumber);
        }
        CanonicalValuePattern::Comparison {
            operator: comparison.operator,
            value: Box::new(value),
        }
    } else {
        CanonicalValuePattern::Any
    };
    Ok(match within {
        Some(within) => CanonicalValuePattern::Within {
            value: Box::new(base),
            within,
        },
        None => base,
    })
}

fn canonical_call(
    call: CallPattern,
    catalog: &Catalog,
    locale: &Locale,
) -> Result<CanonicalValuePattern, RuleError> {
    let names = match (call.name, call.names) {
        (Some(_), Some(_)) => return Err(RuleError::AmbiguousCallName),
        (Some(name), None) => vec![name],
        (None, Some(names)) if !names.is_empty() => names,
        (None, _) => return Err(RuleError::CallNeedsName),
    };
    let names = names
        .iter()
        .map(|name| resolve_value_or_operator(catalog, locale, name))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CanonicalValuePattern::Call {
        names,
        args: call
            .args
            .into_iter()
            .map(|value| canonical_value(value, catalog, locale, false))
            .collect::<Result<_, _>>()?,
    })
}

fn resolve_value_or_operator(
    catalog: &Catalog,
    locale: &Locale,
    spelling: &str,
) -> Result<String, RuleError> {
    resolve(catalog, Kind::Value, locale, spelling).or_else(|value_error| {
        resolve(catalog, Kind::Operator, locale, spelling).or(Err(value_error))
    })
}

fn resolve(
    catalog: &Catalog,
    kind: Kind,
    locale: &Locale,
    spelling: &str,
) -> Result<String, RuleError> {
    catalog
        .entry(kind, spelling)
        .map(|entry| entry.id.clone())
        .or_else(|| {
            catalog
                .resolve(kind, locale, spelling)
                .map(|entry| entry.id.clone())
        })
        .ok_or_else(|| RuleError::UnknownSpelling {
            kind: kind.as_str(),
            spelling: spelling.to_string(),
            locale: locale.to_string(),
        })
}

fn resolve_enum_domain(
    catalog: &Catalog,
    locale: &Locale,
    spelling: &str,
) -> Result<String, RuleError> {
    catalog
        .enum_domain(spelling)
        .map(|_| spelling.to_string())
        .or_else(|| {
            catalog
                .resolve_enum_domain(locale, spelling)
                .map(str::to_string)
        })
        .ok_or_else(|| RuleError::UnknownSpelling {
            kind: "enum domain",
            spelling: spelling.to_string(),
            locale: locale.to_string(),
        })
}
