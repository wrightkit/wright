//! Refusal-metadata composition (ADR-0035, #888) and the agent-surface
//! [`ToolService::handle`] entry that applies it.
//!
//! Name-resolution emitters produce a neutral `message` plus structured
//! `fields` — `candidates` entries (`address`, `file`/`path`, `line`,
//! `column`), the `listOp`/`listAll` operation that lists the declared
//! names, `declared`/`declaredTotal`, and the queried `name`. The message
//! needs no rewriting: each surface appends its own clause — `wright
//! serve` the operation name (`the 'rules' operation`), the CLI its
//! `wright inspect` command — and resolves `file` indices to the loaded
//! program's path spellings.

use serde_json::json;

use wright_analyzer::service::Request;

use super::{ToolErrorInfo, ToolRequest, ToolResponse, ToolService};
use crate::Loaded;

impl<'a> ToolService<'a> {
    /// Dispatch one request: the shared result, with each refusal's
    /// clauses rendered in the service's operation vocabulary — the CLI
    /// renders its own `wright inspect` spelling on the same result
    /// (#888).
    pub fn handle(&mut self, request: &ToolRequest) -> ToolResponse {
        let response = self.handle_response(request);
        let loaded = self.snapshot.as_ref().map(|snapshot| &snapshot.loaded);
        match response {
            ToolResponse::Error { mut error } => {
                compose_fields(
                    &mut error.message,
                    &mut error.fields,
                    loaded,
                    agent_operation,
                );
                ToolResponse::Error { error }
            }
            ToolResponse::Ok { mut result } => {
                compose_payload(&mut result, loaded, agent_operation);
                ToolResponse::Ok { result }
            }
        }
    }

    fn merge_references_into_usage(
        &self,
        references_result: serde_json::Value,
        usage: serde_json::Value,
    ) -> serde_json::Value {
        let mut payload = usage;
        payload["snapshot"] = json!(self.generation);
        match references_result {
            serde_json::Value::Object(mut selected) => {
                payload["references"] = selected.remove("references").unwrap_or_default();
                if let Some(selection) = selected.remove("selection") {
                    payload["selection"] = selection;
                }
            }
            result => payload["references"] = result,
        }
        payload
    }

    fn handle_response(&mut self, request: &ToolRequest) -> ToolResponse {
        let response = self.handle_full(request);
        let ToolResponse::Ok { mut result } = response else {
            return response;
        };
        match request {
            ToolRequest::Check { all } => crate::essential::envelope(&mut result, *all),
            ToolRequest::Analyze { all, .. } => {
                result["result"] = crate::essential::analyze(&result["result"], *all);
                crate::essential::envelope(&mut result, *all);
            }
            ToolRequest::Inspect { all } => {
                result["result"] = crate::essential::inspect(&result["result"], *all);
                crate::essential::envelope(&mut result, *all);
            }
            ToolRequest::Lint { all, .. } => result = crate::essential::lint(&result, *all),
            ToolRequest::References { symbol, all, .. } => {
                let id = match self.symbol_address(symbol) {
                    Ok(id) => id,
                    Err(error) => return ToolResponse::Error { error },
                };
                let usage = match self.semantic_query(Request::GetUsage { symbol: id }) {
                    ToolResponse::Ok { result } => result,
                    other => return other,
                };
                result =
                    crate::essential::query(&self.merge_references_into_usage(result, usage), *all);
            }
            ToolRequest::CostEstimate { all, .. } => result = crate::essential::cost(&result, *all),
            ToolRequest::Rules { all, .. }
            | ToolRequest::Symbols { all, .. }
            | ToolRequest::Cfg { all, .. }
            | ToolRequest::Findings { all, .. }
            | ToolRequest::PersistentObjects { all }
            | ToolRequest::CallGraph { all, .. } => result = crate::essential::query(&result, *all),
            _ => {}
        }
        ToolResponse::Ok { result }
    }
}

/// A refusal with a Wright-coined `code` — the strict kebab grammar is
/// asserted inside `ToolErrorInfo::new` (#871).
pub(crate) fn refusal(code: &str, message: impl Into<String>) -> ToolResponse {
    ToolResponse::Error {
        error: ToolErrorInfo::new(code, message),
    }
}

/// A refusal whose `code` is owner- or provider-issued vocabulary carried
/// verbatim (the LPP `refusalCode` contract, e.g. `lookup.unknownWithin`):
/// the wire contract fixes the spelling, so the kebab grammar does not
/// apply — only the whitespace-free floor.
pub(crate) fn carried_refusal(code: &str, message: impl Into<String>) -> ToolResponse {
    ToolResponse::Error {
        error: ToolErrorInfo::carried(code, message),
    }
}

/// An unknown selection/filter value is a structured usage error, never a
/// silent empty result (#430, #531).
pub(crate) fn invalid_selection(message: String) -> ToolResponse {
    invalid_selection_fields(message, Default::default())
}

/// The same refusal carrying the emitter's structured fields (#888): a
/// selector whose failure already names candidates or the listing
/// operation keeps them for the surface to render.
pub(crate) fn invalid_selection_fields(
    message: String,
    fields: serde_json::Map<String, serde_json::Value>,
) -> ToolResponse {
    ToolResponse::Error {
        error: ToolErrorInfo {
            fields,
            ..ToolErrorInfo::new("invalid-selection", message)
        },
    }
}

/// The agent surface's `listOp` spelling (#888): the operation name —
/// `the 'rules' operation` — while the CLI names its own `wright
/// inspect` command on the same fields.
fn agent_operation(operation: &str, _unbounded: bool) -> String {
    format!("the '{operation}' operation")
}

/// Resolve a refusal's `candidates` against `loaded` and append
/// `: {address} at {path}:{line}:{column}` to `message` (#888), plus the
/// `unaddressable` rename note. Runs at the emit boundary that owns the
/// file table — each `file` index is rewritten to `path`, so a second
/// call with a different table never re-resolves.
pub(crate) fn resolve_candidates(
    message: &mut String,
    fields: &mut serde_json::Map<String, serde_json::Value>,
    loaded: Option<&Loaded>,
) {
    let candidates = fields
        .get_mut("candidates")
        .and_then(serde_json::Value::as_array_mut);
    if let Some(candidates) = candidates {
        // Entries carrying `path` were rendered by an earlier pass —
        // appending again would double the candidate list.
        let mut rendered = Vec::with_capacity(candidates.len());
        for candidate in candidates.iter_mut() {
            let Some(entry) = candidate.as_object_mut() else {
                continue;
            };
            if entry.contains_key("path") {
                continue;
            }
            let address = entry
                .get("address")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let location = candidate_location(entry, loaded);
            rendered.push(format!("{address} at {location}"));
        }
        if !rendered.is_empty() {
            message.push_str(&format!(": {}", rendered.join(", ")));
        }
    }
    if fields
        .remove("unaddressable")
        .and_then(|value| value.as_bool())
        == Some(true)
    {
        message.push_str("; rename one of the conflicting rules to select it");
    }
}

/// Append this surface's clauses to a refusal's neutral `message`
/// (#888): [`resolve_candidates`] first, then `listOp` appends
/// `; {spelling} lists them{ all}` — `spelling(operation, unbounded)` is
/// the surface's name for the listing operation (`the 'rules' operation`
/// on agent surfaces, `wright inspect rules` on the CLI). A message
/// with no refusal fields is returned untouched.
pub fn compose_fields(
    message: &mut String,
    fields: &mut serde_json::Map<String, serde_json::Value>,
    loaded: Option<&Loaded>,
    spelling: impl Fn(&str, bool) -> String,
) {
    resolve_candidates(message, fields, loaded);
    if let Some(operation) = fields.get("listOp").and_then(serde_json::Value::as_str) {
        let unbounded = fields.get("listAll").and_then(serde_json::Value::as_bool) == Some(true);
        message.push_str(&format!(
            "; {} lists them{}",
            spelling(operation, unbounded),
            if unbounded { " all" } else { "" },
        ));
    }
}

/// One candidate's `path:line:column`: `path` already resolved stays
/// (the emitters resolve against their own file table), a `file` index
/// resolves against `loaded` and is rewritten in place, and a missing
/// location renders `<unknown>`. The rendered `path` is always written
/// back so a later pass never re-appends the same candidate.
fn candidate_location(
    entry: &mut serde_json::Map<String, serde_json::Value>,
    loaded: Option<&Loaded>,
) -> String {
    let file = entry.get("file").and_then(serde_json::Value::as_u64);
    let path = entry
        .get("path")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| match (file, loaded) {
            (Some(file), Some(loaded)) => {
                entry.remove("file");
                Some(crate::session::span_path(Some(file), loaded))
            }
            _ => None,
        });
    let path = path.unwrap_or_else(|| "<unknown>".to_string());
    entry.insert("path".to_string(), path.clone().into());
    match (entry.get("line"), entry.get("column")) {
        (Some(line), Some(column)) if path != "<unknown>" => {
            format!("{path}:{line}:{column}")
        }
        _ => "<unknown>".to_string(),
    }
}

/// Compose the diagnostics embedded in an `Ok` payload (#888): any JSON
/// object carrying refusal fields and a `message` gets this surface's
/// clauses, wherever in the payload it nests. `Ok` payloads carry no
/// other rewriting — the payload itself passes through verbatim.
fn compose_payload(
    value: &mut serde_json::Value,
    loaded: Option<&Loaded>,
    spelling: fn(&str, bool) -> String,
) {
    match value {
        serde_json::Value::Object(object) => {
            if (object.contains_key("listOp") || object.contains_key("candidates"))
                && let Some(serde_json::Value::String(message)) = object.remove("message")
            {
                let mut message = message;
                compose_fields(&mut message, object, loaded, spelling);
                object.insert("message".to_string(), message.into());
            }
            object
                .values_mut()
                .for_each(|value| compose_payload(value, loaded, spelling));
        }
        serde_json::Value::Array(items) => items
            .iter_mut()
            .for_each(|value| compose_payload(value, loaded, spelling)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "violates the code grammar")]
    fn refusal_rejects_a_spaced_code() {
        let _ = refusal("invalid language", "message");
    }

    #[test]
    #[should_panic(expected = "violates the code grammar")]
    fn refusal_rejects_an_uppercase_code() {
        let _ = refusal("Invalid-Language", "message");
    }

    #[test]
    fn carried_refusal_keeps_the_lpp_refusal_code() {
        let response = carried_refusal(crate::lookup::UNKNOWN_WITHIN, "message");
        let ToolResponse::Error { error } = response else {
            panic!("a refusal is always an error response");
        };
        assert_eq!(error.code, "lookup.unknownWithin");
    }

    #[test]
    #[should_panic(expected = "empty or contains whitespace")]
    fn carried_refusal_rejects_whitespace() {
        let _ = carried_refusal("bad code", "message");
    }
}
