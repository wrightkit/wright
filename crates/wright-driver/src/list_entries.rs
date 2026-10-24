use serde_json::{Value, json};
use workshop_rs::catalog::{Catalog, Kind, Locale};

use crate::CompilerSession;
use crate::input::sha256_hex;
use crate::lookup::{
    OWNER_OPY, OWNER_WORKSHOP, UNKNOWN_WITHIN, quoted_list, unavailable, workshop_entry,
    workshop_locale, workshop_within,
};
use crate::service::{ToolResponse, carried_refusal, refusal};

const LIST_ENTRIES_CAPABILITY: &str = "listEntries";
const DEFAULT_PAGE_SIZE: usize = 100;
const MAX_PAGE_SIZE: usize = 500;
const LISTABLE_KINDS: [&str; 6] = [
    "action",
    "value",
    "event",
    "enum",
    "enumMember",
    "structural",
];

pub(crate) fn list_entries(
    session: &CompilerSession,
    language: &str,
    kind: &str,
    within: Option<&str>,
    locale: Option<&str>,
    page_size: Option<usize>,
    cursor: Option<&str>,
) -> ToolResponse {
    match language {
        "workshop" => {}
        "opy" => {
            let mut response = unavailable(
                "opy",
                OWNER_OPY,
                LIST_ENTRIES_CAPABILITY,
                "listing OverPy vocabulary needs a negotiated LPP capability that opy-rs does not provide",
            );
            if let ToolResponse::Ok { result } = &mut response {
                result["total"] = json!(0);
            }
            return response;
        }
        other => {
            return refusal(
                "invalid-language",
                format!("listEntries serves 'workshop' and 'opy'; got '{other}'"),
            );
        }
    }
    let list_kind = match kind {
        "action" => Some(Kind::Action),
        "value" => Some(Kind::Value),
        "event" => Some(Kind::Event),
        "enum" => Some(Kind::Enum),
        "structural" => Some(Kind::Structural),
        "enumMember" => None,
        _ => {
            return refusal(
                "invalid-kind",
                format!(
                    "kind must be one of {}; got '{kind}'",
                    quoted_list(&LISTABLE_KINDS)
                ),
            );
        }
    };
    match (list_kind.is_none(), within) {
        (true, None) => {
            return refusal("malformed-request", "kind 'enumMember' requires 'within'");
        }
        (false, Some(_)) => {
            return refusal(
                "malformed-request",
                format!("'within' is only valid with kind 'enumMember'; got kind '{kind}'"),
            );
        }
        _ => {}
    }
    let page_size = page_size.unwrap_or(DEFAULT_PAGE_SIZE);
    if !(1..=MAX_PAGE_SIZE).contains(&page_size) {
        return refusal(
            "malformed-request",
            format!("pageSize must be within 1..={MAX_PAGE_SIZE}; got {page_size}"),
        );
    }
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
    let entries: Vec<Value> = match list_kind {
        // The owner lists `Kind::Action`, `Kind::Value`, `Kind::Event`,
        // and enum domains through `lookup_entries`; `Kind::Structural`
        // has no listing surface there (workshop-rs
        // `Catalog::lookup_entries` rejects it), so the callable
        // structural forms — the signature-bearing ones, not the
        // section/suffix keywords — resolve one `lookup` each, the same
        // `LookupMatch::Builtin` and signature `lookup` emits for them.
        Some(Kind::Structural) => catalog
            .entries_of(Kind::Structural)
            .filter_map(|entry| {
                catalog
                    .lookup(&locale, &entry.id)
                    .ok()?
                    .into_iter()
                    .find(|found| {
                        matches!(
                            found,
                            workshop_rs::lookup::LookupMatch::Builtin {
                                kind: Kind::Structural,
                                id,
                                signature: Some(_),
                                ..
                            } if id == &entry.id
                        )
                    })
                    .and_then(|found| workshop_entry(catalog, &locale, found))
            })
            .collect(),
        Some(list_kind) => match catalog.lookup_entries(&locale, list_kind) {
            Ok(matches) => matches
                .into_iter()
                .filter_map(|found| workshop_entry(catalog, &locale, found))
                .collect(),
            Err(error) => return refusal("lookup-failed", error.to_string()),
        },
        None => {
            let within = within.unwrap_or_default();
            match workshop_within(catalog, &locale, within, None, Some("enumMember")) {
                Ok(members) if !members.is_empty() => members,
                Ok(_) => {
                    return carried_refusal(
                        UNKNOWN_WITHIN,
                        format!("'{within}' is not an enum domain with members"),
                    );
                }
                Err(error) => return error,
            }
        }
    };
    let binding = cursor_binding(catalog, kind, within, &locale);
    let start = match cursor {
        None => 0,
        Some(cursor) => match parse_cursor(cursor, &binding) {
            Some(offset) if offset <= entries.len() => offset,
            _ => {
                return refusal(
                    "invalid-cursor",
                    "the cursor does not belong to this request and owner version; restart from the first page",
                );
            }
        },
    };
    let total = entries.len();
    let end = (start + page_size).min(total);
    let mut result = json!({
        "language": "workshop",
        "owner": OWNER_WORKSHOP,
        "entries": entries[start..end],
        "total": total,
    });
    if end < total {
        result["nextCursor"] = json!(format!("{end:x}.{binding}"));
    }
    ToolResponse::Ok { result }
}

fn cursor_binding(catalog: &Catalog, kind: &str, within: Option<&str>, locale: &Locale) -> String {
    let identity = serde_json::to_string(&catalog.identity()).unwrap_or_default();
    let material = format!(
        "workshop\0{kind}\0{}\0{locale}\0{identity}",
        within.unwrap_or_default()
    );
    sha256_hex(material.as_bytes())[..16].to_string()
}

fn parse_cursor(cursor: &str, binding: &str) -> Option<usize> {
    let (offset, bound) = cursor.split_once('.')?;
    (bound == binding)
        .then(|| usize::from_str_radix(offset, 16).ok())
        .flatten()
}
