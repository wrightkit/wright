//! The agent-surface [`ToolService::handle`] entry and the neutral-marker
//! seam it applies (ADR-0035, #888).
//!
//! Name-resolution refusals arrive carrying markers — `{file:N}` for a
//! source-file location and `{list:OP}`/`{list-all:OP}` for the operation
//! that lists the declared names — so the one driver result stays
//! surface-neutral. Each surface renders them its own way: `wright serve`
//! (and every `handle` caller) the operation name, the CLI its
//! `wright inspect` command spelling.

use serde_json::json;

use wright_analyzer::service::Request;

use super::{ToolRequest, ToolResponse, ToolService};
use crate::Loaded;

impl<'a> ToolService<'a> {
    /// Dispatch one request: the shared result, with each refusal's
    /// markers rendered in the service's operation vocabulary — the CLI
    /// renders its own spelling on the same result (#888).
    pub fn handle(&mut self, request: &ToolRequest) -> ToolResponse {
        let response = self.handle_response(request);
        let ToolResponse::Error { mut error } = response else {
            return response;
        };
        error.message = self.render_error_markers(error.message);
        ToolResponse::Error { error }
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
                let mut payload = usage;
                payload["snapshot"] = json!(self.generation);
                if let Some(selected) = result.as_object_mut() {
                    payload["references"] = selected.remove("references").unwrap_or_default();
                    if let Some(selection) = selected.remove("selection") {
                        payload["selection"] = selection;
                    }
                } else {
                    payload["references"] = result;
                }
                result = crate::essential::query(&payload, *all);
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

    /// Render a refusal's neutral markers for the agent surface (#888):
    /// `{file:N}` against the served file table and the name-listing
    /// marker as the operation name (`the 'rules' operation`).
    fn render_error_markers(&self, message: String) -> String {
        let message = match self.snapshot.as_ref().map(|snapshot| &snapshot.loaded) {
            Some(loaded) => render_file_markers(&message, loaded),
            None => message,
        };
        render_list_markers(&message, |operation, _unbounded| {
            format!("the '{operation}' operation")
        })
    }
}

/// Expand the `{marker:…}` tokens name-resolution messages carry
/// (ADR-0035, #888): `render` answers each token's replacement, `None`
/// keeps the text as written. One driver result carries the marker; each
/// surface supplies the rendering.
fn expand_markers(message: &str, mut render: impl FnMut(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(open) = rest.find('{') {
        let Some(inner) = rest[open + 1..].find('}') else {
            break;
        };
        let close = open + 1 + inner;
        match render(&rest[open + 1..close]) {
            Some(rendered) => {
                out.push_str(&rest[..open]);
                out.push_str(&rendered);
            }
            None => out.push_str(&rest[..=close]),
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

/// Expand `{file:N}` location markers against the loaded program's file
/// table (#888): both surfaces render the same source-file spelling — the
/// analyzer knows only file ids, never the spellings.
pub(crate) fn render_file_markers(message: &str, loaded: &Loaded) -> String {
    expand_markers(message, |token| {
        token
            .strip_prefix(wright_analyzer::service::marker::FILE_PREFIX)
            .and_then(|index| index.parse::<u64>().ok())
            .map(|index| crate::session::span_path(Some(index), loaded))
    })
}

/// Expand `{list:OP}`/`{list-all:OP}` name-listing markers (#888):
/// `render(operation, unbounded)` gives the surface's spelling —
/// `wright serve` the operation name, the CLI its `wright inspect`
/// command.
pub fn render_list_markers(message: &str, mut render: impl FnMut(&str, bool) -> String) -> String {
    expand_markers(message, |token| {
        token
            .strip_prefix(wright_analyzer::service::marker::LIST_ALL_PREFIX)
            .map(|operation| render(operation, true))
            .or_else(|| {
                token
                    .strip_prefix(wright_analyzer::service::marker::LIST_PREFIX)
                    .map(|operation| render(operation, false))
            })
    })
}
