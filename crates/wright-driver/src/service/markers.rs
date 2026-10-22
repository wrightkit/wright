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
    /// `{file:N}` against the served file table, the name-listing
    /// marker as the operation name (`the 'rules' operation`), and the
    /// final `unescape` once both expansions ran.
    fn render_error_markers(&self, message: String) -> String {
        let message = match self.snapshot.as_ref().map(|snapshot| &snapshot.loaded) {
            Some(loaded) => render_file_markers(&message, loaded),
            None => message,
        };
        let message = render_list_markers(&message, |operation, _unbounded| {
            format!("the '{operation}' operation")
        });
        wright_analyzer::service::marker::unescape(&message)
    }
}

/// Expand the `{marker:…}` tokens name-resolution messages carry
/// (ADR-0035, #888): `render` answers each token's replacement, `None`
/// keeps the `{` as literal text. `{{` is escaped user content: both
/// braces copy through verbatim — the surface's final `unescape` renders
/// the literal `{` only after every expansion pass, so an escaped
/// `{list:…}` can never re-expand on a later pass.
fn expand_markers(message: &str, mut render: impl FnMut(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        rest = &rest[open..];
        if rest.as_bytes().get(1) == Some(&b'{') {
            out.push_str("{{");
            rest = &rest[2..];
            continue;
        }
        let Some(inner) = rest[1..].find('}') else {
            break;
        };
        let close = 1 + inner;
        match render(&rest[1..close]) {
            Some(rendered) => {
                out.push_str(&rendered);
                rest = &rest[close + 1..];
            }
            // Not a marker — emit the `{` and keep scanning: a stray
            // brace must not swallow a real marker behind it.
            None => {
                out.push('{');
                rest = &rest[1..];
            }
        }
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

#[cfg(test)]
mod tests {
    use super::expand_markers;
    use wright_analyzer::service::marker::{escape, unescape};

    /// A stub `file:N` renderer — expansion shape only, no session.
    fn file(token: &str) -> Option<String> {
        token
            .strip_prefix("file:")
            .map(|index| format!("path{index}.ws"))
    }

    // The scanner consumes `{{` as an escaped literal before marker
    // parsing (#888): only a single-braced trusted token expands.
    #[test]
    fn escaped_and_unknown_braces_stay_literal() {
        for (message, rendered) in [
            ("{{file:0}}", "{{file:0}}"),
            ("{file:0}", "path0.ws"),
            ("{{x}}", "{{x}}"),
            ("{{", "{{"),
            ("{", "{"),
            ("{file:", "{file:"),
            ("{unknown:x}", "{unknown:x}"),
            ("a{file:0}b{file:1}c", "apath0.wsbpath1.wsc"),
            ("{file:0}{{file:1}}", "path0.ws{{file:1}}"),
        ] {
            assert_eq!(expand_markers(message, file), rendered, "{message}");
        }
    }

    // Composition-time escaping (#888): user text spelled like a marker
    // passes every expansion pass untouched and the final `unescape`
    // returns it verbatim.
    #[test]
    fn escaped_content_round_trips_through_expansion() {
        for text in [
            "{file:0}",
            "{list:rules}",
            "a{list:rules}b",
            "{{",
            "{",
            "{file:",
            "{unknown:x}",
            "plain",
        ] {
            assert_eq!(
                unescape(&expand_markers(&escape(text), file)),
                *text,
                "{text}"
            );
        }
    }
}
