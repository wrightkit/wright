use std::collections::HashSet;

use serde_json::{Value, json};
use workshop_rs::catalog::{Catalog, Kind, Locale};
use workshop_rs::lookup::{LookupEnumMember, LookupMatch, SignatureParam};
use workshop_rs::settings::{SettingDefinition, SettingValueDomain};
#[cfg(not(target_arch = "wasm32"))]
use wright_lpp::{Capability, LookupParams, LookupWithin};

use crate::CompilerSession;
#[cfg(not(target_arch = "wasm32"))]
use crate::opy_provider;
#[cfg(not(target_arch = "wasm32"))]
use crate::service::{SERVICE_NAME, SERVICE_VERSION};
use crate::service::{ToolResponse, carried_refusal, refusal};

const DEFAULT_LIMIT: usize = 3;
const MAX_LIMIT: usize = 10;
const INLINE_MEMBER_LIMIT: usize = 32;

pub(crate) const OWNER_WORKSHOP: &str = "workshop-rs";
pub(crate) const OWNER_OPY: &str = "opy-rs";
const LOOKUP_CAPABILITY: &str = "lookup";
const KINDS: [&str; 12] = [
    "action",
    "value",
    "event",
    "enumMember",
    "enum",
    "setting",
    "parameter",
    "settingPath",
    "memberAction",
    "memberValue",
    "structural",
    "operator",
];
pub(crate) const UNKNOWN_WITHIN: &str = "lookup.unknownWithin";
#[cfg(not(target_arch = "wasm32"))]
const WITHIN_KINDS: [&str; 3] = ["enum", "callable", "settings"];

pub(crate) fn lookup(
    session: &CompilerSession,
    language: &str,
    query: Option<&str>,
    kind: Option<&str>,
    within: Option<&str>,
    locale: Option<&str>,
    limit: Option<usize>,
) -> ToolResponse {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return refusal(
            "malformed-request",
            format!("limit must be within 1..={MAX_LIMIT}; got {limit}"),
        );
    }
    if let Some(kind) = kind
        && !KINDS.contains(&kind)
    {
        return refusal(
            "invalid-kind",
            format!("kind must be one of {}; got '{kind}'", quoted_list(&KINDS)),
        );
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

/// `'a', 'b', 'c'` — the kind list an `invalid-kind` refusal recites.
pub(crate) fn quoted_list(kinds: &[&str]) -> String {
    kinds
        .iter()
        .map(|kind| format!("'{kind}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Each member's display name, falling back to its canonical id.
fn member_names(members: &[LookupEnumMember]) -> Vec<String> {
    members
        .iter()
        .map(|member| {
            member
                .display_name
                .clone()
                .unwrap_or_else(|| member.id.clone())
        })
        .collect()
}

/// A declared `"null"` default is JSON `null`; any other default is text.
fn default_json(default: &str) -> Value {
    if default == "null" {
        Value::Null
    } else {
        json!(default)
    }
}

fn result(language: &str, owner: &str, entries: Vec<Value>) -> ToolResponse {
    ToolResponse::Ok {
        result: json!({
            "language": language,
            "owner": owner,
            "entries": entries,
        }),
    }
}

pub(crate) fn unavailable(
    language: &str,
    owner: &str,
    capability: &str,
    message: impl Into<String>,
) -> ToolResponse {
    ToolResponse::Ok {
        result: json!({
            "language": language,
            "owner": owner,
            "entries": [],
            "unavailable": {
                "capability": capability,
                "message": message.into(),
            },
        }),
    }
}

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

pub(crate) fn workshop_locale(
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

fn match_identity(found: &LookupMatch) -> Option<String> {
    match found {
        LookupMatch::Builtin { id, .. } => Some(id.clone()),
        LookupMatch::EnumMember { domain, member, .. } => Some(format!("{domain}.{member}")),
        LookupMatch::EnumDomain { domain, .. } => Some(domain.clone()),
        LookupMatch::Setting { definition, .. } => Some(definition.path().to_string()),
        LookupMatch::Parameter {
            callable, param, ..
        } => Some(format!("{callable}.{}", param.name)),
        LookupMatch::SettingPath { path, .. } => Some(path.clone()),
        _ => None,
    }
}

pub(crate) fn workshop_entry(
    catalog: &Catalog,
    locale: &Locale,
    found: LookupMatch,
) -> Option<Value> {
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
                    "members": member_names(&members),
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
        LookupMatch::Parameter {
            required,
            param,
            domain,
            ..
        } => {
            let mut parameter = json!({
                "type": param
                    .param_type
                    .clone()
                    .unwrap_or_else(|| "any".to_string()),
                "required": required,
            });
            if let Some(default) = &param.default {
                parameter["default"] = default_json(default);
            }
            if let Some(domain) = domain {
                parameter["enum"] = json!({
                    "domain": domain.domain,
                    "members": member_names(&domain.members),
                });
            }
            param_facts(&param, &mut parameter);
            json!({
                "identity": identity,
                "kind": "parameter",
                "spelling": param.name,
                "displayName": param.name,
                "parameter": parameter,
            })
        }
        LookupMatch::SettingPath { path, segment } => json!({
            "identity": identity,
            "kind": "settingPath",
            "spelling": segment,
            "displayName": path,
        }),
        _ => return None,
    })
}

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
                wire["default"] = default_json(default);
            }
            if let Some(domain) = &param.domain {
                wire["enum"] = json!({
                    "domain": domain,
                    "members": signature
                        .domains
                        .iter()
                        .find(|known| &known.domain == domain)
                        .map(|known| member_names(&known.members))
                        .unwrap_or_default(),
                });
            }
            param_facts(param, &mut wire);
            wire
        })
        .collect::<Vec<_>>();
    let mut facts = json!({ "parameters": parameters });
    if let Some(return_type) = &signature.return_type {
        facts["returns"] = json!(return_type);
    }
    if signature.variadic {
        facts["variadic"] = json!(true);
    }
    facts
}

fn param_facts(param: &SignatureParam, fact: &mut Value) {
    if let Some(content) = &param.content {
        let mut wire = json!({});
        if let Some(fold) = &content.fold {
            wire["fold"] = json!({ "call": fold.call, "arg": fold.arg });
        }
        if !content.operations.is_empty() {
            wire["operations"] = json!(content.operations);
        }
        if wire != json!({}) {
            fact["content"] = wire;
        }
    }
    if let Some(label) = &param.display_name {
        fact["displayName"] = json!(label);
    }
}

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

fn workshop_default(catalog: &Catalog, locale: &Locale, default: &str) -> String {
    if let Some((domain, member)) = default.split_once('.')
        && let Some(spelling) = catalog.enum_spelling(domain, locale, member)
    {
        return spelling.to_string();
    }
    catalog
        .spelling(Kind::Value, locale, default)
        .unwrap_or(default)
        .to_string()
}

pub(crate) fn workshop_within(
    catalog: &Catalog,
    locale: &Locale,
    within: &str,
    query: Option<&str>,
    kind: Option<&str>,
) -> Result<Vec<Value>, ToolResponse> {
    let children = catalog
        .lookup_within(locale, within, query)
        .map_err(|error| match &error {
            workshop_rs::WorkshopError::Unknown {
                kind: "lookup scope",
                ..
            }
            | workshop_rs::WorkshopError::UnknownWithCandidates {
                kind: "lookup scope",
                ..
            } => carried_refusal(UNKNOWN_WITHIN, error.to_string()),
            _ => refusal("lookup-failed", error.to_string()),
        })?;
    let mut entries: Vec<Value> = children
        .into_iter()
        .filter_map(|child| workshop_entry(catalog, locale, child))
        .collect();
    for entry in &mut entries {
        if entry["kind"] != "setting" {
            continue;
        }
        if let Some(segment) = entry["identity"]
            .as_str()
            .and_then(|path| path.rsplit('.').next())
            .map(str::to_string)
        {
            entry["spelling"] = json!(segment);
        }
    }
    entries.retain(|entry| kind.is_none_or(|k| entry["kind"] == k));
    Ok(entries)
}

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

/// OPY lookup needs the LPP provider transport — native-only (ADR-0037
/// decision 5); the wasm build reports the capability as unavailable,
/// naming the owning implementation the same way an unconfigured provider
/// does (#959).
#[cfg(target_arch = "wasm32")]
fn opy_lookup(
    _session: &CompilerSession,
    _query: Option<&str>,
    _kind: Option<&str>,
    _within: Option<&str>,
    _locale: Option<&str>,
    _limit: usize,
) -> ToolResponse {
    unavailable(
        "opy",
        OWNER_OPY,
        LOOKUP_CAPABILITY,
        "the WebAssembly build carries no LPP provider transport for 'opy' lookups",
    )
}

#[cfg(not(target_arch = "wasm32"))]
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
            return unavailable("opy", OWNER_OPY, LOOKUP_CAPABILITY, error.to_string());
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
                    unavailable("opy", OWNER_OPY, LOOKUP_CAPABILITY, error.to_string())
                }
                _ => carried_refusal(error.code(), error.to_string()),
            };
        }
    };
    if !negotiated.supports(Capability::Lookup) {
        let _ = provider.shutdown();
        return unavailable(
            "opy",
            OWNER_OPY,
            LOOKUP_CAPABILITY,
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
            return carried_refusal(
                error.refusal_code().unwrap_or_else(|| error.code()),
                error.to_string(),
            );
        }
    };
    for entry in &mut entries {
        if let Some(callable) = entry.get("callable").cloned()
            && let Some(signature) = lpp_signature(entry, &callable)
        {
            entry["signature"] = json!(signature);
        }
    }
    result("opy", OWNER_OPY, entries)
}

struct ParamView<'a> {
    name: &'a str,
    param_type: Option<&'a str>,
    required: bool,
    default: DefaultView,
    domain: Option<DomainView>,
}

struct DomainView {
    name: String,
    members: Vec<String>,
}

enum DefaultView {
    None,
    Null,
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

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
fn lpp_default_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
