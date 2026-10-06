//! `lookup` (ADR-0021, #529): resolve a free-text name guess — a display
//! name, a near spelling, or a guess — to the owner's accepted vocabulary:
//! spellings, enum domains and members, settings keys, and callable
//! signatures.
//!
//! `workshop` answers from the in-process `workshop-rs` catalog; `opy`
//! answers through the LPP 1.5 `lpp/lookup` capability of the configured
//! provider, reporting an explicit `unavailable` result (never a text
//! fallback) when the capability is absent. The operation consults language
//! vocabulary only: it neither requires nor triggers a project load, so it
//! answers in a session whose configured project cannot load.
//!
//! Owners match and rank and supply facts; Wright renders the `signature`
//! string from those facts under its own presentation rules (ADR-0021
//! decision 4). Wright performs no matching of its own — a scoped `within`
//! query is the owner's ranked matches intersected with the scope's
//! children — stores no names, and does not translate between languages.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{Value, json};
use workshop_rs::catalog::{Catalog, Kind, Locale};
use workshop_rs::lookup::LookupMatch;
use workshop_rs::settings::{self, SettingDefinition, SettingValueDomain};
use wright_lpp::{Capability, LookupParams, LookupWithin};

use crate::CompilerSession;
use crate::opy_provider;
use crate::service::{SERVICE_NAME, SERVICE_VERSION, ToolErrorInfo, ToolResponse};

/// `lookup` without `limit` returns at most this many entries (ADR-0021).
const DEFAULT_LIMIT: usize = 3;
/// `lookup` never returns more than this many entries (ADR-0021).
const MAX_LIMIT: usize = 10;
/// A required enum parameter lists its domain members inline in a rendered
/// signature when the domain has at most this many members, once per entry;
/// a larger domain reports its member count (ADR-0021).
const INLINE_MEMBER_LIMIT: usize = 32;

const OWNER_WORKSHOP: &str = "workshop-rs";
const OWNER_OPY: &str = "opy-rs";
const LOOKUP_CAPABILITY: &str = "lookup";
/// The LPP refusal code an owner returns when `within` names no known
/// scope; Wright's `within` identity tries each selector kind in order and
/// this refusal is what moves resolution to the next kind.
const UNKNOWN_WITHIN: &str = "lookup.unknownWithin";
/// The `within` selector kinds tried for an identity, in order.
const WITHIN_KINDS: [&str; 3] = ["enum", "callable", "settings"];

/// Run one `lookup` request against `session`'s language vocabulary.
pub(crate) fn lookup(
    session: &CompilerSession,
    language: &str,
    query: Option<&str>,
    kind: Option<&str>,
    within: Option<&str>,
    locale: Option<&str>,
    limit: Option<usize>,
) -> ToolResponse {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    if let Some(kind) = kind {
        if !matches!(
            kind,
            "action" | "value" | "event" | "enumMember" | "setting"
        ) {
            return refusal(
                "invalid-kind",
                format!(
                    "kind must be one of 'action', 'value', 'event', 'enumMember', 'setting'; got '{kind}'"
                ),
            );
        }
    }
    match language {
        "workshop" => workshop_lookup(session, query, kind, within, locale, limit),
        "opy" => opy_lookup(session, query, kind, within, locale, limit),
        other => refusal(
            "invalid-language",
            format!("lookup serves 'workshop' and 'opy'; got '{other}'"),
        ),
    }
}

fn refusal(code: &str, message: impl Into<String>) -> ToolResponse {
    ToolResponse::Error {
        error: ToolErrorInfo {
            code: code.to_string(),
            message: message.into(),
        },
    }
}

/// The shared result shape: the queried language, the answering owner named
/// once, the ranked entries, and `unavailable` when the owner lacks the
/// capability (ADR-0021 decision 3).
fn result(language: &str, owner: &str, entries: Vec<Value>) -> ToolResponse {
    ToolResponse::Ok {
        result: json!({
            "language": language,
            "owner": owner,
            "entries": entries,
        }),
    }
}

fn unavailable(language: &str, owner: &str, message: impl Into<String>) -> ToolResponse {
    ToolResponse::Ok {
        result: json!({
            "language": language,
            "owner": owner,
            "entries": [],
            "unavailable": {
                "capability": LOOKUP_CAPABILITY,
                "message": message.into(),
            },
        }),
    }
}

// -- workshop --------------------------------------------------------------

fn workshop_lookup(
    session: &CompilerSession,
    query: Option<&str>,
    kind: Option<&str>,
    within: Option<&str>,
    locale: Option<&str>,
    limit: usize,
) -> ToolResponse {
    let catalog = session.catalog();
    let Some(locale) = workshop_locale(session, catalog, locale) else {
        return refusal(
            "unsupported-locale",
            format!(
                "locale '{}' is not declared by the Workshop catalog",
                locale.unwrap_or_default()
            ),
        );
    };
    let entries = match within {
        Some(within) => match workshop_within(catalog, &locale, within, query, kind) {
            Ok(entries) => entries.into_iter().take(limit).collect(),
            Err(error) => return error,
        },
        None => {
            let matches = match catalog.lookup(&locale, query.unwrap_or("")) {
                Ok(matches) => matches,
                Err(error) => return refusal("lookup-failed", error.to_string()),
            };
            matches
                .into_iter()
                .filter_map(|found| workshop_entry(catalog, &locale, found))
                .filter(|entry| kind.is_none_or(|k| entry["kind"] == k))
                .take(limit)
                .collect()
        }
    };
    result("workshop", OWNER_WORKSHOP, entries)
}

/// The effective locale: the request's, then the session's configured
/// override, then the catalog primary locale. `None` when the chosen locale
/// is not declared — the only query the owner cannot answer.
fn workshop_locale(
    session: &CompilerSession,
    catalog: &Catalog,
    locale: Option<&str>,
) -> Option<Locale> {
    let locale = locale
        .map(Locale::new)
        .or_else(|| session.config.locale.as_deref().map(Locale::new))
        .unwrap_or_else(|| catalog.primary_locale().clone());
    catalog.supports(&locale).then_some(locale)
}

/// The owner-issued identity of one `lookup` match — the `identity` an
/// entry carries and the key a scoped query intersects on.
fn match_identity(found: &LookupMatch) -> Option<String> {
    match found {
        LookupMatch::Builtin { id, .. } => Some(id.clone()),
        LookupMatch::EnumMember { domain, member, .. } => Some(format!("{domain}.{member}")),
        LookupMatch::EnumDomain { domain, .. } => Some(domain.clone()),
        LookupMatch::Setting { definition, .. } => Some(definition.path().to_string()),
        _ => None,
    }
}

/// One `lookup` match projected to the contract entry: owner identity and
/// kind, the accepted spelling, the display name, and a Wright-rendered
/// `signature` for callables (plus the structured `callable`/`enum`/
/// `setting` facts the owner supplies).
fn workshop_entry(catalog: &Catalog, locale: &Locale, found: LookupMatch) -> Option<Value> {
    let identity = match_identity(&found)?;
    Some(match found {
        LookupMatch::Builtin {
            kind,
            display_name,
            signature,
            ..
        } => {
            let head = display_name.unwrap_or_else(|| identity.clone());
            let mut entry = json!({
                "identity": identity,
                "kind": kind.as_str(),
                "spelling": head.clone(),
                "displayName": head,
            });
            if let Some(signature) = signature {
                entry["callable"] = callable_facts(&signature);
                entry["signature"] = json!(render_signature(
                    &head,
                    None,
                    signature.variadic,
                    signature
                        .params
                        .iter()
                        .enumerate()
                        .map(|(index, param)| {
                            workshop_param(catalog, locale, &signature, index, param)
                        })
                        .collect::<Vec<_>>()
                ));
            }
            entry
        }
        LookupMatch::EnumMember {
            member,
            display_name,
            ..
        } => {
            let display = display_name.unwrap_or_else(|| member.clone());
            json!({
                "identity": identity,
                "kind": "enumMember",
                "spelling": display.clone(),
                "displayName": display,
            })
        }
        LookupMatch::EnumDomain {
            domain,
            display_name,
            members,
        } => {
            let display = display_name.unwrap_or_else(|| domain.clone());
            json!({
                "identity": identity,
                "kind": "enum",
                "spelling": display.clone(),
                "displayName": display,
                "enum": {
                    "domain": domain,
                    "members": members
                        .iter()
                        .map(|member| {
                            member
                                .display_name
                                .clone()
                                .unwrap_or_else(|| member.id.clone())
                        })
                        .collect::<Vec<_>>(),
                },
            })
        }
        LookupMatch::Setting {
            definition,
            display_name,
        } => json!({
            "identity": identity,
            "kind": "setting",
            "spelling": identity,
            "displayName": display_name,
            "setting": setting_facts(&definition),
        }),
        // A `LookupMatch` variant added after this Wright: unknown entries
        // drop out of the result rather than fail the request.
        _ => return None,
    })
}

/// The `callable` fact object for a Workshop callable: the ordered
/// parameter facts the signature renders from (`name-lookup.md` §21.4).
fn callable_facts(signature: &workshop_rs::lookup::Signature) -> Value {
    let parameters = signature
        .params
        .iter()
        .enumerate()
        .map(|(index, param)| {
            let mut wire = json!({
                "name": param.name,
                "type": param.param_type.clone().unwrap_or_else(|| "any".to_string()),
                "required": index < signature.required_params,
            });
            if let Some(default) = &param.default {
                wire["default"] = if default == "null" {
                    Value::Null
                } else {
                    json!(default)
                };
            }
            if let Some(domain) = &param.domain {
                wire["enum"] = json!({
                    "domain": domain,
                    "members": signature
                        .domains
                        .iter()
                        .find(|known| &known.domain == domain)
                        .map(|known| {
                            known
                                .members
                                .iter()
                                .map(|member| {
                                    member
                                        .display_name
                                        .clone()
                                        .unwrap_or_else(|| member.id.clone())
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                });
            }
            wire
        })
        .collect::<Vec<_>>();
    json!({ "parameters": parameters })
}

/// One Workshop signature parameter as the renderer's fact view. An enum
/// domain the catalog cannot resolve degrades to the declared type, the
/// same way the owner's own rendering degrades.
fn workshop_param<'a>(
    catalog: &Catalog,
    locale: &Locale,
    signature: &'a workshop_rs::lookup::Signature,
    index: usize,
    param: &'a workshop_rs::lookup::SignatureParam,
) -> ParamView<'a> {
    let domain = param.domain.as_deref().and_then(|domain| {
        catalog.enum_domain(domain).map(|entry| DomainView {
            name: entry.spelling(locale).unwrap_or(domain).to_string(),
            members: entry
                .members
                .iter()
                .map(|member| {
                    catalog
                        .enum_spelling(domain, locale, &member.member)
                        .unwrap_or(member.member.as_str())
                        .to_string()
                })
                .collect(),
        })
    });
    let default = match &param.default {
        Some(declared) if declared == "null" => DefaultView::Null,
        Some(declared) => DefaultView::Text(workshop_default(catalog, locale, declared)),
        None => DefaultView::None,
    };
    ParamView {
        name: &param.name,
        param_type: param.param_type.as_deref(),
        required: index < signature.required_params,
        default,
        domain,
    }
}

/// Render a Workshop declared default in the locale's call syntax: a
/// `Domain.Member` literal resolves to the member's accepted localized
/// spelling, a canonical value id to its localized spelling, and any other
/// literal stays as declared.
fn workshop_default(catalog: &Catalog, locale: &Locale, default: &str) -> String {
    if let Some((domain, member)) = default.split_once('.') {
        if let Some(spelling) = catalog.enum_spelling(domain, locale, member) {
            return spelling.to_string();
        }
    }
    catalog
        .spelling(Kind::Value, locale, default)
        .unwrap_or(default)
        .to_string()
}

/// `within` children for Workshop: the identity selects the members of an
/// enum domain, the parameters of a callable, or the settings keys and
/// segments under a path prefix — resolved in that order, so a canonical
/// domain name or settings prefix cannot be shadowed by a same-named
/// callable spelling. `query` then filters the children by identity or
/// spelling, `kind` by entry kind.
fn workshop_within(
    catalog: &Catalog,
    locale: &Locale,
    within: &str,
    query: Option<&str>,
    kind: Option<&str>,
) -> Result<Vec<Value>, ToolResponse> {
    let mut entries = workshop_within_enum(catalog, locale, within)
        .or_else(|| workshop_within_callable(catalog, locale, within))
        .or_else(|| workshop_within_settings(within))
        .ok_or_else(|| ToolResponse::Error {
            error: ToolErrorInfo {
                code: UNKNOWN_WITHIN.to_string(),
                message: format!("within names no known Workshop scope: {within:?}"),
            },
        })?;
    if let Some(query) = query.filter(|query| !query.is_empty()) {
        // Matching and ranking stay with the owner (ADR-0021 decision 4):
        // a scoped query is the owner's ranked unscoped matches intersected
        // with the scope's children by identity. Children outside the
        // owner's match vocabulary — parameters and path segments — drop,
        // exactly as they cannot answer an unscoped request.
        let matches = catalog
            .lookup(locale, query)
            .map_err(|error| refusal("lookup-failed", error.to_string()))?;
        let mut rank = HashMap::new();
        for (index, found) in matches.iter().enumerate() {
            if let Some(identity) = match_identity(found) {
                rank.entry(identity).or_insert(index);
            }
        }
        entries.retain(|entry| rank.contains_key(entry["identity"].as_str().unwrap_or_default()));
        entries.sort_by_key(|entry| rank[entry["identity"].as_str().unwrap_or_default()]);
    }
    entries.retain(|entry| kind.is_none_or(|k| entry["kind"] == k));
    Ok(entries)
}

/// The parameters of one Workshop callable, in call order. `value` is the
/// canonical id or a localized spelling the catalog resolves.
fn workshop_within_callable(catalog: &Catalog, locale: &Locale, value: &str) -> Option<Vec<Value>> {
    let entry = [Kind::Action, Kind::Value].into_iter().find_map(|kind| {
        catalog
            .entry(kind, value)
            .or_else(|| catalog.resolve(kind, locale, value))
    })?;
    let required = entry.required_param_count();
    let mut entries = Vec::with_capacity(entry.param_count());
    for index in 0..entry.param_count() {
        let name = entry.param_name(index).unwrap_or_default().to_string();
        let mut parameter = json!({
            "type": entry.param_type(index).unwrap_or("any"),
            "required": index < required,
        });
        if let Some(default) = entry.param_default(index) {
            parameter["default"] = if default == "null" {
                Value::Null
            } else {
                json!(default)
            };
        }
        if let Some(domain) = entry.param_domain(index) {
            parameter["enum"] = json!({
                "domain": domain,
                "members": catalog
                    .enum_domain(domain)
                    .map(|domain_entry| {
                        domain_entry
                            .members
                            .iter()
                            .map(|member| {
                                catalog
                                    .enum_spelling(domain, locale, &member.member)
                                    .unwrap_or(member.member.as_str())
                                    .to_string()
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            });
        }
        entries.push(json!({
            "identity": format!("{}.{}", entry.id, name),
            "kind": "parameter",
            "spelling": name.clone(),
            "displayName": name,
            "parameter": parameter,
        }));
    }
    Some(entries)
}

/// The members of one Workshop enum domain — a catalog domain or a settings
/// enum domain — in the domain's order. `value` is the canonical domain
/// name or a spelling the catalog resolves.
fn workshop_within_enum(catalog: &Catalog, locale: &Locale, value: &str) -> Option<Vec<Value>> {
    let domain = if catalog.enum_domain(value).is_some() {
        Some(value.to_string())
    } else {
        catalog
            .resolve_enum_domain(locale, value)
            .map(str::to_string)
    };
    if let Some(domain) = domain {
        let domain_entry = catalog.enum_domain(&domain)?;
        return Some(
            domain_entry
                .members
                .iter()
                .map(|member| {
                    let spelling = catalog
                        .enum_spelling(&domain, locale, &member.member)
                        .unwrap_or(member.member.as_str())
                        .to_string();
                    json!({
                        "identity": format!("{domain}.{}", member.member),
                        "kind": "enumMember",
                        "spelling": spelling.clone(),
                        "displayName": spelling,
                    })
                })
                .collect(),
        );
    }
    // A settings enum domain answers under the same identity.
    let members: Vec<Value> = settings::definitions()
        .filter(|definition| {
            matches!(
                definition.domain(),
                SettingValueDomain::Enum { domain } if domain.as_str() == value
            )
        })
        .flat_map(|definition| {
            definition
                .enum_members()
                .map(|member| {
                    let spelling = member.english_name().to_string();
                    json!({
                        "identity": format!("{}.{}", member.domain(), member.id()),
                        "kind": "enumMember",
                        "spelling": spelling.clone(),
                        "displayName": spelling,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect();
    (!members.is_empty()).then_some(members)
}

/// The immediate settings children under `prefix`, matched segment by
/// segment against the canonical settings paths (whose template segments
/// are `<team>` and `<hero>` and match any concrete value): leaf keys
/// become `setting` entries, intermediate segments `settingPath` entries,
/// each `spelling` equal to its own path segment. An empty `prefix` lists
/// the root.
fn workshop_within_settings(prefix: &str) -> Option<Vec<Value>> {
    let prefix_segments: Vec<&str> = if prefix.is_empty() {
        Vec::new()
    } else {
        prefix.split('.').collect()
    };
    let mut children: BTreeMap<String, Value> = BTreeMap::new();
    let mut known_scope = prefix.is_empty();
    for definition in settings::definitions() {
        let segments: Vec<&str> = definition.path().split('.').collect();
        if segments.len() < prefix_segments.len() {
            continue;
        }
        let matches_prefix =
            prefix_segments
                .iter()
                .zip(segments.iter())
                .all(|(asked, declared)| {
                    declared == asked || *declared == "<team>" || *declared == "<hero>"
                });
        if !matches_prefix {
            continue;
        }
        if segments.len() == prefix_segments.len() {
            // The prefix names this leaf itself — an existing but empty
            // scope, not an unknown one.
            known_scope = true;
            continue;
        }
        known_scope = true;
        let child = segments[prefix_segments.len()];
        let identity = if prefix.is_empty() {
            child.to_string()
        } else {
            format!("{prefix}.{child}")
        };
        if segments.len() == prefix_segments.len() + 1 {
            children.entry(identity).or_insert_with(|| {
                json!({
                    "identity": definition.path().to_string(),
                    "kind": "setting",
                    "spelling": child,
                    "displayName": definition.presentation().english_name,
                    "setting": setting_facts(&definition),
                })
            });
        } else {
            children.entry(identity.clone()).or_insert_with(|| {
                json!({
                    "identity": identity.clone(),
                    "kind": "settingPath",
                    "spelling": child,
                    "displayName": identity,
                })
            });
        }
    }
    known_scope.then(|| children.into_values().collect())
}

/// The `setting` fact object: the value domain the key accepts, with its
/// numeric bounds or enum domain (`name-lookup.md` §21.2).
fn setting_facts(definition: &SettingDefinition) -> Value {
    let domain = definition.domain();
    let mut wire = json!({ "type": domain.kind() });
    match domain {
        SettingValueDomain::Number(bounds) | SettingValueDomain::Percent(bounds) => {
            if let Some(min) = bounds.min() {
                wire["minimum"] = json!(min);
            }
            if let Some(max) = bounds.max() {
                wire["maximum"] = json!(max);
            }
        }
        SettingValueDomain::Enum { domain } => {
            wire["enum"] = json!({
                "domain": domain,
                "members": definition
                    .enum_members()
                    .map(|member| member.english_name().to_string())
                    .collect::<Vec<_>>(),
            });
        }
        _ => {}
    }
    wire
}

// -- opy -------------------------------------------------------------------

/// `opy` lookup answers through the configured provider's LPP 1.5
/// `lpp/lookup` capability. A missing provider, a session that cannot
/// negotiate 1.5, or a negotiated session without `capabilities.lookup`
/// reports `unavailable` naming `opy-rs` — never a text fallback; transport
/// failures after negotiation are ordinary structured errors.
fn opy_lookup(
    session: &CompilerSession,
    query: Option<&str>,
    kind: Option<&str>,
    within: Option<&str>,
    locale: Option<&str>,
    limit: usize,
) -> ToolResponse {
    let mut provider = match session.language_provider(opy_provider::OPY_LANGUAGE_ID) {
        Ok(provider) => provider,
        Err(error) => {
            return unavailable("opy", OWNER_OPY, error.to_string());
        }
    };
    let client = wright_lpp::ClientInfo {
        name: SERVICE_NAME.to_string(),
        version: SERVICE_VERSION.to_string(),
    };
    let negotiated = match provider.initialize_lookup(Some(&client)) {
        Ok(result) => result.capabilities,
        Err(error) => {
            let _ = provider.shutdown();
            return match error.code() {
                "protocol-version-mismatch" | "capability-unavailable" => {
                    unavailable("opy", OWNER_OPY, error.to_string())
                }
                _ => refusal(error.code(), error.to_string()),
            };
        }
    };
    if !negotiated.supports(Capability::Lookup) {
        let _ = provider.shutdown();
        return unavailable(
            "opy",
            OWNER_OPY,
            "opy-rs negotiated an LPP session without the 'lookup' capability",
        );
    }
    let request = |within: Option<LookupWithin>| LookupParams {
        language_id: opy_provider::OPY_LANGUAGE_ID.to_string(),
        query: query.map(str::to_string),
        kind: kind.map(str::to_string),
        within,
        locale: locale.map(str::to_string),
        limit: Some(limit as u64),
    };
    // A `within` identity selects the scope the provider resolves it as:
    // an enum domain, a callable, or a settings prefix, tried in that
    // order. The identity is opaque and passed through unchanged: the
    // protocol requires a provider to accept the identities it issued as
    // `within` values for the matching selector kinds, so Wright never
    // interprets them. When every kind refuses with `lookup.unknownWithin`,
    // the last refusal is the answer.
    let outcome = match within {
        Some(within) => {
            let mut outcome = None;
            for kind in WITHIN_KINDS {
                let response = provider.lookup(&request(Some(LookupWithin {
                    kind: kind.to_string(),
                    value: within.to_string(),
                })));
                let unknown = matches!(
                    &response,
                    Err(error) if error.refusal_code() == Some(UNKNOWN_WITHIN)
                );
                outcome = Some(response);
                if !unknown {
                    break;
                }
            }
            outcome.expect("the three selector kinds always produce a response")
        }
        None => provider.lookup(&request(None)),
    };
    let _ = provider.shutdown();
    let mut entries = match outcome {
        Ok(answer) => answer.entries,
        Err(error) => {
            return refusal(
                error.refusal_code().unwrap_or_else(|| error.code()),
                error.to_string(),
            );
        }
    };
    for entry in &mut entries {
        if let Some(callable) = entry.get("callable").cloned() {
            if let Some(signature) = lpp_signature(entry, &callable) {
                entry["signature"] = json!(signature);
            }
        }
    }
    result("opy", OWNER_OPY, entries)
}

// -- signature rendering ----------------------------------------------------
//
// One renderer serves both languages (ADR-0021 decision 4): parameters in
// call order, `name: Type` for a required parameter, `name=default` or
// `name?` for an optional one (types of optional non-enum parameters are
// omitted), and `name: Domain(members)` for a required enum parameter whose
// domain has at most INLINE_MEMBER_LIMIT members — once per signature — or
// `name: Domain(N members)` for a larger domain. An optional enum parameter
// shows its default and never lists members.

/// The renderer's view of one parameter, adapted from either owner's facts.
struct ParamView<'a> {
    name: &'a str,
    param_type: Option<&'a str>,
    required: bool,
    default: DefaultView,
    domain: Option<DomainView>,
}

struct DomainView {
    /// The domain's display name in the locale (or the identity tail for
    /// opaque provider identities).
    name: String,
    /// The domain's member spellings, already localized by the owner.
    members: Vec<String>,
}

/// How a parameter's declared default renders in the signature.
enum DefaultView {
    /// No declared default: `name: Type`/`name: Domain(...)` when required,
    /// `name?` when optional.
    None,
    /// A declared null default: `name?`.
    Null,
    /// A declared default already in source terms: `name=<text>`.
    Text(String),
}

fn render_signature(
    head: &str,
    receiver: Option<&str>,
    variadic: bool,
    params: Vec<ParamView<'_>>,
) -> String {
    let mut listed_domains = HashSet::new();
    let mut rendered: Vec<String> = Vec::with_capacity(params.len());
    for param in &params {
        rendered.push(match &param.default {
            DefaultView::Null => format!("{}?", param.name),
            DefaultView::Text(default) => format!("{}={}", param.name, default),
            DefaultView::None => match (param.required, &param.domain) {
                (true, Some(domain)) => {
                    if !listed_domains.insert(domain.name.clone()) {
                        format!("{}: {}", param.name, domain.name)
                    } else if domain.members.len() <= INLINE_MEMBER_LIMIT {
                        format!(
                            "{}: {}({})",
                            param.name,
                            domain.name,
                            domain.members.join("|")
                        )
                    } else {
                        format!(
                            "{}: {}({} members)",
                            param.name,
                            domain.name,
                            domain.members.len()
                        )
                    }
                }
                (true, None) => match param.param_type {
                    Some(param_type) => format!("{}: {}", param.name, param_type),
                    None => param.name.to_string(),
                },
                (false, _) => format!("{}?", param.name),
            },
        });
    }
    if variadic {
        rendered.push("...".to_string());
    }
    let head = match receiver {
        Some(receiver) => format!("{receiver}.{head}"),
        None => head.to_string(),
    };
    format!("{head}({})", rendered.join(", "))
}

/// Render an LPP `callable` entry's signature from its wire facts
/// (`name-lookup.md` §21.2/§21.4). Returns `None` when the callable facts
/// are malformed — the entry still passes through, just without a
/// `signature`.
fn lpp_signature(entry: &Value, callable: &Value) -> Option<String> {
    let head = entry["spelling"].as_str()?;
    let receiver = callable["receiver"].as_str();
    let variadic = callable["variadic"].as_bool().unwrap_or(false);
    let params = callable["parameters"].as_array()?;
    let params = params
        .iter()
        .map(|param| {
            let default = match param.get("default") {
                None => DefaultView::None,
                Some(Value::Null) => DefaultView::Null,
                Some(value) => DefaultView::Text(lpp_default_text(value)),
            };
            ParamView {
                name: param["name"].as_str().unwrap_or_default(),
                param_type: param["type"].as_str(),
                required: param["required"].as_bool().unwrap_or(true),
                default,
                domain: param.get("enum").map(|domain| DomainView {
                    // The rendered domain name is the parameter's declared
                    // type; the enum fact's `domain` is an opaque
                    // provider-issued identity, shown whole only as a
                    // fallback — never parsed (`name-lookup.md` §21.4).
                    name: param["type"]
                        .as_str()
                        .or_else(|| domain["domain"].as_str())
                        .unwrap_or_default()
                        .to_string(),
                    members: domain["members"]
                        .as_array()
                        .map(|members| {
                            members
                                .iter()
                                .filter_map(|member| member.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default(),
                }),
            }
        })
        .collect();
    Some(render_signature(head, receiver, variadic, params))
}

/// An LPP scalar default in source terms: strings are already source
/// spellings; other scalars print as their JSON literal.
fn lpp_default_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
